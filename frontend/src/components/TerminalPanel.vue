<script setup lang="ts">
/**
 * The terminal drawer (`docs/12` §11, decision D6).
 *
 * A real shell in a real pty, drawn by xterm.js — the one surface in this app the engine has
 * no part in. `--mode rpc-ui` sets `PI_NO_PTY=1`, so the agent's `bash` runs without a terminal
 * and can neither see nor drive this; the panel says so in as many words, because "the agent's
 * shell" is the assumption a user would otherwise make.
 *
 * # What this component owns, and what it does not
 *
 * The *tabs* belong to the window (they are the host's own rows, `terminal-open`/`close`), and
 * the *emulators* belong here: one xterm instance per tab, kept out of reactivity because an
 * emulator is a thing being drawn, not an input to a render. Output arrives on one channel for
 * every tab and is routed by id — into its emulator when one is mounted, into a bounded buffer
 * when one is not.
 *
 * That buffer is not a precaution. A shell prints its prompt the moment it is allocated, which
 * is before the window that asked for it learns the tab's id, so the first batch of a new tab's
 * output always arrives first. The same is true of every terminal whose drawer is closed: an
 * emulator is mounted when the drawer is *shown*, and until then the tail is what is kept —
 * which is what a terminal shows anyway.
 *
 * # Not the agent's shell
 *
 * Nothing here is wired to the conversation, deliberately (D6: no automatic command injection
 * in v1). A card's `bash` output is the transcript's, and this is the user's. The one shared
 * thing is the working directory a tab starts in.
 */
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { onTerminalOutput, terminalResize, terminalWrite } from "../bridge";
import type { AppTheme } from "../lib/appTheme";
import { decodeOutput, Pending, type TerminalTab } from "../lib/terminals";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  tabs: TerminalTab[];
  /** The tab on screen. The host's set and this selection are the window's state. */
  active: string | null;
  /** Whether the drawer is on screen at all; a hidden emulator is not worth mounting. */
  visible: boolean;
  /** The app palette, so already-mounted xterm instances change with the window. */
  theme: AppTheme;
}>();

const emit = defineEmits<{
  select: [id: string];
  close: [id: string];
  /** A new tab, in the directory the active one runs in. */
  open: [];
  failed: [message: string];
}>();

/** The element the emulators are hosted in. */
const body = ref<HTMLElement | null>(null);

/** One emulator per tab, outside reactivity: an xterm instance is not a render input. */
const emulators = new Map<string, { term: Terminal; fit: FitAddon }>();

/** Output that arrived before its tab had an emulator, per tab. */
const pending = new Pending();

/** The size a tab was last told, so a resize is sent when it changes rather than every frame. */
const told = new Map<string, string>();

/**
 * The panel's colours, from the app's own tokens.
 *
 * Read rather than duplicated: the window defines its palette once (`style.css`), and a
 * terminal is one more surface that palette has to cover. `--font-mono` is a stack, which is
 * what xterm wants, so it is passed through rather than resolved here.
 */
function token(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value === "" ? fallback : value;
}

function terminalTheme(): { background: string; foreground: string; cursor: string; selectionBackground: string } {
  return {
    background: token("--color-canvas", "#1f1e33"),
    foreground: token("--color-fg", "#eceaf6"),
    cursor: token("--color-accent", "#a397e9"),
    selectionBackground: token("--color-selected", "#28283e"),
  };
}

/**
 * Mount an emulator for a tab, and hand it whatever output was waiting for it.
 *
 * Called only while the drawer is on screen: xterm measures its cell size when it is opened,
 * and an element inside `display: none` measures zero — which is a terminal the user would then
 * have to resize by hand to make legible.
 */
function mount(tab: TerminalTab, element: HTMLElement): void {
  const term = new Terminal({
    fontFamily: token("--font-mono", "monospace"),
    fontSize: 12,
    lineHeight: 1.2,
    scrollback: 5000,
    cursorBlink: true,
    // The shell wraps its own lines. An emulator that also wraps double-wraps every long line.
    convertEol: false,
    theme: terminalTheme(),
  });

  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);

  term.onData((data) => {
    void terminalWrite(tab.id, data).catch((cause: unknown) => emit("failed", describe(cause)));
  });

  // `Ctrl/⌘+Shift+C` copies the selection: a terminal cannot give `Ctrl+C` up (that is the
  // interrupt), and without this there is no way to take anything out of one.
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    const modified = event.ctrlKey || event.metaKey;
    const copy = event.shiftKey && event.key.toLowerCase() === "c";

    if (modified && copy) {
      const selection = term.getSelection();
      if (selection !== "") void navigator.clipboard.writeText(selection);
      return false;
    }
    return true;
  });

  term.options.disableStdin = !tab.running;
  emulators.set(tab.id, { term, fit });

  const waiting = pending.take(tab.id);
  if (waiting !== null) term.write(waiting);

  fitTab(tab.id);
}

/** Size an emulator to its container, and tell its shell only if that changed anything. */
function fitTab(id: string): void {
  const found = emulators.get(id);
  const host = body.value?.querySelector<HTMLElement>(`[data-terminal="${id}"]`);
  // A hidden or not-yet-laid-out host has no size to fit into, and the fit addon answers
  // nonsense for one. The observer calls back when there is a real box.
  if (!found || !host || host.clientWidth === 0 || host.clientHeight === 0) return;

  const before = `${found.term.cols}x${found.term.rows}`;
  try {
    found.fit.fit();
  } catch {
    return;
  }

  const now = `${found.term.cols}x${found.term.rows}`;
  if (now === before && told.get(id) === now) return;
  told.set(id, now);

  void terminalResize(id, found.term.cols, found.term.rows).catch((cause: unknown) =>
    emit("failed", describe(cause)),
  );
}

