<script setup lang="ts">
/**
 * The composer's input row and its actions (`docs/12` §5.2, §3.4, §5.1).
 *
 * Owns the draft and the attachments, and nothing else: the session's liveness arrives
 * as props, every operation goes back to the host as one command, and a failure is
 * reported upward rather than swallowed. The chips of §5.1 slot in below the row.
 *
 * Attachments take three routes, and the rule that decides between them is the one
 * worth keeping straight: **bytes the browser holds go as `prompt{images}`, anything
 * with a path goes in as its path.** So a paste and an image chosen through `+` send
 * bytes, while a file dropped from the file manager sends its path for the agent to
 * read — which is also why a dropped file gets no thumbnail here (`lib/attachments.ts`).
 *
 * The keyboard map is `lib/composer.ts` — pure, and asserted — so this file only
 * arranges what it decides.
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  followUp,
  prompt as sendPrompt,
  steer,
  stopTurn,
  stopTurnAndSend,
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
import {
  frameRefusal,
  imagesToSend,
  messageFor,
  nameFromPath,
  roomForAnotherImage,
  type Attachment,
} from "../lib/attachments";
import { readImage } from "../lib/image-encode";
import { type ChipRow } from "../lib/chips";
import AttachmentStrip from "./AttachmentStrip.vue";
import Icon from "./ui/Icon.vue";
import CommandPalette from "./CommandPalette.vue";
import ComposerChips from "./ComposerChips.vue";

const props = defineProps<{
  /** The thread this composer speaks for: every command is addressed by its id. */
  thread: string;
  /** A turn is in flight, as `get_state` last reported. */
  streaming: boolean;
  /** Messages waiting behind the running turn. */
  queued: number;
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
}>();

const draft = ref("");
/** One send at a time: two Enters must not become two prompts. */
const sending = ref(false);
const attachments = ref<Attachment[]>([]);

/** `draft` is emptied by a send as much as by hand, so the height follows every path. */
watch(draft, () => void nextTick(grow));
/** The last refusal, shown until the next successful attach or send. */
const refusal = ref<string | null>(null);

/** The box itself, for the caret the palette is derived from. */
const box = ref<HTMLTextAreaElement | null>(null);
/** The composer card: the `+`, the row, the strip. Also the drop target. */
const card = ref<HTMLElement | null>(null);
const picker = ref<HTMLInputElement | null>(null);
/** A drag is over the composer, so it reads as a target. */
const hovering = ref(false);

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
});

const hasDraft = computed(() => draft.value.trim() !== "");
/** Attachment alone is enough: measured, the engine starts a turn for an image and no words. */
const hasInput = computed(() => hasDraft.value || attachments.value.length > 0);
const canSend = computed(() => hasInput.value && !props.disabled && !sending.value);
/** The message as it would go out, paths included — which is what the frame carries. */
const outbound = computed(() => messageFor(draft.value, attachments.value));

type Sendable = "prompt" | "steer" | "follow-up" | "stop-and-send";

/**
 * Take one image the browser handed over: a paste, or a file chosen through `+`.
 *
 * The room it has to fit is measured against the message as it stands, so a second
 * image is budgeted after the first rather than against the frame on its own.
 */
