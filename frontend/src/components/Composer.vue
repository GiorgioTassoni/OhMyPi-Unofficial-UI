<script setup lang="ts">
/**
 * The composer's input row and its actions (`docs/12` §5.2, §3.4, §5.1).
 *
 * Owns the draft and the attachments, and nothing else: the session's liveness arrives
 * as props, every operation goes back to the host as one command, and a failure is
 * reported upward rather than swallowed. The chips of §5.1 slot in below the row.
 *
 * Images travel in `prompt{images}`. A text file chosen through `+` is included as
 * labelled UTF-8 text, since browser pickers do not expose its absolute path. A file
 * dropped from the file manager sends its path for the agent to read.
 *
 * The keyboard map is `lib/composer.ts` — pure, and asserted — so this file only
 * arranges what it decides.
 */
import { computed, nextTick, ref, watch } from "vue";
import {
  followUp,
  prompt as sendPrompt,
  searchWorkspaceFiles,
  steer,
  stopTurn,
  stopTurnAndSend,
  type WorkspaceFileMatch,
} from "../bridge";
import { operationFor } from "../lib/composer";
import {
  dispatchOf,
  insertionOf,
  move,
  paletteFor,
  replaceToken,
  WINDOW_ACTIONS,
  type CommandEntry,
} from "../lib/palette";
import { findAtToken } from "../lib/filePalette";
import {
  frameRefusal,
  imagesToSend,
  messageFor,
  nameFromPath,
  roomForAnotherImage,
  type Attachment,
} from "../lib/attachments";
import { readImage } from "../lib/image-encode";
import { pasteImages } from "../lib/image-paste";
import { clipboardImage, readFileBase64 } from "../bridge";
import { readTextAttachment } from "../lib/text-attachment";
import { createPastedTextAttachment, shouldAttachAsPastedText } from "../lib/pastedText";
import { type ChipRow } from "../lib/chips";
import AttachmentStrip from "./AttachmentStrip.vue";
import Icon from "./ui/Icon.vue";
import CommandPalette from "./CommandPalette.vue";
import FilePalette from "./FilePalette.vue";
import ComposerChips from "./ComposerChips.vue";

const props = defineProps<{
  /** The thread this composer speaks for: every command is addressed by its id. */
  thread: string;
  /** A turn is in flight, as `get_state` last reported. */
  streaming: boolean;
  /** Whether any subagents are currently in flight for this thread. */
  hasActiveAgents?: boolean;
  /** No session, so there is nowhere to send. */
  disabled: boolean;
  /** Everything the chip row renders from (`docs/12` §5.1), passed through untouched. */
  chips: ChipRow;
  /**
   * The frame the engine advertised in `ready`; what an attachment has to fit.
   *
   * Zero before a session exists — and nothing can be attached then either, since
   * there is no engine to attach to.
   */
  frameLimit: number;
  /**
   * What the engine advertises for the palette (`docs/12` §7.3).
   *
   * Fetched and kept by the screen rather than here: the list is session state that
   * changes when the engine says so, and the composer only reads it.
   */
  commands: CommandEntry[];
}>();

const emit = defineEmits<{
  (event: "failed", message: string): void;
  /** A chip changed something about the session, so the screen re-reads it. */
  (event: "changed"): void;
  /** A chip asked for another directory. */
  (event: "workspace", path: string): void;
  /** The folder chip's terminal entry: the app's own shell, in this session's directory. */
  (event: "terminal", path: string): void;
  /** The favourites order changed. */
  (event: "favourites", keys: string[]): void;
  /** A palette row for something the *window* owns, which the shell opens (`docs/12` §12). */
  (event: "app-action", id: string): void;
  /** A steer was submitted, providing optimistic feedback while awaiting engine interrupt. */
  (event: "steering", payload: { text: string }): void;
}>();

const draft = ref("");
/** One send at a time: two Enters must not become two prompts. */
const sending = ref(false);
const steeringPending = ref(false);
const stopping = ref(false);
const readingFiles = ref(false);
const attachments = ref<Attachment[]>([]);

/** `draft` is emptied by a send as much as by hand, so the height follows every path. */
watch(draft, () => void nextTick(grow));
/** The last refusal, shown until the next successful attach or send. */
const refusal = ref<string | null>(null);

/** The box itself, for the caret the palette is derived from. */
const box = ref<HTMLTextAreaElement | null>(null);
const picker = ref<HTMLInputElement | null>(null);
/** A drag is over the composer, so it reads as a target. */

