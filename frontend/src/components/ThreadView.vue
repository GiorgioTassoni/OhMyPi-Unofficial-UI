<script setup lang="ts">
/**
 * One thread's column: its conversation, its dialogs, its composer.
 *
 * A component per thread rather than one view over the active thread, because a turn keeps
 * streaming in a thread the user has navigated away from and its rows have to keep landing
 * — the host publishes patches to whoever is listening, and this is what listens.
 *
 * The state here is entirely derived from what the session reported: the status is read,
 * the rows arrive as patches, the dialogs and the command list arrive as sets. Nothing is
 * remembered separately, so a chip cannot disagree with the session it describes.
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import {
  availableCommands,
  type SearchHit,
  onActivity,
  onCommandsUpdated,
  onRows,
  onUiRequests,
  threadStatus,
  transcript as readTranscript,
  uiRequests as readUiRequests,
  type ActivitySnapshot,
  type CommandEntry,
  type ModelOption,
  type RowPatch,
  type RowSnapshot,
  type SessionStatus,
  type UiRequestSnapshot,
} from "../bridge";
import { applyRowPatch } from "../lib/rows";
import type { ThreadChrome } from "../lib/chrome";
import { jumpTarget } from "../lib/search";
import { type ChipRow } from "../lib/chips";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Composer from "./Composer.vue";
import ConversationRow from "./ConversationRow.vue";
import DialogPanel from "./DialogPanel.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string;
  /** App-wide model state, which the chips render from. */
  models: ModelOption[];
  refreshing: boolean;
  favourites: string[];
  /** Directories this window has been pointed at, for the folder chip. */
  recent: string[];
  /** The OS user, for the empty thread's greeting (`docs/12` §4). */
  user: string | null;
  /**
   * This thread's extension chrome (`lib/chrome.ts`), held by the shell rather than read from
   * an event here: a thread that is not live has no view to receive one, and what an
   * extension pushed has to survive the view being mounted later.
   *
   * Never null — a thread nothing has pushed to gets the empty record, which is what makes the
   * template's two surfaces one condition each.
   */
  chrome: ThreadChrome;
}>();

const emit = defineEmits<{
  failed: [message: string];
  /**
   * A palette row for a surface the shell owns (`lib/palette.ts`'s `WINDOW_ACTIONS`).
   *
   * Forwarded with its id: the composer no longer reports "the settings screen was asked for",
   * it reports *which* row was taken, and the shell decides what that means — a mapping it needs
   * the moment the palette offers more than one.
   */
  "app-action": [id: string];
  /** The folder chip asked for another directory: a *new* thread, not this one. */
  workspace: [path: string];
  /** The folder chip asked for a terminal: the app's own shell, in this session's directory. */
  terminal: [path: string];
  favourites: [keys: string[]];
}>();

const status = ref<SessionStatus | null>(null);
const rows = ref<RowSnapshot[]>([]);
const dialogs = ref<UiRequestSnapshot[]>([]);
const activity = ref<ActivitySnapshot[]>([]);
const commands = ref<CommandEntry[]>([]);

/** The tail is capped: it exists to show liveness, not to keep history. */
const ACTIVITY_TAIL = 60;

/**
 * How often the status is re-read while events stream.
 *
 * A turn emits deltas far faster than any of this can change, and the previous version
 * read the status per event — correct, and a command round-trip per delta. The trailing
 * read is what keeps the chips from showing a stale value once the stream goes quiet.
 */
const STATUS_INTERVAL = 250;

let unlisten: UnlistenFn[] = [];
let lastStatusRead = 0;
let trailing: ReturnType<typeof setTimeout> | null = null;

onMounted(async () => {
  unlisten = await Promise.all([
    onRows((event) => {
      if (event.thread === props.thread) void applyPatch(event.payload);
    }),
    onUiRequests((event) => {
      if (event.thread === props.thread) dialogs.value = event.payload;
    }),
    onActivity((event) => {
      if (event.thread !== props.thread) return;
      activity.value = [...activity.value.slice(-(ACTIVITY_TAIL - 1)), event.payload];
      void refreshStatus();
    }),
    onCommandsUpdated((event) => {
      if (event.thread === props.thread) commands.value = event.payload;
    }),
  ]);

  // Read rather than wait for an event: extensions push dialogs and the engine pushes its
  // command list at startup, before this window can hear either.
  try {
    status.value = await threadStatus(props.thread);
    rows.value = await readTranscript(props.thread);
    dialogs.value = await readUiRequests(props.thread);
    commands.value = await availableCommands(props.thread).catch(() => []);
  } catch (cause) {
    emit("failed", describe(cause));
  }
});