async function attach(blob: Blob, name: string): Promise<void> {
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

/**
 * Take the paths a drop delivered.
 *
 * Paths, not bytes: the OS told the window where the files are, which is exactly the
 * second route — the agent reads them, and the engine's own pipeline applies. The same
 * file dropped twice is one attachment.
 */
function attachPaths(paths: readonly string[]): void {
  if (props.disabled) {
    refusal.value = "no session is open, so there is nothing to attach to";
    return;
  }

  // One `seen` set for both halves of the question — what is already attached *and*
  // what this drop has already offered. Measured: filtering against the held list alone
  // attached `screen 1.png` twice when the same drop named it twice.
  const seen = new Set(
    attachments.value.flatMap((attachment) =>
      attachment.kind === "path" ? [attachment.path] : [],
    ),
  );
  const added = paths
    .filter((path) => (seen.has(path) ? false : seen.add(path) !== undefined))
    .map((path) => ({
      id: crypto.randomUUID(),
      kind: "path" as const,
      name: nameFromPath(path),
      path,
    }));

  if (added.length > 0) {
    attachments.value = [...attachments.value, ...added];
    refusal.value = null;
  }
}

async function onPicked(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  // Cleared before the reads, so choosing the same file twice still fires a change.
  input.value = "";

  for (const file of files) {
    await attach(file, file.name);
  }
}

async function onPaste(event: ClipboardEvent): Promise<void> {
  if (props.disabled) {
    return;
  }

  const images = Array.from(event.clipboardData?.items ?? [])
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .flatMap((item) => {
      const file = item.getAsFile();
      return file === null ? [] : [file];
    });

  if (images.length === 0) {
    // Ordinary text: the text area's own paste, untouched. Only a clipboard that
    // brought an image is intercepted, so nothing about typing changes.
    return;
  }

  event.preventDefault();
  for (const image of images) {
    await attach(image, "pasted image");
  }
}

/**
 * Whether a drag position is over the composer.
 *
 * The event reports *physical* pixels while a rect is in CSS pixels, so the scale
 * factor converts between them. A drag anywhere else is left alone rather than
 * attached: this is the one surface in the window that means "attach", and the same
 * event will serve the terminal and the file tree when they arrive.
 */
function isOver(position: { x: number; y: number }): boolean {
  const element = card.value;
  if (element === null) {
    return false;
  }

  const rect = element.getBoundingClientRect();
  const scale = window.devicePixelRatio || 1;
  const x = position.x / scale;
  const y = position.y / scale;

  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

let stopListening: UnlistenFn | null = null;

onMounted(async () => {
  stopListening = await getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;

    switch (payload.type) {
      case "enter":
      case "over":
        hovering.value = isOver(payload.position);
        break;
      case "drop":
        hovering.value = false;
        if (isOver(payload.position)) {
          attachPaths(payload.paths);
        }
        break;
      case "leave":
        hovering.value = false;
        break;
    }
  });
});

onUnmounted(() => {
  stopListening?.();
});

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
  // Cleared before the await, so the box is ready for the next thought and pressing
  // Enter twice cannot send the same message twice.
  draft.value = "";
  attachments.value = [];
  refusal.value = null;

  try {
    switch (operation) {
      case "prompt":
        await sendPrompt(props.thread, text, images);
        break;
      case "steer":
        await steer(props.thread, text, images);
        break;
      case "follow-up":
        await followUp(props.thread, text, images);
        break;
      case "stop-and-send":
        await stopTurnAndSend(props.thread, text, images);
        break;
    }
  } catch (cause) {
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
  }
}

/**
 * Put a palette row's text in the box and leave the caret after it.
 *
 * The caret is where the levels come from: `/security ` is the subcommand list and
 * `/security scan` is a message, so completing a row is the same edit either way.
 */
async function insert(text: string): Promise<void> {
  const next = replaceToken(draft.value, caret.value, text);
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
  element.style.height = `${element.scrollHeight}px`;
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

  const text = props.streaming ? null : dispatchOf(item);
  if (text === null) {
    await insert(insertionOf(item));
    return;
  }

  await dispatch(text);
}

async function stop(): Promise<void> {
  try {
    await stopTurn(props.thread);
  } catch (cause) {
    emit("failed", String(cause));
  }
}