/**
 * Where the caret is, as of the last thing the box reported.
 *
 * The palette is derived from the text *before* the caret, not from the whole draft: a
 * palette driven by the whole box would stay open while someone typed an argument in the
 * middle of a message.
 *
 * Every way the caret can move updates this — a key (read before the key applies, which
 * is what the key's *meaning* depends on), the edit that key produced, a click, a
 * selection. Measured the hard way: with only the keydown reader, a box filled by
 * anything other than typing (a test harness, a paste script, a future draft-restore)
 * left the caret at zero and the palette filtering against an empty string.
 */
const caret = ref(0);
/** The highlighted row. */
const paletteIndex = ref(0);
/** `Esc` puts the palette away without touching the text, until the next edit. */
const paletteDismissed = ref(false);

const before = computed(() => draft.value.slice(0, caret.value));
const palette = computed(() =>
  paletteDismissed.value ? null : paletteFor(before.value, props.commands, WINDOW_ACTIONS),
);
/** The highlighted row, clamped: the list narrows as the user types. */
const selected = computed(() => {
  const items = palette.value?.items ?? [];
  if (items.length === 0) {
    return null;
  }

  return items[Math.min(paletteIndex.value, items.length - 1)] ?? null;
});

// Any edit re-derives the palette from scratch: a dismissal is not permanent, and the
// highlight belongs to the list as it now stands.
watch(draft, () => {
  paletteDismissed.value = false;
  paletteIndex.value = 0;
  fileIndex.value = 0;
});

const atToken = computed(() => (paletteDismissed.value ? null : findAtToken(before.value)));
const fileMatches = ref<WorkspaceFileMatch[]>([]);
const fileIndex = ref(0);
const isFilePaletteOpen = computed(() => atToken.value !== null && fileMatches.value.length > 0);

let searchSeq = 0;
watch(
  [() => props.thread, () => (atToken.value ? `${atToken.value.start}:${atToken.value.query}` : null)],
  async ([thread, tokenKey]) => {
    if (!tokenKey || !thread || !atToken.value) {
      fileMatches.value = [];
      fileIndex.value = 0;
      return;
    }
    const query = atToken.value.query;
    const seq = ++searchSeq;
    try {
      const results = await searchWorkspaceFiles(thread, query, 25);
      if (seq === searchSeq) {
        fileMatches.value = results;
        fileIndex.value = 0;
      }
    } catch {
      if (seq === searchSeq) {
        fileMatches.value = [];
        fileIndex.value = 0;
      }
    }
  },
  { immediate: true },
);

const hasDraft = computed(() => draft.value.trim() !== "");
/** An attachment alone is enough to send a turn. */
const hasInput = computed(() => hasDraft.value || attachments.value.length > 0);
const canSend = computed(
  () => hasInput.value && !props.disabled && !sending.value && !readingFiles.value,
);
/** The message as it would go out, paths included — which is what the frame carries. */
const outbound = computed(() => messageFor(draft.value, attachments.value));

type Sendable = "prompt" | "steer" | "follow-up" | "stop-and-send";

/**
 * Take one image the browser handed over: a paste, or a file chosen through `+`.
 *
 * The room it has to fit is measured against the message as it stands, so a second
 * image is budgeted after the first rather than against the frame on its own.
 */
async function attachImage(blob: Blob, name: string): Promise<void> {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }

  try {
    const room = roomForAnotherImage(
      props.frameLimit,
      outbound.value,
      attachments.value,
    );
    const attachment = await readImage(blob, name, crypto.randomUUID(), room);
    attachments.value = [...attachments.value, attachment];
    refusal.value = null;
  } catch (cause) {
    // Named, with both sizes, by `lib/attachments.ts` — an image that cannot be sent
    // is refused here rather than dropped quietly or discovered by a failed send.
    refusal.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function attachText(file: File): Promise<void> {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }
  try {
    const attachment = await readTextAttachment(file, crypto.randomUUID());
    const next = [...attachments.value, attachment];
    const tooLarge = frameRefusal(props.frameLimit, draft.value, next);
    if (tooLarge !== null) {
      refusal.value = `${file.name} cannot be attached: ${tooLarge}`;
      return;
    }
    attachments.value = next;
    refusal.value = null;
  } catch (cause) {
    refusal.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function attachFiles(files: File[]): Promise<void> {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }
  readingFiles.value = true;
  try {
    for (const file of files) {
      const raster = (file.type.startsWith("image/") && file.type !== "image/svg+xml")
        || /\.(png|jpe?g|gif|webp)$/i.test(file.name);
      if (raster) {
        await attachImage(file, file.name);
      } else {
        await attachText(file);
      }
    }
  } finally {
    readingFiles.value = false;
  }
}

function attachPastedText(text: string): void {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }
  try {
    const attachment = createPastedTextAttachment(text, attachments.value);
    const next = [...attachments.value, attachment];
    const tooLarge = frameRefusal(props.frameLimit, draft.value, next);
    if (tooLarge !== null) {
      refusal.value = `Pasted text cannot be attached: ${tooLarge}`;
      return;
    }
    attachments.value = next;
    refusal.value = null;
  } catch (cause) {
    refusal.value = cause instanceof Error ? cause.message : String(cause);
  }
}