onUnmounted(() => {
  for (const off of unlisten) off();
  if (trailing) clearTimeout(trailing);
});

/**
 * Fold a patch into the conversation.
 *
 * A patch that starts beyond what we hold means we missed one (a late subscription, a
 * dropped event), and the recovery is to re-read: the host only publishes what changed, so
 * the gap cannot be filled from here.
 */
async function applyPatch(patch: RowPatch): Promise<void> {
  const outcome = applyRowPatch(rows.value, patch);
  if (!outcome.stale) {
    const follow = isAtBottom();
    rows.value = outcome.rows;
    if (follow) followAfterRender();
    return;
  }

  const recovered = await readTranscript(props.thread).catch(() => rows.value);
  const follow = isAtBottom();
  rows.value = recovered;
  if (follow) followAfterRender();
}

/** A coalesced status read, so a streaming turn does not become a request per delta. */
async function refreshStatus(): Promise<void> {
  const now = Date.now();
  if (now - lastStatusRead < STATUS_INTERVAL) {
    if (!trailing) {
      trailing = setTimeout(() => {
        trailing = null;
        void refreshStatus();
      }, STATUS_INTERVAL);
    }
    return;
  }

  lastStatusRead = now;
  try {
    status.value = await threadStatus(props.thread);
  } catch (cause) {
    emit("failed", describe(cause));
  }
}

/**
 * A chip changed the session.
 *
 * The approval mode restarts the sidecar — the engine has no runtime setter — so what comes
 * back describes a different process and the conversation has to be re-read rather than
 * patched; the host hydrates the resumed thread, and this is where that lands.
 */
async function onChanged(): Promise<void> {
  try {
    status.value = await threadStatus(props.thread);
    rows.value = await readTranscript(props.thread);
    activity.value = [];
  } catch (cause) {
    emit("failed", describe(cause));
  }
}

/** Tauri rejects with a plain string, and a thrown `Error` would stringify badly. */
function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}

/**
 * What the chip row renders from (`docs/12` §5.1).
 *
 * Each chip reads a value the session already reported: the model and level from
 * `get_state`, the context ring from `contextUsage`, the mode from the launch the host
 * made.
 */
const chips = computed<ChipRow>(() => ({
  workspace: status.value?.workspace ?? "",
  recent: props.recent,
  mode: status.value?.approvalMode ?? null,
  model: status.value?.control.model ?? null,
  models: props.models,
  refreshing: props.refreshing,
  favourites: props.favourites,
  thinkingLevel: status.value?.control.thinkingLevel ?? null,
  context: status.value?.control.context ?? null,
  autoCompaction: status.value?.control.autoCompactionEnabled ?? null,
  compacting: status.value?.control.isCompacting ?? false,
}));

/** What the composer needs to know about the turn in flight. */
const streaming = computed(() => status.value?.control.isStreaming ?? false);
const queued = computed(() => status.value?.control.queuedMessageCount ?? 0);
/** The turn has started, but the engine has not produced a row the reader can follow yet. */
const waitingForOutput = computed(
  () => streaming.value && !rows.value.some((row) => row.streaming),
);

watch(waitingForOutput, (waiting) => {
  if (waiting && isAtBottom()) followAfterRender();
});

/**
 * Re-read everything: the status and the conversation.
 *
 * Needed after a flow that changes the session *in place* — a handoff commits a compaction
 * entry into the same file, nothing about which is visible in the message rows, while the
 * context ring the status carries moves immediately.
 */
async function reload(): Promise<void> {
  try {
    status.value = await threadStatus(props.thread);
    rows.value = await readTranscript(props.thread);
  } catch (cause) {
    emit("failed", describe(cause));
  }
}

/** The rendered row elements, so a search hit can be scrolled to. */
const rowElements = ref<(HTMLElement | null)[]>([]);
/** The row a jump landed on, flashed briefly so the eye can find it. */
const flashed = ref<number | null>(null);