/** Size the tab on screen. The others have no box to measure until they are shown. */
function fitActive(): void {
  if (props.active !== null) fitTab(props.active);
}

/**
 * Bring the emulators in line with the tabs: mount what is new, forget what is gone.
 *
 * Gated on the drawer being visible, so a terminal opened while it was closed mounts when the
 * user looks at it — with the tail of its output waiting.
 */
function sync(): void {
  if (body.value === null) return;

  for (const tab of props.tabs) {
    const existing = emulators.get(tab.id);
    if (existing !== undefined) {
      // A shell that has exited takes no more input: typing into one should do nothing, which
      // is what a terminal does, rather than round-trip a refusal from the host.
      existing.term.options.disableStdin = !tab.running;
      continue;
    }
    if (!props.visible) continue;

    const host = body.value.querySelector<HTMLElement>(`[data-terminal="${tab.id}"]`);
    if (host !== null) mount(tab, host);
  }

  // Deleting the entry a `Map` iterator is standing on is safe, so the cleanup walks the live
  // map rather than a copy of it.
  for (const [id, entry] of emulators) {
    if (props.tabs.some((tab) => tab.id === id)) continue;
    entry.term.dispose();
    emulators.delete(id);
    told.delete(id);
    pending.forget(id);
  }

  fitActive();
}

/** Focus the tab on screen, so a click into the panel is enough to start typing. */
function focusActive(): void {
  if (props.active === null) return;
  emulators.get(props.active)?.term.focus();
}

let observer: ResizeObserver | null = null;
let off: (() => void) | null = null;

onMounted(async () => {
  off = await onTerminalOutput((output) => {
    const bytes = decodeOutput(output.data);
    if (bytes === null) return;

    const found = emulators.get(output.id);
    if (found !== undefined) found.term.write(bytes);
    else pending.push(output.id, bytes);
  });

  observer = new ResizeObserver(() => fitActive());
  if (body.value !== null) observer.observe(body.value);
});

onUnmounted(() => {
  off?.();
  observer?.disconnect();
  for (const entry of emulators.values()) entry.term.dispose();
  emulators.clear();
});

// The container exists only once, so the observers are bound to the element rather than
// re-created: what changes is the tabs, and a new one needs a box before it can be mounted.
watch(() => props.tabs, () => void nextTick(sync), { flush: "post" });
watch(() => props.visible, () => void nextTick(sync), { flush: "post" });
watch(() => props.theme, () => {
  for (const { term } of emulators.values()) term.options.theme = terminalTheme();
}, { flush: "post" });
watch(
  () => props.active,
  () =>
    void nextTick(() => {
      fitActive();
      focusActive();
    }),
  { flush: "post" },
);
onMounted(() => void nextTick(sync));

/** Tauri rejects with a plain string, and a thrown `Error` would stringify badly. */
function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}
</script>

<template>
  <section
    class="flex h-80 shrink-0 flex-col border-t border-line bg-canvas"
    data-terminal-panel
  >
    <!--
      The tab strip. A tab keeps its shell's state, not its transcript: a run that ended stays
      listed with the way it ended, so the last screenful is still readable — which is what a
      terminal tab is for.
    -->
    <div class="flex items-center gap-1 border-b border-line px-2 py-1.5">
      <button
        v-for="tab in props.tabs"
        :key="tab.id"
        type="button"
        class="flex items-center gap-1.5 rounded-[6px] px-2.5 py-1 text-[12.5px]"
        :class="
          tab.id === props.active
            ? 'bg-raised text-fg'
            : 'text-dim hover:text-fg'
        "
        :title="tab.cwd"
        @click="emit('select', tab.id)"
      >
        <span
          class="size-1.5 rounded-full"
          :class="tab.running ? 'bg-ok' : 'bg-faint'"
        ></span>
        {{ tab.label }}
        <span v-if="!tab.running" class="text-[10.5px] text-faint">{{ tab.ended }}</span>
        <span
          class="ml-0.5 grid h-4 w-4 place-items-center rounded-[4px] text-faint hover:text-err"
          title="close this terminal"
          @click.stop="emit('close', tab.id)"
        >
          <Icon name="close" class="h-3 w-3" />
        </span>
      </button>

      <button
        type="button"
        class="grid h-6 w-6 place-items-center rounded-[6px] text-faint hover:bg-raised hover:text-fg"
        title="another terminal in this directory"
        @click="emit('open')"
      >
        <Icon name="plus" class="h-3.5 w-3.5" />
      </button>

      <span class="ml-auto truncate font-mono text-[10.5px] text-faint">{{ props.active === null ? "" : (props.tabs.find((tab) => tab.id === props.active)?.cwd ?? "") }}</span>
    </div>

    <div ref="body" class="relative min-h-0 flex-1">
      <!--
        One host per tab, mounted whether or not it is on screen: a terminal in the background
        keeps drawing, which is the whole point of a drawer that survives switching threads.
      -->
      <div
        v-for="tab in props.tabs"
        v-show="tab.id === props.active"
        :key="tab.id"
        class="absolute inset-0 p-2"
      >
        <div :data-terminal="tab.id" class="h-full w-full"></div>
      </div>

      <div
        v-if="props.tabs.length === 0"
        class="flex h-full flex-col items-center justify-center gap-1 px-6 text-center"
      >
        <p class="text-[13px] text-dim">No terminals in this window</p>
        <p class="max-w-sm text-[12.5px] text-faint">
          <span class="text-dim">+</span> opens a shell in this workspace. This is
          <span class="text-dim">your</span> shell, not the agent's: the agent's
          <span class="text-dim">bash</span> runs without a terminal and cannot see or
          drive this one.
        </p>
      </div>
    </div>
  </section>
</template>