/**
 * Take the paths a drop delivered.
 *
 * If a dropped path is an image, we attempt to read it directly as a base64 image
 * attachment so multimodal models can inspect it. Otherwise, it is held as a path
 * attachment that the agent reads with its tools.
 */
async function attachPaths(paths: readonly string[]): Promise<void> {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }

  const seen = new Set(
    attachments.value.flatMap((attachment) =>
      attachment.kind === "path" ? [attachment.path] : [],
    ),
  );

  const pathsToProcess = paths.filter((path) => (seen.has(path) ? false : seen.add(path) !== undefined));
  if (pathsToProcess.length === 0) return;

  readingFiles.value = true;
  try {
    for (const path of pathsToProcess) {
      const isRaster = /\.(png|jpe?g|gif|webp)$/i.test(path);
      if (isRaster) {
        try {
          const b64 = await readFileBase64(null, path);
          const ext = path.split(".").pop()?.toLowerCase();
          const mime = ext === "png" ? "image/png" : ext === "webp" ? "image/webp" : ext === "gif" ? "image/gif" : "image/jpeg";
          const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
          const blob = new Blob([bytes], { type: mime });
          await attachImage(blob, nameFromPath(path));
          continue;
        } catch {
          // If reading base64 fails, fall back to path attachment below
        }
      }

      const item: Attachment = {
        id: crypto.randomUUID(),
        kind: "path",
        name: nameFromPath(path),
        path,
      };
      attachments.value = [...attachments.value, item];
      refusal.value = null;
    }
  } finally {
    readingFiles.value = false;
  }
}

async function onPicked(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  // Cleared before the reads, so choosing the same file twice still fires a change.
  input.value = "";
  await attachFiles(files);
}

async function onPaste(event: ClipboardEvent): Promise<void> {
  if (props.disabled) {
    return;
  }

  readingFiles.value = true;
  try {
    const clipboardItems = Array.from(event.clipboardData?.items ?? []);
    const hasImage = clipboardItems.some((item) => item.kind === "file" && item.type.startsWith("image/"));
    if (hasImage) {
      await pasteImages(event, clipboardImage, (image) => attachImage(image, "pasted image"));
      return;
    }

    const pastedText = event.clipboardData?.getData("text/plain");
    if (pastedText && shouldAttachAsPastedText(pastedText)) {
      event.preventDefault();
      attachPastedText(pastedText);
      return;
    }

    // Otherwise fallback to native image paste (e.g. Linux empty WebKit event)
    await pasteImages(event, clipboardImage, (image) => attachImage(image, "pasted image"));
  } catch (cause) {
    refusal.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    readingFiles.value = false;
  }
}

function remove(id: string): void {
  attachments.value = attachments.value.filter((attachment) => attachment.id !== id);
  refusal.value = null;
}