/**
 * Scroll to the row a search hit is talking about (`docs/12` §7.4's jump target).
 *
 * The hit comes from the *index*, which read the session file; the rows here are the
 * transcript's own reduction of the message stream, so the two are matched by text
 * (`lib/search.ts`) rather than by position. A cold thread has to finish loading first, which
 * is what the bounded wait is for — not finding the row is not an error, it just means the
 * thread opened without it.
 */
async function jumpTo(hit: SearchHit): Promise<void> {
  if (rows.value.length === 0) {
    const deadline = Date.now() + 2000;
    while (rows.value.length === 0 && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  await revealRow(jumpTarget(rows.value, hit));
}

/**
 * The greeting an empty thread shows (`docs/12` §4).
 *
 * Decorative and app-owned: a time of day and who is sitting here. It is keyed on the *engine's*
 * message count rather than on the rows being empty, because an empty row list is also what a
 * resume looks like for its first moment — and greeting someone who is waiting for their own
 * conversation to load is the one way this could be wrong rather than merely quaint.
 */
const greeting = computed(() => {
  const hour = new Date().getHours();
  const part =
    hour < 5 ? "Late night" : hour < 12 ? "Morning" : hour < 18 ? "Afternoon" : "Evening";
  return props.user === null || props.user === "" ? part : `${part}, ${props.user}`;
});

const nothingSaidYet = computed(
  () => rows.value.length === 0 && status.value?.control.messageCount === 0 && !streaming.value,
);

/**
 * Put a row in front of the reader and mark it for a moment.
 *
 * The right panel already knows the index it wants — it derived it from these same rows
 * (`lib/panel.ts`) — so this is the one place that scrolls and flashes, and the two callers
 * differ only in how they found the row.
 */
async function revealRow(at: number | null): Promise<void> {
  if (at === null || at < 0 || at >= rows.value.length) return;

  if (rows.value.length === 0) {
    const deadline = Date.now() + 2000;
    while (rows.value.length === 0 && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  await nextTick();
  rowElements.value[at]?.scrollIntoView({ block: "center" });
  flashed.value = at;
  setTimeout(() => {
    if (flashed.value === at) flashed.value = null;
  }, 1500);
}

/** The composer, for the one op that has to reach into it. */
const composer = ref<{ setDraft: (text: string) => void } | null>(null);

/**
 * Put the engine's own text in this thread's composer (`editor-text`, `lib/chrome.ts`).
 *
 * The shell routes the op to the column it names, and the box itself belongs to `Composer` —
 * so this forwards rather than keeping a second copy of the draft, which is what would let the
 * composer and the shell disagree about what the user is typing.
 */
function setDraft(text: string): void {
  composer.value?.setDraft(text);
}

const conversationRef = ref<HTMLElement | null>(null);
const isScrolledUp = ref(false);

/** A little tolerance absorbs fractional layout pixels without treating a reader as scrolled. */
const BOTTOM_TOLERANCE = 2;

function isAtBottom(): boolean {
  const el = conversationRef.value;
  if (!el) return true;
  return el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_TOLERANCE;
}

function onScroll(): void {
  isScrolledUp.value = !isAtBottom();
}

/** Keep a reader who was at the tail pinned there as streamed rows change height. */
function followAfterRender(): void {
  void nextTick(() => {
    if (isScrolledUp.value) return;
    const el = conversationRef.value;
    if (el) el.scrollTop = el.scrollHeight;
  });
}

function scrollToBottom(): void {
  const el = conversationRef.value;
  if (!el) return;
  el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
}

// Exposed for the shell's diagnostics drawer, for re-reading after a flow, for a search hit's
// jump, and for the one chrome op that writes the box. `defineExpose` unwraps refs, so the
// parent sees values (`views[id].status`, not `views[id].status.value`).
defineExpose({
  status,
  activity,
  dialogs,
  rows,
  refreshStatus,
  reload,
  jumpTo,
  revealRow,
  setDraft,
});
</script>

<template>
  <section class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
    <!--
      The row kinds of `docs/12` §3.1, rendered from the patch stream. Each kind lives in
      `ConversationRow`: a user turn with its attachments, an assistant turn as sanitized
      markdown with its reasoning collapsed, a tool card (`ToolCard` + `DiffView`), and
      maintenance chips.

      One centred column, the width a line of prose wants, which is what the reference does with
      both the transcript and the composer — the two read as one page rather than as two panes.
    -->
    <div
      ref="conversationRef"
      class="flex min-h-0 flex-1 flex-col overflow-auto px-4"
      data-conversation
      @scroll="onScroll"
    >
      <!--
        `docs/12` §4: a time of day and a sub-line, in the thread's accent, at the top of an
        empty conversation. It is decorative, it is ours, and it is gone the moment there is a
        message to read. The reference puts it here rather than above the composer, and that is
        the reading that survived contact with a real window: the composer is where the eye
        already is.
      -->
      <div
        v-if="nothingSaidYet"
        class="my-auto flex flex-col items-center justify-center py-16 text-center select-none"
        data-greeting
      >
        <p class="text-[30px] font-normal leading-tight text-fg">{{ greeting }}</p>
        <p class="mt-2 text-[13px] text-faint">A fresh thread — nothing said yet.</p>
      </div>

      <div v-else class="mx-auto flex w-full max-w-[46rem] flex-col gap-3 py-4">
        <!--
          Each row is wrapped so a search hit has an element to scroll to: a component ref hands
          back its exposed proxy, not the node, and `scrollIntoView` needs the node.
        -->
        <div
          v-for="(row, index) in rows"
          :key="index"
          :ref="(element) => (rowElements[index] = (element as HTMLElement | null))"
          class="shrink-0 rounded-[8px] transition-colors duration-500"
          :class="flashed === index ? 'bg-accent/10 ring-1 ring-accent' : ''"
          :data-flash="flashed === index ? 'on' : null"
        >
          <ConversationRow :row="row" @failed="emit('failed', $event)" />
        </div>

        <p
          v-if="waitingForOutput"
          class="shrink-0 animate-pulse text-[12.5px] text-faint"
          role="status"
          aria-live="polite"
        >
          Loading…
        </p>
      </div>
    </div>

    <!-- Floating scroll-to-bottom button when reading history -->
    <div
      v-if="isScrolledUp && rows.length > 0"
      class="pointer-events-auto absolute bottom-24 left-1/2 -translate-x-1/2 z-20 transition-all"
    >
      <button
        type="button"
        class="grid h-8 w-8 place-items-center rounded-full border border-line-strong/70 bg-surface text-dim shadow-xl shadow-black/50 transition-all hover:bg-raised hover:text-fg hover:scale-105 active:scale-95"
        title="Scroll to bottom"
        @click="scrollToBottom"
      >
        <Icon name="chevron-down" class="h-4 w-4" />
      </button>
    </div>

    <div class="mx-auto w-full max-w-[46rem] px-4 pb-4">
      <!--
        What this session's extensions pushed (`session-chrome`). Both surfaces sit by the
        composer because that is where the engine's own status and widget rows are — a thin
        line under the box the user is typing in. A cleared surface is `null` and draws
        nothing; `data-status` and `data-widget` are what the window checks find them by.
      -->
      <pre
        v-if="props.chrome.widget !== null"
        class="mb-1 overflow-x-auto whitespace-pre-wrap break-all rounded-[6px] bg-raised px-2 py-1 font-mono text-[11px] leading-snug text-dim"
        data-widget
      >{{ props.chrome.widget.join("\n") }}</pre>
      <p
        v-if="props.chrome.status !== null"
        class="mb-1 truncate text-[11px] text-faint"
        data-status
      >{{ props.chrome.status }}</p>

      <!-- The dialogs the agent is waiting on (`docs/12` §10). -->
      <DialogPanel
        :thread="thread"
        :dialogs="dialogs"
        @failed="emit('failed', $event)"
      />

      <Composer
        ref="composer"
        :thread="thread"
        :streaming="streaming"
        :queued="queued"
        :disabled="status === null"
        :frame-limit="status?.ready.maxFrameBytes ?? 0"
        :commands="commands"
        :chips="chips"
        @failed="emit('failed', $event)"
        @changed="onChanged"
        @workspace="emit('workspace', $event)"
        @terminal="emit('terminal', $event)"
        @favourites="emit('favourites', $event)"
        @app-action="emit('app-action', $event)"
      />
    </div>
  </section>
</template>