async function onKeydown(event: KeyboardEvent): Promise<void> {
  // Read before the key applies: the palette is derived from the text up to the caret, and
  // this handler decides what the key means for exactly that text.
  caret.value = event.currentTarget instanceof HTMLTextAreaElement
    ? event.currentTarget.selectionStart ?? draft.value.length
    : draft.value.length;

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
      palette: palette.value !== null,
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
      paletteIndex.value = move("down", paletteIndex.value, palette.value?.items.length ?? 0);
      return;

    case "palette-up":
      event.preventDefault();
      paletteIndex.value = move("up", paletteIndex.value, palette.value?.items.length ?? 0);
      return;

    case "palette-accept":
      event.preventDefault();
      await accept();
      return;

    case "palette-complete": {
      event.preventDefault();
      const item = selected.value;
      // A window action has nothing to complete — it is not text the box will send.
      if (item !== null && item.kind !== "app") {
        await insert(insertionOf(item));
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

defineExpose({ setDraft });
</script>

<template>
  <!--
    The drop target and the palette's host, with no chrome of its own: the *card* below is what a
    file is dropped on, so the highlight is a ring around that card rather than a second box
    around the box. (Until this pass it drew both, with tokens that no longer exist.)
  -->
  <div
    ref="card"
    class="rounded-[12px] p-1 transition-shadow"
    :class="hovering ? 'ring-1 ring-accent/60' : ''"
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

    <!--
      The card, in the reference's language: one raised rounded block with a leading `+`, the
      prompt itself in the window's sans at reading size, and the actions appearing inside it
      only once there is something to send.
    -->
    <div
      class="flex items-start gap-2 rounded-[12px] border border-line-strong/80 bg-raised/90 px-3 py-2 transition-all focus-within:border-accent/80 focus-within:ring-1 focus-within:ring-accent/30 shadow-sm"
    >
      <!--
        `+` is the bytes route's discoverable door: an image chosen here travels with the
        message. Anything else already has a path, so it goes in as one — drop it on this
        card, or type the path — and the agent reads it.
      -->
      <button
        class="mt-0.5 grid h-6 w-6 shrink-0 place-items-center rounded-[6px] text-faint transition-colors hover:bg-surface hover:text-fg"
        title="attach an image — or drop any file here to send its path"
        aria-label="attach"
        @click="picker?.click()"
      >
        <Icon name="plus" class="h-4 w-4" />
      </button>
      <input
        ref="picker"
        type="file"
        accept="image/*"
        multiple
        class="hidden"
        @change="onPicked"
      />

      <textarea
        ref="box"
        :value="draft"
        rows="1"
        spellcheck="false"
        placeholder="Type / for commands"
        class="max-h-40 flex-1 self-center resize-none overflow-auto bg-transparent text-[13px] leading-relaxed text-fg outline-none placeholder:text-faint"
        @input="onInput"
        @keydown="onKeydown"
        @keyup="caret = caretOf($event)"
        @paste="onPaste"
        @click="caret = caretOf($event)"
        @select="caret = caretOf($event)"
      />

      <button
        v-if="hasInput"
        :disabled="!canSend"
        class="mt-0.5 grid h-6 min-w-6 place-items-center rounded-[6px] bg-accent px-2 py-0.5 text-[12px] font-semibold text-canvas transition-opacity hover:opacity-90 disabled:opacity-40"
        :title="streaming ? 'steer' : 'send (Enter)'"
        @click="send(streaming ? 'steer' : 'prompt')"
      >
        <span>{{ streaming ? "steer" : "↵" }}</span>
      </button>
      <button
        v-if="streaming"
        class="mt-0.5 shrink-0 rounded-[6px] border border-line px-2 py-0.5 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:text-fg"
        @click="stop"
      >
        stop
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

    <div
      v-if="streaming || queued > 0"
      class="mt-2 flex items-center gap-2 text-[11px] text-faint"
    >
      <!--
        The streaming actions are labelled, not modifier-only: `steer` is already the
        primary button, so the two less-common ones appear beside it only when there
        is something to send.
      -->
      <template v-if="streaming && hasInput">
        <button
          :disabled="!canSend"
          class="rounded-[6px] border border-line px-2 py-0.5 hover:border-line-strong hover:text-fg disabled:opacity-40"
          @click="send('follow-up')"
        >
          queue
        </button>
        <button
          :disabled="!canSend"
          class="rounded-[6px] border border-line px-2 py-0.5 hover:border-err/60 hover:text-err disabled:opacity-40"
          @click="send('stop-and-send')"
        >
          stop and send
        </button>
      </template>

      <span v-if="queued > 0" class="text-dim">{{ queued }} queued</span>
      <span v-if="streaming" class="text-ok">streaming</span>
      <span v-if="streaming" class="ml-auto text-faint">
        enter steers · ⌥enter queues · esc stops
      </span>
    </div>
  </div>
</template>