async function send(operation: Sendable): Promise<void> {
  if (!canSend.value) {
    return;
  }

  // Checked again here, not only when each attachment arrived: a draft can grow past
  // the frame afterwards, and the transport would refuse the send after the fact.
  const tooLarge = frameRefusal(props.frameLimit, draft.value, attachments.value);
  if (tooLarge !== null) {
    refusal.value = tooLarge;
    return;
  }

  const text = messageFor(draft.value, attachments.value);
  const images = imagesToSend(attachments.value);
  const heldDraft = draft.value;
  const heldAttachments = attachments.value;

  sending.value = true;
  refusal.value = null;

  if (operation === "steer") {
    steeringPending.value = true;
    emit("steering", { text });
  } else {
    draft.value = "";
    attachments.value = [];
  }

  try {
    switch (operation) {
      case "prompt":
        await sendPrompt(props.thread, text, images);
        break;
      case "steer":
        await steer(props.thread, text, images);
        draft.value = "";
        attachments.value = [];
        break;
      case "follow-up":
        await followUp(props.thread, text, images);
        break;
      case "stop-and-send":
        await stopTurnAndSend(props.thread, text, images);
        emit("changed");
        break;
    }
  } catch (cause) {
    if (operation === "steer") {
      emit("steering", { text: "" });
    }
    // Give back what was sent rather than lose it — but only if nothing was typed or
    // attached in the meantime, so an error cannot overwrite newer work. The draft
    // comes back as it was typed, not as it was sent: the paths live on the chips.
    if (draft.value === "") {
      draft.value = heldDraft;
    }
    if (attachments.value.length === 0) {
      attachments.value = heldAttachments;
    }
    emit("failed", String(cause));
  } finally {
    sending.value = false;
    steeringPending.value = false;
  }
}

/**
 * Put a palette row's text in the box and leave the caret after it.
 *
 * The caret is where the levels come from: `/security ` is the subcommand list and
 * `/security scan` is a message, so completing a row is the same edit either way.
 */
async function insert(text: string): Promise<void> {
  const tokenStart = palette.value?.tokenStart ?? 0;
  const next = replaceToken(draft.value, caret.value, text, tokenStart);
  draft.value = next.text;
  caret.value = next.caret;

  // The box holds the text; give the caret back to it once Vue has painted the new value.
  await nextTick();
  box.value?.focus();
  box.value?.setSelectionRange(next.caret, next.caret);
}

/**
 * The box's own edit, hand-written rather than `v-model`.
 *
 * Measured the hard way: `v-model="draft"` **plus** an explicit `@input` on a native
 * element is not two handlers — the compiler keeps the explicit one and drops the model's,
 * so the box filled with text and the component never heard about it. Nothing typed in
 * this app would have reached the draft. One handler that takes both facts from the same
 * event is the honest version.
 */
function onInput(event: Event): void {
  const element = event.target;
  if (!(element instanceof HTMLTextAreaElement)) {
    return;
  }

  draft.value = element.value;
  caret.value = element.selectionStart ?? element.value.length;
  grow();
}

/**
 * Keep the box exactly as tall as its text.
 *
 * One line when it is empty — which is what the reference's card is, and what a one-line prompt
 * deserves — and it grows as the prompt does. Two rows of height for an empty box is a card that
 * is mostly nothing, and it pushes the chips row further from the prompt they belong to.
 *
 * `height: auto` first: `scrollHeight` is the height of the content *at the current height*, so
 * without the reset a box that has grown never shrinks back.
 */
function grow(): void {
  const element = box.value;
  if (element === null) return;
  element.style.height = "auto";
  element.style.height = `${Math.max(28, element.scrollHeight)}px`;
}

/** The caret a box event reports, or the end of the text when it reports none. */
function caretOf(event: Event): number {
  const element = event.target;
  if (!(element instanceof HTMLTextAreaElement)) {
    return draft.value.length;
  }

  return element.selectionStart ?? element.value.length;
}

/** Run a row that needs nothing more from the user. */
async function dispatch(text: string): Promise<void> {
  if (props.disabled || sending.value) {
    return;
  }

  sending.value = true;
  // The command replaces the box: what is sent is what the row said, and nothing the
  // palette covered stays behind it. Attachments are left alone — a command is not a
  // message with pictures, and whoever attached them still wants them.
  draft.value = "";

  try {
    await sendPrompt(props.thread, text, []);
  } catch (cause) {
    draft.value = text;
    emit("failed", String(cause));
  } finally {
    sending.value = false;
  }
}

/**
 * `Enter` in the palette: run the row, or finish its name when it wants arguments.
 *
 * A command that needs something typed after it is never dispatched here — that is the
 * one rule of §7.3 that a highlighted row makes easy to get wrong. Neither is anything
 * dispatched while a turn is streaming: `prompt` is rejected then (measured in 8a), so
 * the command is offered as text to send or steer with instead of a refusal.
 */
async function accept(): Promise<void> {
  const item = selected.value;
  if (item === null) {
    return;
  }

  // A window action is not a command: the token that opened the palette is cleared and the
  // shell is told, because sending the word to the engine would be a prompt it cannot answer.
  // *Which* action goes with it: the rows are not all the same thing, and a composer that
  // answered every one of them with settings was only right while there was one.
  if (item.kind === "app") {
    await insert("");
    emit("app-action", item.app.id);
    return;
  }

  const isMidPrompt = (palette.value?.tokenStart ?? 0) > 0;
  const text = props.streaming || isMidPrompt ? null : dispatchOf(item);
  if (text === null) {
    await insert(insertionOf(item));
    return;
  }

  await dispatch(text);
}

/**
 * Accept a selected file match into the draft at the current `@` token.
 */
async function acceptFile(index?: number): Promise<void> {
  const i = index ?? fileIndex.value;
  const match = fileMatches.value[i];
  if (!match || !atToken.value) return;
  const tokenStart = atToken.value.start;
  const replacement = `@${match.path} `;
  const next = replaceToken(draft.value, caret.value, replacement, tokenStart);
  draft.value = next.text;
  caret.value = next.caret;
  fileMatches.value = [];
  fileIndex.value = 0;
  await nextTick();
  box.value?.focus();
  box.value?.setSelectionRange(next.caret, next.caret);
}

async function stop(): Promise<void> {
  if (stopping.value) return;
  stopping.value = true;
  try {
    await stopTurn(props.thread);
    emit("changed");
  } catch (cause) {
    const msg = String(cause);
    if (!msg.includes("no live thread")) {
      emit("failed", msg);
    }
  } finally {
    stopping.value = false;
  }
}

async function onKeydown(event: KeyboardEvent): Promise<void> {
  // Read before the key applies: the palette is derived from the text up to the caret, and
  // this handler decides what the key means for exactly that text.
  caret.value = event.currentTarget instanceof HTMLTextAreaElement
    ? event.currentTarget.selectionStart ?? draft.value.length
    : draft.value.length;

  const isPaletteActive = palette.value !== null;
  const isFilesActive = !isPaletteActive && isFilePaletteOpen.value;

  const operation = operationFor(
    {
      key: event.key,
      alt: event.altKey,
      shift: event.shiftKey,
      composing: event.isComposing,
    },
    {
      streaming: props.streaming,
      draft: draft.value,
      attachments: attachments.value.length,
      palette: isPaletteActive || isFilesActive,
    },
  );

  switch (operation) {
    case "none":
    case "newline":
      // `newline` is the text area's own behaviour; letting it through is the point.
      return;

    case "clear":
      event.preventDefault();
      draft.value = "";
      attachments.value = [];
      refusal.value = null;
      return;

    case "abort":
      event.preventDefault();
      await stop();
      return;

    case "palette-down":
      event.preventDefault();
      if (isPaletteActive) {
        paletteIndex.value = move("down", paletteIndex.value, palette.value?.items.length ?? 0);
      } else if (isFilesActive) {
        fileIndex.value = move("down", fileIndex.value, fileMatches.value.length);
      }
      return;

    case "palette-up":
      event.preventDefault();
      if (isPaletteActive) {
        paletteIndex.value = move("up", paletteIndex.value, palette.value?.items.length ?? 0);
      } else if (isFilesActive) {
        fileIndex.value = move("up", fileIndex.value, fileMatches.value.length);
      }
      return;

    case "palette-accept":
      event.preventDefault();
      if (isPaletteActive) {
        await accept();
      } else if (isFilesActive) {
        await acceptFile();
      }
      return;

    case "palette-complete": {
      event.preventDefault();
      if (isPaletteActive) {
        const item = selected.value;
        // A window action has nothing to complete — it is not text the box will send.
        if (item !== null && item.kind !== "app") {
          await insert(insertionOf(item));
        }
      } else if (isFilesActive) {
        await acceptFile();
      }
      return;
    }

    case "palette-close":
      event.preventDefault();
      paletteDismissed.value = true;
      return;

    default:
      // Enter would otherwise insert a newline into the box we just emptied.
      event.preventDefault();
      await send(operation);
  }
}

/**
 * Put the engine's own text in the box (the `editor-text` chrome op, `lib/chrome.ts`).
 *
 * Called by the shell, which has already decided this is the thread on screen — the composer
 * cannot know that, and the rule that protects another thread's draft lives where the answer
 * does. The text **replaces** the draft: "set the editor's text" is not a merge, and a
 * half-merged box would be a draft the extension never wrote. The caret follows to the end so
 * what arrives is editable where it stops, and the box is not focused — the op asks for text,
 * not for the user's attention.
 */
function setDraft(text: string): void {
  draft.value = text;
  caret.value = text.length;
}

defineExpose({ setDraft, attachPaths, attachFiles, attachPastedText });
</script>

<template>
  <!--
    The drop target and the palette's host, with no chrome of its own: the *card* below is what a
    file is dropped on, so the highlight is a ring around that card rather than a second box
    around the box. (Until this pass it drew both, with tokens that no longer exist.)
  -->
  <div
    class="rounded-[12px] p-1 transition-shadow"
  >
    <AttachmentStrip
      :attachments="attachments"
      :refusal="refusal"
      @remove="remove"
    />

    <CommandPalette
      v-if="palette"
      :view="palette"
      :selected="paletteIndex"
      @pick="(index) => { paletteIndex = index; void accept(); }"
      @hover="(index) => { paletteIndex = index; }"
    />

    <FilePalette
      v-else-if="isFilePaletteOpen"
      :files="fileMatches"
      :selected="fileIndex"
      :query="atToken?.query ?? ''"
      @pick="(index) => { fileIndex = index; void acceptFile(index); }"
      @hover="(index) => { fileIndex = index; }"
    />

    <!--
      The card, in the reference's language: one raised rounded block with a leading `+`, the
      prompt itself in the window's sans at reading size, and the actions appearing inside it
      only once there is something to send.
    -->
    <div
      class="flex items-end gap-2 rounded-[12px] border border-line-strong/70 bg-raised/95 backdrop-blur-md px-3 py-2 transition-all focus-within:border-accent/80 focus-within:ring-1 focus-within:ring-accent/35 shadow-md shadow-black/20"
    >
      <!-- `+` accepts images and UTF-8 files; a drop carries an OS path instead. -->
      <button
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-[6px] text-faint transition-all hover:bg-surface hover:text-fg active:scale-95"
        aria-label="attach"
        @click="picker?.click()"
      >
        <Icon name="plus" class="h-4 w-4" />
      </button>
      <input
        ref="picker"
        type="file"
        multiple
        class="hidden"
        @change="onPicked"
      />

      <span v-if="readingFiles" class="flex h-7 shrink-0 items-center text-[11px] text-faint">Reading files…</span>

      <textarea
        ref="box"
        :value="draft"
        rows="1"
        spellcheck="false"
        placeholder="Type / for commands, @ for files"
        class="max-h-40 min-h-[28px] flex-1 resize-none overflow-y-auto bg-transparent py-1 text-[13px] leading-5 text-fg outline-none placeholder:text-faint [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        @input="onInput"
        @keydown="onKeydown"
        @keyup="caret = caretOf($event)"
        @paste="onPaste"
        @click="caret = caretOf($event)"
        @select="caret = caretOf($event)"
      />

      <button
        v-if="hasInput || steeringPending"
        :disabled="!canSend || steeringPending"
        class="flex h-7 min-w-7 shrink-0 items-center justify-center rounded-[6px] bg-accent px-2.5 text-[12px] font-semibold text-canvas transition-all hover:brightness-110 active:scale-95 disabled:opacity-40 disabled:hover:brightness-100 shadow-sm"
        @click="send(props.streaming ? 'steer' : 'prompt')"
      >
        <span v-if="steeringPending" class="inline-flex items-center gap-1.5">
          <Icon name="refresh" class="h-3 w-3 animate-spin text-canvas" />
          <span>steer…</span>
        </span>
        <span v-else class="leading-none select-none">{{ props.streaming ? "steer" : "↵" }}</span>
      </button>
      <button
        v-if="props.streaming || props.hasActiveAgents"
        :disabled="stopping"
        class="flex h-7 shrink-0 items-center justify-center rounded-[6px] border border-line px-2.5 text-[11.5px] text-dim transition-all hover:border-err/60 hover:text-err hover:bg-err/10 active:scale-95 disabled:opacity-50"
        @click="stop"
      >
        <span class="leading-none">{{ stopping ? "stopping…" : "stop" }}</span>
      </button>
    </div>

    <ComposerChips
      :thread="thread"
      v-bind="props.chips"
      @failed="emit('failed', $event)"
      @changed="emit('changed')"
      @workspace="emit('workspace', $event)"
      @terminal="emit('terminal', $event)"
      @favourites="emit('favourites', $event)"
    />
  </div>
</template>
