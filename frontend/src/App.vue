<script setup lang="ts">
/**
 * The window: a sidebar of threads, one thread's column, and the diagnostics drawer.
 *
 * The app holds **several sessions at once** (decision D5), one `omp --mode rpc-ui` sidecar
 * each, so this shell owns only what is genuinely app-wide: the session catalogue, the live
 * thread set, which thread is on screen, the model catalogue, the favourites, and the
 * unread marks. Everything session-scoped lives in the thread's own view, which is why a
 * turn keeps landing correctly in a thread the user has navigated away from.
 *
 * A thread's id is the engine's session id — the same id the catalogue lists on disk — so
 * "resume this session" and "this live thread" are one key rather than two identity systems
 * that would have to be kept in step.
 */
import { computed, nextTick, onMounted, onUnmounted, ref, shallowReactive, watch } from "vue";
import {
  closeThread,
  deleteSession,
  favourites as readFavourites,
  launchContext,
  models as readModels,
  onChrome,
  onModelsUpdated,
  onNotifications,
  agents as readAgents,
  type ThreadAgents,
  onAgents,
  onThreadsUpdated,
  notifyOs,
  openExternal,
  openThread,
  projects as readProjects,
  refreshModels,
  renameThread as sendRename,
  sessions as readSessions,
  setFavourites,
  setFocusedThread,
  terminalClose,
  terminalOpen,
  terminalWrite,
  terminals as readTerminals,
  onTerminals,
  threads as readThreads,
  type ActivitySnapshot,
  type ChromeEvent,
  type ModelOption,
  type NotificationEvent,
  type ProjectSnapshot,
  type RowSnapshot,
  type SearchHit,
  type SessionStatus,
  type SessionSummary,
  type ThreadSnapshot,
  type UiRequestSnapshot,
} from "./bridge";
import { activeCount } from "./lib/agents";
import { applyChrome, chromeFor, draftTarget, type ThreadChrome } from "./lib/chrome";
import {
  addNotice,
  dropNotice,
  marksUnread,
  noticeForAction,
  noticeFor,
  noticeFromChrome,
  shouldNotifyOs,
  showsNotice,
  type Notice,
} from "./lib/notify";
import { selectAfterClose, selectFrom, tabsFrom, type TerminalTab } from "./lib/terminals";
import { sidebarModel, type ProjectGroup } from "./lib/threads";
import type { ThreadContext } from "./lib/thread-menu";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import DiagnosticsPanel from "./components/DiagnosticsPanel.vue";
import Modal from "./components/Modal.vue";
import RightPanel from "./components/RightPanel.vue";
import AgentsPanel from "./components/AgentsPanel.vue";
import SearchOverlay from "./components/SearchOverlay.vue";
import SettingsScreen from "./components/SettingsScreen.vue";
import Sidebar from "./components/Sidebar.vue";
import ThreadActions from "./components/ThreadActions.vue";
import TerminalPanel from "./components/TerminalPanel.vue";
import ThreadView from "./components/ThreadView.vue";
import TitleBar from "./components/TitleBar.vue";

/**
 * What a `ThreadView` exposes to the shell.
 *
 * The diagnostics drawer reads the *active* thread's own view rather than keeping a second
 * copy of the same state: one read per thread, in the place that needs it.
 */
interface ThreadViewApi {
  /** Scroll to the row a search hit is about (`docs/12` §7.4). */
  jumpTo: (hit: SearchHit) => Promise<void>;
  /** **Values, not refs.** `defineExpose` unwraps: an exposed ref arrives as its value, so
   * `view.status` is the status and `view.status.value` is `undefined`. Checking them still
   * tracks them — the unwrapping read happens inside this shell's computeds — but writing
   * `.value` here is the same mistake in reverse. */
  status: SessionStatus | null;
  activity: ActivitySnapshot[];
  dialogs: UiRequestSnapshot[];
  /** The transcript — what the conversation renders, and what the right panel derives its
   * file and artifact lists from. */
  rows: RowSnapshot[];
  /** Re-read the whole thread: for flows that change a session in place. */
  reload: () => Promise<void>;
  /** Re-read only `get_state`: what the chips and the plan come from. */
  refreshStatus: () => Promise<void>;
  /** Put the engine's own text in the composer, replacing whatever was there (`editor-text`). */
  setDraft: (text: string) => void;
  /** Put one transcript row in front of the reader, flashing it. */
  revealRow: (at: number | null) => Promise<void>;
}

/**
 * A thread to keep on screen: the active one, plus every thread that is live.
 *
 * `shallowReactive`, not `reactive`, and the difference is not cosmetic: a deep proxy
 * **unwraps refs it finds**, so `views[id].status` would hand back the *value* of the view's
 * `status` ref and `views[id].status.value` would be `undefined` — which is exactly how the
 * diagnostics drawer came to read "not connected" for a live thread. Shallow keeps the map's
 * keys reactive (a view appearing must be noticed) and hands out the instance untouched, so
 * the refs inside it stay refs and their own reactivity does the rest.
 */
const views = shallowReactive<Record<string, ThreadViewApi | undefined>>({});

const catalogue = ref<SessionSummary[]>([]);
const live = ref<ThreadSnapshot[]>([]);

/**
 * Every live thread's agent roster, as the host publishes it (`docs/12` §9).
 *
 * Pushed rather than polled, because the engine deletes a settled subagent from its own
 * registry the moment it settles: the frames the host folds in are the only place a finished
 * agent is still a row, and a roster rebuilt by asking again would lose exactly the agent a
 * reader is looking for.
 */
const rosters = ref<ThreadAgents[]>([]);
const projects = ref<ProjectSnapshot[]>([]);
const activeId = ref<string | null>(null);
const unread = ref<Set<string>>(new Set());

/**
 * Which thread is on screen, told to the host (`docs/14` step 14).
 *
 * The idle policy releases a sidecar after ten quiet minutes, and the one thing it cannot work
 * out for itself is which thread a person is *reading* — so the window says it, on every
 * change including the first (which is `null` before anything is open). Nothing else in the app
 * reads this: it is not a preference and not a setting, and the host decides nothing about
 * whether to interrupt anyone from it.
 */
watch(activeId, (thread) => {
  void setFocusedThread(thread);
}, { immediate: true });

/**
 * The host's notifications, and the chrome an extension pushed (`docs/12` §13).
 *
 * A stack of notices rather than one line, because these arrive from sessions nobody is
 * watching and two facts landing together are two things to say: the second must not erase the
 * first before it has been read. `lib/notify.ts` owns which facts become one.
 */
const notices = ref<Notice[]>([]);
/** How many notices stay on screen; the oldest goes first, since it is the least new. */
const NOTICE_LIMIT = 3;
const ACTION_NOTICE_DURATION = 6000;
const actionNoticeTimers = new Map<string, ReturnType<typeof setTimeout>>();

/**
 * The status line and widget each thread's extensions have pushed (`lib/chrome.ts`).
 *
 * Held by the shell rather than by a thread's own view: a column is mounted only while its
 * thread is live or on screen, and a widget pushed at the engine's startup has to survive the
 * second between the push and the view that draws it.
 */
const chrome = ref<Record<string, ThreadChrome>>({});

/**
 * Titles the engine set for a thread itself (`setTitle`).
 *
 * Folded over the catalogue rather than written anywhere: the title is the engine's, the
 * window only remembers it, and the row and the panel both read the catalogue — so one overlay
 * moves both. A rename through the menu clears it, because that is the user speaking last.
 */
const engineTitles = ref<Record<string, string>>({});

/** Notice ids: monotonic, so two identical facts are still two notices. */
let noticeSeq = 0;

/** App-wide, because the engine's model catalogue is a property of the engine. */
const models = ref<ModelOption[]>([]);
const refreshing = ref(false);
const favourites = ref<string[]>([]);

/** Directories this window has been pointed at, for the folder chip. */
const recent = ref<string[]>([]);

/**
 * Who this window is running as, and which build it is (`docs/12` §2.2's footer row).
 *
 * Read from the launch context rather than guessed in the frontend: the OS user is the host's
 * to know, and a version the window made up would be a decoration.
 */
const identity = ref<{ user: string | null; version: string }>({ user: null, version: "" });
const busy = ref(false);
const error = ref<string | null>(null);
const diagnostics = ref(false);
/**
 * Whether the settings screen owns the window (`docs/12` §12).
 *
 * A view rather than an overlay: settings is a place in the window, and the thread columns are
 * what it replaces. The titlebar stays, because the window is still the window — new thread,
 * terminal and the panel toggles all mean the same thing from here.
 */
const settings = ref(false);
/** `docs/12` §1: the panel owns the right column, and the diagnostics borrow it when toggled. */
const panelOpen = ref(true);

/** Resizable sidebars & collapse thresholds */
const DEFAULT_SIDEBAR_WIDTH = 288;
const MIN_SIDEBAR_WIDTH = 180;
const MAX_SIDEBAR_WIDTH = 520;
const SIDEBAR_COLLAPSE_THRESHOLD = 140;

const DEFAULT_RIGHT_PANEL_WIDTH = 320;
const MIN_RIGHT_PANEL_WIDTH = 200;
const MAX_RIGHT_PANEL_WIDTH = 600;
const RIGHT_PANEL_COLLAPSE_THRESHOLD = 160;

/** A forgiving drag target around the quiet one-pixel divider the user sees. */
const RESIZE_HANDLE_CLASS =
  "z-30 h-full w-2 cursor-col-resize select-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-warn after:opacity-0 after:transition-opacity hover:after:opacity-80";

function readStorageNumber(key: string, fallback: number): number {
  try {
    const val = localStorage.getItem(key);
    if (!val) return fallback;
    const num = Number(val);
    return Number.isFinite(num) && num > 0 ? num : fallback;
  } catch {
    return fallback;
  }
}

const sidebarWidth = ref(readStorageNumber("omp:sidebar-width", DEFAULT_SIDEBAR_WIDTH));
const sidebarCollapsed = ref(localStorage.getItem("omp:sidebar-collapsed") === "true");
const rightPanelWidth = ref(readStorageNumber("omp:right-panel-width", DEFAULT_RIGHT_PANEL_WIDTH));

const isDraggingLeft = ref(false);
const isDraggingRight = ref(false);

watch(sidebarWidth, (next) => {
  try {
    localStorage.setItem("omp:sidebar-width", String(next));
  } catch {}
});

watch(sidebarCollapsed, (next) => {
  try {
    localStorage.setItem("omp:sidebar-collapsed", String(next));
  } catch {}
});

watch(rightPanelWidth, (next) => {
  try {
    localStorage.setItem("omp:right-panel-width", String(next));
  } catch {}
});

function startLeftResize(event: PointerEvent): void {
  event.preventDefault();
  isDraggingLeft.value = true;
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";

  const onPointerMove = (e: PointerEvent) => {
    if (!isDraggingLeft.value) return;
    const currentX = e.clientX;
    if (currentX < SIDEBAR_COLLAPSE_THRESHOLD) {
      sidebarCollapsed.value = true;
    } else {
      sidebarCollapsed.value = false;
      const maxAllowed = Math.min(MAX_SIDEBAR_WIDTH, Math.floor(window.innerWidth * 0.45));
      sidebarWidth.value = Math.max(MIN_SIDEBAR_WIDTH, Math.min(currentX, maxAllowed));
    }
  };

  const onPointerUp = () => {
    isDraggingLeft.value = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerUp);
  };

  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  window.addEventListener("pointercancel", onPointerUp);
}

function resetLeftWidth(): void {
  sidebarWidth.value = DEFAULT_SIDEBAR_WIDTH;
  sidebarCollapsed.value = false;
}

function startRightResize(event: PointerEvent): void {
  event.preventDefault();
  isDraggingRight.value = true;
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";

  const onPointerMove = (e: PointerEvent) => {
    if (!isDraggingRight.value) return;
    const currentWidth = window.innerWidth - e.clientX;
    if (currentWidth < RIGHT_PANEL_COLLAPSE_THRESHOLD) {
      panelOpen.value = false;
    } else {
      panelOpen.value = true;
      const maxAllowed = Math.min(MAX_RIGHT_PANEL_WIDTH, Math.floor(window.innerWidth * 0.45));
      rightPanelWidth.value = Math.max(MIN_RIGHT_PANEL_WIDTH, Math.min(currentWidth, maxAllowed));
    }
  };

  const onPointerUp = () => {
    isDraggingRight.value = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerUp);
  };

  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  window.addEventListener("pointercancel", onPointerUp);
}

function resetRightWidth(): void {
  rightPanelWidth.value = DEFAULT_RIGHT_PANEL_WIDTH;
  panelOpen.value = true;
}

/** How many subagents are in flight, for the sidebar's row (`docs/12` §2.1). */
const activeAgents = computed(() =>
  activeCount(
    // Every roster the window holds, filtered to the threads that still have an engine: a
    // closed thread's roster is what it *had*, and nothing runs in a session whose engine has
    // gone. The panel applies the same rule, so the sidebar's number and the panel's cannot
    // disagree — which they did until a browser check compared them.
    rosters.value,
    live.value.map((thread) => thread.id),
  ),
);

/** Thread titles, for the panel's headings. */
const threadTitles = computed(() =>
  Object.fromEntries(rows.value.map((row) => [row.id, row.title])),
);

/**
 * Open the agents panel, reading the rosters the host has not pushed.
 *
 * The pushed ones cover every live thread — that is what the frames are — so this is for the
 * threads whose engine has *gone*: a subagent that was still running when a sidecar closed is
 * only in the registry's remembered roster, and no event will ever announce it.
 */
async function refreshAgents(): Promise<void> {
  try {
    const read = await readAgents();
    for (const entry of read) {
      if (!rosters.value.some((known) => known.thread === entry.thread && known.agents.length > 0)) {
        rosters.value = [...rosters.value.filter((known) => known.thread !== entry.thread), entry];
      }
    }
  } catch (cause) {
    error.value = describe(cause);
  }
}

async function openAgents(): Promise<void> {
  agentsOpen.value = true;
  await refreshAgents();
}

/** The active thread's row in the sidebar model, for the title and the workspace. */
const activeRow = computed(() => {
  const id = activeId.value;
  if (id === null) return null;
  for (const group of groups.value) {
    const row = group.threads.find((entry) => entry.id === id);
    if (row) return row;
  }
  return null;
});

/** The active thread's transcript, owned by its view (`docs/12` §1's one-read rule). */
const activeRows = computed<RowSnapshot[]>(() => {
  const id = activeId.value;
  return id === null ? [] : (views[id]?.rows ?? []);
});


/** Open or close the settings screen, whatever put the user there. */
function openSettings(open: boolean): void {
  settings.value = open;
}

/**
 * The approval mode changed somewhere in the settings screen, which restarts that session's
 * sidecar: the session the window was watching is gone, so its status is read again.
 */
async function onSettingsChanged(): Promise<void> {
  await activeView.value?.refreshStatus();
}

/** Cross-thread search (`docs/12` §7.4), which §2.1's `Search ⌘K` row and `⌘K` both open. */
const searchOpen = ref(false);

/** The global agents panel (`docs/12` §9), which is app-wide rather than per thread. */
const agentsOpen = ref(false);

/**
 * The terminal drawer (`docs/12` §11, decision D6), which is app-wide too.
 *
 * The rows are the *host's* — a shell is a process this window does not own, and it is the
 * host that notices one exiting — so this keeps the set it was last told and which of them is
 * on screen. Both outlive a session switch, which is the point: a terminal is where a person
 * goes while the agent works.
 */
const terminalsOpen = ref(false);
const terminals = ref<TerminalTab[]>([]);
const activeTerminal = ref<string | null>(null);

/**
 * Open a terminal in `cwd`, and put it on screen.
 *
 * The size is a placeholder: the emulator measures itself once it is mounted and tells the
 * host, which is also what makes the shell redraw to it. 80×24 is the size a terminal has
 * been since VT100, so a shell that prints its prompt before the first resize sees a sane one.
 */
async function openTerminal(cwd: string): Promise<void> {
  if (cwd === "") {
    error.value = "there is nowhere to open a terminal: no workspace is open";
    return;
  }

  try {
    const opened = await terminalOpen(cwd, 80, 24);
    terminalsOpen.value = true;
    activeTerminal.value = opened.id;
  } catch (cause) {
    error.value = describe(cause);
  }
}

/** Select a tab. */
function selectTerminal(id: string): void {
  activeTerminal.value = id;
}

/**
 * Close a tab, ending its shell.
 *
 * The selection is decided *before* the answer, from the set on screen: the host's reply is
 * the remaining tabs, and by then the one that was closed is not in it to be found.
 */
async function closeTerminal(id: string): Promise<void> {
  const next = selectAfterClose(terminals.value, id, activeTerminal.value);

  try {
    terminals.value = tabsFrom(await terminalClose(id));
    activeTerminal.value = next;
  } catch (cause) {
    error.value = describe(cause);
  }
}

/**
 * What a window action asked for, by id (`lib/palette.ts`'s `WINDOW_ACTIONS`).
 *
 * One dispatcher rather than one emit per action: the composer reports which row was taken, and
 * the window decides what that means — a mapping it needs the moment there are two rows.
 */
function onAppAction(id: string): void {
  if (id === "settings") {
    openSettings(true);
    return;
  }

  if (id === "log-in") {
    logIn();
  }
}

/**
 * The login affordance.
 *
 * The engine has no `omp login` — `docs/04` records that `/login` exists only inside the TUI and
 * that `omp login`/`omp logout` are not commands at all. What does exist is its credential
 * vault's own OAuth flow, `omp auth-broker login`. That flow is interactive by design (a browser
 * and a callback), so the app's terminal is the honest host for it rather than an attempt to
 * drive it over RPC — and the credential never passes through this window, because the vault
 * writes it where the engine reads it.
 *
 * The command is written to the pty immediately, with no wait for a prompt: measured on a real
 * pty, bytes written before the shell has printed anything are read and executed as its first
 * line, so a delay here would be a guess dressed as caution.
 */
function logIn(): void {
  // A terminal needs a directory, and this is the app's existing rule for one that has none —
  // the same refusal `openTerminal` gives, for the same reason: a shell started somewhere nobody
  // chose is a shell in the wrong place. A first run has to open a directory before it has a
  // composer at all, so nothing here is reachable before that point anyway.
  const current = terminals.value.find((tab) => tab.id === activeTerminal.value)?.cwd;
  const cwd = current ?? workspace.value ?? "";
  if (cwd === "") {
    error.value = "there is nowhere to log in from: no workspace is open";
    return;
  }

  void (async () => {
    try {
      const opened = await terminalOpen(cwd, 80, 24);
      terminalsOpen.value = true;
      activeTerminal.value = opened.id;
      await terminalWrite(opened.id, "omp auth-broker login\r");
    } catch (cause) {
      error.value = describe(cause);
    }
  })();
}

/** Another tab in the directory the active one runs in, or in the workspace. */
function openTerminalBeside(): void {
  const current = terminals.value.find((tab) => tab.id === activeTerminal.value)?.cwd;
  void openTerminal(current ?? workspace.value ?? "");
}

/** The open thread menu (`docs/12` §2.3): which thread, and where the press landed. */
const menu = ref<{
  id: string;
  at: { x: number; y: number };
  /** A row shortcut can open one action without drawing the full menu. */
  action?: "delete";
} | null>(null);
/** The thread whose title is being edited in place. */
const renameId = ref<string | null>(null);

let unlisten: UnlistenFn[] = [];

/**
 * The thread ids that need a mounted view: only live threads with active sidecars.
 *
 * Each live thread keeps its view mounted so background streaming and events keep landing.
 * A thread without a live sidecar is never mounted — attempting to query thread status or
 * transcript on an inactive thread would fail with no live thread.
 */
const viewIds = computed(() => {
  return [...new Set(live.value.map((thread) => thread.id))];
});

const groups = computed(() =>
  sidebarModel({
    // The engine's own titles are folded in here rather than written into the catalogue: one
    // overlay feeds the row and the panel, and a later catalogue read is still authoritative.
    sessions: catalogue.value.map((session) => {
      const title = engineTitles.value[session.id];
      return title === undefined ? session : { ...session, title };
    }),
    live: live.value,
    unread: [...unread.value],
    hidden: projects.value.filter((project) => project.hidden).map((project) => project.path),
    now: Date.now(),
  }),
);

/** Every row on screen, for the lookups below. */
const rows = computed(() => groups.value.flatMap((group) => group.threads));

/** The project the active thread belongs to, for the titlebar label. */
const project = computed(() =>
  activeId.value === null ? null : (rows.value.find((row) => row.id === activeId.value)?.project ?? null),
);

/** Where a new thread would start: the active thread's project, else the last opened. */
const workspace = computed(() => project.value ?? recent.value[0] ?? null);

const activeView = computed(() => (activeId.value ? views[activeId.value] : undefined));
// Every read of the view's exposed surface goes through `?.` on the second hop as well as
// the first: `defineExpose` is not type-linked to `ThreadViewApi`, so a field the view
// forgot to expose is a compile-time secret and a runtime throw inside a render.
const activeStatus = computed(() => activeView.value?.status ?? null);
/** Whether the active thread has a sidecar: without one there is nothing to write to. */
const activeLive = computed(() => {
  const id = activeId.value;
  return id !== null && live.value.some((thread) => thread.id === id);
});

/** The engine's plan for the active thread: `get_state.todoPhases`, and nothing else. */
const activePhases = computed(() => activeStatus.value?.control.todoPhases ?? []);

function revealRow(at: number): void {
  void activeView.value?.revealRow(at);
}

/**
 * The agents panel's jump: reveal a conversation row in the thread that owns it.
 *
 * The panel is global, so the row it names may belong to a thread that is not on screen —
 * selecting it is part of the jump rather than something the caller has to remember.
 */
async function revealIn(thread: string, at: number): Promise<void> {
  if (thread !== "" && thread !== activeId.value) await select(thread);
  agentsOpen.value = false;
  revealRow(at);
}

/** A plan was written: the answer is the engine's, so the state that carries it is re-read. */
async function onPlanWritten(): Promise<void> {
  await activeView.value?.refreshStatus();
}
const activeActivity = computed(() => activeView.value?.activity ?? []);
const activeDialogs = computed(() => activeView.value?.dialogs ?? []);
/** A parked run is the one state worth shouting about, and it belongs to the active thread. */
const blocked = computed(() => activeDialogs.value.length > 0);

/** The row the menu is about, so its rules describe the right thread. */
const menuRow = computed(() =>
  menu.value === null ? null : (rows.value.find((row) => row.id === menu.value?.id) ?? null),
);

/**
 * What the menu's rules need about that thread.
 *
 * Its own live row decides streaming, and *any* live thread is enough for the engine
 * commands that take a session id — which is why pinning a cold session is possible while a
 * different thread is open, and impossible with none.
 */
const menuContext = computed<ThreadContext>(() => {
  const id = menu.value?.id ?? "";
  const thread = live.value.find((entry) => entry.id === id);
  const stored = catalogue.value.find((entry) => entry.id === id) ?? null;
  return {
    session: stored === null ? null : { pinned: stored.pinned, cwd: stored.cwd, path: stored.path },
    live: thread !== undefined,
    streaming: thread?.streaming ?? false,
    anyLive: live.value.length > 0,
  };
});

/** A live thread that can carry another session's engine commands (`/pin`). */
const dispatcher = computed(() => live.value[0]?.id ?? null);

onMounted(async () => {
  window.addEventListener("keydown", onShortcut);

  unlisten = await Promise.all([
    // The live set is replaced wholesale. Which thread *finished* is a fact the host reports as
    // a notification, so unread marks are not derived here as well — two sources for one ring.
    onThreadsUpdated((threads) => {
      const left = live.value.some(
        (thread) => !threads.some((next) => next.id === thread.id),
      );
      live.value = threads;
      // A thread that left the live set is either a manual stop or the idle policy releasing it
      // (`docs/11` §3.1), and only the catalogue knows which: `suspended` is the host's id set.
      // Re-read then, rather than on every change — a turn starting and ending moves this event
      // without anything about suspension having moved.
      if (left) void refresh();
    }),
    onNotifications((event) => onNotification(event)),
    onChrome((event) => onChromeEvent(event)),
    onTerminals((published) => {
      const before = terminals.value;
      const active = activeTerminal.value;
      terminals.value = tabsFrom(published);

      // The host is authoritative, so a tab it no longer lists is gone — however it went. And
      // the selection has to survive that: a closed tab's neighbour is where the user lands,
      // and a set nobody selected from keeps its first tab on screen.
      activeTerminal.value = terminals.value.some((tab) => tab.id === active)
        ? active
        : selectFrom(terminals.value, selectAfterClose(before, active ?? "", active));
    }),
    onAgents((event) => {
      // Replaced, not merged: the host publishes a thread's whole roster on every change, so
      // the last one it sent *is* what that thread has.
      rosters.value = [
        ...rosters.value.filter((entry) => entry.thread !== event.thread),
        { thread: event.thread, agents: event.payload, error: null },
      ];
    }),
    onModelsUpdated((catalogue) => {
      models.value = catalogue.options;
      refreshing.value = catalogue.refreshing;
    }),
  ]);

  try {
    const cached = await readModels();
    models.value = cached.options;
    refreshing.value = cached.refreshing;
    favourites.value = await readFavourites();
    void refreshModels();
  } catch (cause) {
    error.value = describe(cause);
  }

  await refresh();

  // The sidebar's count has to be right before anything is opened, and the host only pushes a
  // thread's roster when it *changes* — so a subagent that started before this window
  // connected is announced by nobody. One read at startup covers every thread the app can
  // still ask; from then on the stream keeps it current.
  void refreshAgents();

  // The same for the tabs: the host pushes the set when it changes, and a tab that existed
  // before this window subscribed would otherwise be a shell running with no tab to show it.
  try {
    terminals.value = tabsFrom(await readTerminals());
    activeTerminal.value = selectFrom(terminals.value, activeTerminal.value);
  } catch (cause) {
    error.value = describe(cause);
  }

  // A workspace argument means the app was launched *for* a project, so open it rather than
  // showing an empty state the user did not ask for.
  try {
    const context = await launchContext();
    identity.value = { user: context.user, version: context.version };
    if (context.workspace) await open(context.workspace);
  } catch (cause) {
    error.value = describe(cause);
  }
});

onUnmounted(() => {
  for (const off of unlisten) off();
  for (const timer of actionNoticeTimers.values()) clearTimeout(timer);
  window.removeEventListener("keydown", onShortcut);
});

/**
 * The window's own accelerators (`docs/12` §1: "standard accelerators work as keyboard
 * handlers", since D8 keeps no native menu).
 *
 * `⌘K`, `⌘N` and `⌘,`, and `Ctrl` for the same keys because this is Linux as often as it is a
 * Mac. Anything with a modifier that is not one of these is left alone, so typing in the
 * composer is untouched.
 */
function onShortcut(event: KeyboardEvent): void {
  if (!event.metaKey && !event.ctrlKey) return;

  const key = event.key.toLowerCase();
  if (key === "k") {
    event.preventDefault();
    searchOpen.value = !searchOpen.value;
    return;
  }
  if (key === "n") {
    event.preventDefault();
    const target = workspace.value;
    if (target !== null) void open(target);
  }
  if (key === ",") {
    event.preventDefault();
    settings.value = !settings.value;
  }
}

/** A search result was chosen: open that thread and land on the matching row. */
async function onSearchPick(thread: string, hit: SearchHit): Promise<void> {
  searchOpen.value = false;
  await select(thread);
  // The hit names a *record* from the index; the view matches it to the row it is rendered
  // as, which is the only honest mapping between the two (the app's rows are its own
  // reduction of the message stream).
  await nextTick();
  await views[thread]?.jumpTo(hit);
}

/** Re-read the store and the live set, without disturbing what is on screen. */
async function refresh(): Promise<void> {
  const [sessions, projectList, threads] = await Promise.all([
    readSessions().catch(() => catalogue.value),
    readProjects().catch(() => projects.value),
    readThreads().catch(() => live.value),
  ]);
  catalogue.value = sessions;
  projects.value = projectList;
  live.value = threads;
}

/**
 * Open a thread: a new session in `workspace`, or a resume of `resume`.
 *
 * The catalogue is refreshed afterwards because a resumed session's file just changed, and
 * a brand-new one may not be on disk at all yet — measured engine behaviour: a session
 * stays memory-only until it contains an assistant message.
 */
async function open(target: string, resume?: string): Promise<void> {
  busy.value = true;
  error.value = null;
  try {
    const thread = await openThread(target, resume);
    recent.value = [target, ...recent.value.filter((path) => path !== target)];
    if (!live.value.some((t) => t.id === thread.id)) {
      live.value = [...live.value, thread];
    }
    activeId.value = thread.id;
    markRead(thread.id);
    // Whatever the previous process pushed went with it; this one's extensions push their own.
    forgetChrome(thread.id);
    void refreshModels();
  } catch (cause) {
    error.value = describe(cause);
  } finally {
    busy.value = false;
    await refresh();
  }
}

/** Select a thread, starting it if it has no sidecar yet. */
async function select(id: string): Promise<void> {
  if (busy.value) return;
  markRead(id);
  activeId.value = id;
  if (live.value.some((thread) => thread.id === id)) return;

  const session = catalogue.value.find((entry) => entry.id === id);
  if (!session) {
    error.value = "that session is no longer in the store";
    activeId.value = null;
    return;
  }
  if (session.cwd === "") {
    // Resuming needs a working directory to spawn in, and the bucket name a session was
    // filed under is a lossy encoding of one (see `omp-store`): nothing to guess from.
    error.value = "that session recorded no working directory, so it cannot be resumed";
    return;
  }

  await open(session.cwd, id);
}

/** The menu's Rename: the sidebar turns that row into an input. */
function startRename(id: string): void {
  renameId.value = id;
}

async function renameThread(id: string, name: string): Promise<void> {
  renameId.value = null;
  // The user's name is the newest one: an engine `setTitle` from earlier in this process must
  // not keep drawing over the row the rename just wrote.
  if (engineTitles.value[id] !== undefined) {
    const titles = { ...engineTitles.value };
    delete titles[id];
    engineTitles.value = titles;
  }
  try {
    await sendRename(id, name);
    // The engine answers a bare ack and emits nothing at all, so the new title is only
    // visible once the catalogue re-reads the slot the engine rewrote in place.
    await refresh();
  } catch (cause) {
    error.value = describe(cause);
  }
}

/** A flow finished; whatever it changed is re-read here. */
async function onActionRefresh(): Promise<void> {
  await refresh();
  // The active thread's own view is asked too: a handoff moves the context ring without
  // adding a row, and nothing else would re-read the status for it.
  await activeView.value?.reload();
}

/** A fork answered with a session the catalogue has never seen. */
async function onForked(forked: ThreadSnapshot): Promise<void> {
  // Order matters: the resume needs the workspace, and only the re-read catalogue knows it.
  await refresh();
  await select(forked.id);
}

/** The session was deleted: there is nothing left to show. */
function onDeleted(id: string): void {
  if (activeId.value === id) activeId.value = null;
  unread.value = new Set([...unread.value].filter((entry) => entry !== id));
  notices.value = notices.value.filter((entry) => entry.thread !== id);
  live.value = live.value.filter((thread) => thread.id !== id);
  forgetChrome(id);
  if (engineTitles.value[id] !== undefined) {
    const next = { ...engineTitles.value };
    delete next[id];
    engineTitles.value = next;
  }
}

const projectToDelete = ref<ProjectGroup | null>(null);
const deletingProject = ref(false);

async function confirmDeleteProject(): Promise<void> {
  const group = projectToDelete.value;
  if (!group) return;
  deletingProject.value = true;
  const name = group.name;
  try {
    for (const thread of group.threads) {
      try {
        if (thread.live) {
          await closeThread(thread.id);
        }
        await deleteSession(thread.id);
        onDeleted(thread.id);
      } catch {
        // continue with other sessions in group
      }
    }
    showActionNotice(`Deleted project "${name}" and all its sessions`);
    await refresh();
  } catch (cause) {
    error.value = describe(cause);
  } finally {
    deletingProject.value = false;
    projectToDelete.value = null;
  }
}

function markRead(id: string): void {
  if (!unread.value.has(id)) return;
  const next = new Set(unread.value);
  next.delete(id);
  unread.value = next;
}

/**
 * One fact the host reported, delivered the three ways `docs/12` §13 asks for: an unread ring
 * on the row, an in-app notice, and — when the window is in the background and this is not the
 * thread on screen — the operating system's own notification.
 *
 * Which of the three applies is `lib/notify.ts`'s decision, made per fact; this is where those
 * answers meet the window's state. Nothing here rewrites the host's sentences.
 */
function onNotification(event: NotificationEvent): void {
  // The row already shows the name the engine recorded for this thread (a user or auto title
  // from the catalogue). The host's own `title` is that name when it has one and the session id
  // when it does not — measured: a fresh session's banner read `01a0c0f7-…`. So the catalogue's
  // naming wins where it exists, and the host's stands where it does not; both are the engine's,
  // and neither is a summary we invented. The body is always the host's sentence.
  const name = threadTitles.value[event.thread] ?? event.title;

  if (marksUnread(event, activeId.value)) {
    unread.value = new Set([...unread.value, event.thread]);
  }
  if (showsNotice(event.thread, activeId.value)) {
    noticeSeq += 1;
    notices.value = addNotice(
      notices.value,
      noticeFor({ ...event, title: name }, `notice-${noticeSeq}`),
      NOTICE_LIMIT,
    );
  }

  void deliverOs(event.thread, name, event.body);
}

/**
 * One chrome push (`protocol::ui::chrome`).
 *
 * Two of the six ops are *state* and land in the record the thread's own column draws its
 * status line and widget from. The other four happen once. The URL takes the host's own
 * allowlisted opener — a second path out of the window would be a second thing to keep safe —
 * and `editor-text` writes only the box on screen (`lib/chrome.ts`).
 */
function onChromeEvent(event: ChromeEvent): void {
  const op = event.op;

  if (op.kind === "status" || op.kind === "widget") {
    chrome.value = applyChrome(chrome.value, event.thread, op);
    return;
  }

  if (op.kind === "title") {
    engineTitles.value = { ...engineTitles.value, [event.thread]: op.title };
    return;
  }

  if (op.kind === "editor-text") {
    if (!draftTarget(activeId.value, event.thread)) return;
    views[event.thread]?.setDraft(op.text);
    return;
  }

  if (op.kind === "open-url") {
    void openExternal(op.url).catch((cause) => {
      error.value = describe(cause);
    });
    return;
  }

  // An extension's `notify` is the same act as a reported notification, so it takes the same
  // two deliveries: the notice's title is the thread's own name (the op carries none), and the
  // sentence is the engine's.
  //
  // The notice is *not* gated on the thread being off screen, unlike the four trigger kinds
  // (`showsNotice`). Those are facts about a turn the person may already be watching; this is
  // an extension saying something to them, and dropping it because they happen to be looking
  // at that thread is how a message disappears without a trace. The OS banner still follows
  // the focus rule, because the notice in the window is already carrying it.
  const name = threadTitles.value[event.thread] ?? "";
  noticeSeq += 1;
  notices.value = addNotice(
    notices.value,
    noticeFromChrome(event.thread, name, op.message, op.level, `notice-${noticeSeq}`),
    NOTICE_LIMIT,
  );

  void deliverOs(event.thread, name, op.message);
}

/**
 * The OS notification, if this fact wants one.
 *
 * Focus is **read** when the fact arrives rather than tracked across `tauri://focus` events:
 * one boolean, no listener to leak, no state to be wrong at startup — and the read only happens
 * for a thread that is not on screen, so a turn finishing in front of the user costs nothing.
 * The window is the thing the OS knows about, so its own answer (`plugin:window|is_focused`) is
 * used rather than the document's.
 *
 * The send itself is the host's, which is what makes it a real API on every platform rather
 * than a webview global that may not be there. It is also the one delivery that is allowed to
 * fail: the notice in the window already carries the same fact, and its own sentences.
 */
async function deliverOs(thread: string, title: string, body: string): Promise<void> {
  if (thread === activeId.value) return;

  const focused = await getCurrentWindow().isFocused().catch(() => true);
  if (!shouldNotifyOs(thread, activeId.value, focused)) return;

  try {
    await notifyOs(title, body);
  } catch {
    // Nothing to say: the fact is on screen, and a banner about a banner is noise.
  }
}

/** Open a notice's thread: what its row does, plus clearing the mark the notice is about. */
async function openNotice(id: string): Promise<void> {
  const notice = notices.value.find((entry) => entry.id === id);
  if (notice === undefined) return;

  notices.value = dropNotice(notices.value, id);
  if (notice.thread !== null) await select(notice.thread);
}

/** Put app-owned action receipts on the same bottom-right surface as other notices. */
function showActionNotice(message: string): void {
  noticeSeq += 1;
  const id = `notice-${noticeSeq}`;
  notices.value = addNotice(
    notices.value,
    noticeForAction(message, id),
    NOTICE_LIMIT,
  );
  actionNoticeTimers.set(
    id,
    setTimeout(() => {
      notices.value = dropNotice(notices.value, id);
      actionNoticeTimers.delete(id);
    }, ACTION_NOTICE_DURATION),
  );
}

/**
 * Forget a thread's chrome.
 *
 * The status and the widget belong to the *process* that pushed them: a resumed thread has a
 * new one and the engine's extensions push theirs again at startup, so carrying the old
 * thread's lines into it would show a widget for a session that no longer has one.
 */
function forgetChrome(id: string): void {
  const pending = chrome.value[id];
  if (pending === undefined || (pending.status === null && pending.widget === null)) return;

  const next = { ...chrome.value };
  delete next[id];
  chrome.value = next;
}

/**
 * Remember a thread's view.
 *
 * A `null` is **not** a removal: Vue calls a `v-for` ref with `null` while it patches the
 * list — which happens on every roster update, since a turn starting or ending moves a
 * thread's place — and erasing the entry there would leave the drawer reading `undefined`
 * for a thread that is very much on screen. Entries only ever leave this map with the
 * thread, and a caller that asks for one that has none gets a defensive `?.`.
 */
function registerView(id: string, view: unknown): void {
  if (view === null || view === undefined) return;
  views[id] = view as ThreadViewApi;
}

async function onFavourites(keys: string[]): Promise<void> {
  try {
    favourites.value = await setFavourites(keys);
  } catch (cause) {
    error.value = describe(cause);
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
</script>

<template>
  <div class="flex h-full w-full bg-canvas text-fg overflow-hidden">
    <!--
      Settings covers the window's working area (`docs/12` §12): the rail and the section stand
      where the sidebar and the thread's column were, and the terminal drawer below goes with
      them — it is a place in the window, not a panel over one.
    -->
    <SettingsScreen
      v-if="settings"
      :thread="activeId"
      :identity="identity"
      :sessions="catalogue.length"
      :models="models"
      :favourites="favourites"
      :refreshing="refreshing"
      :mode="activeStatus?.approvalMode ?? null"
      @back="openSettings(false)"
      @changed="onSettingsChanged"
      @favourites="onFavourites"
    />

    <!--
      `v-show`, not `v-if`: a thread's view is subscribed to its session while it is on screen,
      and hiding the columns must not tear that down and rebuild it for a look at settings.
      The sidebar extends all the way to the top of the window as a full-height column on the left.
    -->
    <div v-show="!settings" class="flex h-full w-full min-h-0 flex-1">
      <!-- Left sidebar with drag-to-resize and collapse threshold -->
      <div
        class="relative flex h-full shrink-0"
        :style="{ width: sidebarCollapsed ? '0px' : `${sidebarWidth}px` }"
        :class="{ 'transition-[width] duration-150 ease-out': !isDraggingLeft }"
      >
        <div class="h-full w-full overflow-hidden">
          <Sidebar
            v-show="!sidebarCollapsed"
            class="h-full w-full min-w-0"
            :groups="groups"
            :active-id="activeId"
            :busy="busy"
            :workspace="workspace"
            :identity="identity"
            :sessions="catalogue.length"
            :rename-id="renameId"
            :agents="activeAgents"
            @select="select"
            @open="open($event)"
            @context="(id, at) => (menu = { id, at })"
            @delete="(id) => (menu = { id, at: { x: 0, y: 0 }, action: 'delete' })"
            @renamed="renameThread"
            @cancel-rename="renameId = null"
            @delete-project="projectToDelete = $event"
            @search="searchOpen = true"
            @agents="openAgents"
            @settings="openSettings(true)"
            @collapse="sidebarCollapsed = true"
          />
        </div>

        <!-- Resize border handle on right edge of sidebar -->
        <div
          v-show="!sidebarCollapsed"
          class="absolute top-0 -right-1"
          :class="[RESIZE_HANDLE_CLASS, isDraggingLeft ? 'after:opacity-100' : '']"
          title="Drag to resize, double-click to reset"
          @pointerdown="startLeftResize"
          @dblclick="resetLeftWidth"
        />
      </div>

      <!-- Edge hit zone to drag sidebar open when collapsed -->
      <div
        v-if="sidebarCollapsed"
        class="fixed top-0 left-0"
        :class="[RESIZE_HANDLE_CLASS, isDraggingLeft ? 'after:opacity-100' : '']"
        title="Drag to open sidebar"
        @pointerdown="startLeftResize"
        @dblclick="resetLeftWidth"
      />

      <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
        <TitleBar
          :workspace="workspace"
          :busy="busy"
          :project="project"
          :title="activeRow?.title ?? null"
          :sidebar-collapsed="sidebarCollapsed"
          :diagnostics="diagnostics"
          :panel="panelOpen"
          :terminals="terminalsOpen"
          @new-thread="workspace !== null && open(workspace)"
          @toggle-sidebar="sidebarCollapsed = !sidebarCollapsed"
          @toggle-panel="panelOpen = !panelOpen"
          @toggle-diagnostics="diagnostics = !diagnostics"
          @toggle-terminals="terminalsOpen = !terminalsOpen"
        />

        <p
          v-if="error"
          class="flex items-start gap-2 whitespace-pre-wrap bg-err/10 px-3 py-2 text-[12px] text-err select-text"
          data-error
        >
          <span class="min-w-0 flex-1">{{ error }}</span>
          <button class="shrink-0 text-err/70 hover:text-err select-none" @click="error = null">dismiss</button>
        </p>

        <main class="flex min-h-0 flex-1 flex-col overflow-hidden">
          <template v-for="id in viewIds" :key="id">
            <ThreadView
              v-show="id === activeId"
              :ref="(view) => registerView(id, view)"
              :thread="id"
              :chrome="chromeFor(chrome, id)"
              :models="models"
              :refreshing="refreshing"
              :favourites="favourites"
              :recent="recent"
              :user="identity.user"
              class="flex-1"
              @failed="error = $event"
              @workspace="open($event)"
              @terminal="openTerminal"
              @favourites="onFavourites"
              @app-action="onAppAction"
            />
          </template>

          <!--
            `docs/12` §4: with nothing open there is nothing to type into, so the empty state
            says what the two ways in are rather than showing a disabled composer.
          -->
          <div
            v-if="activeId === null"
            class="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center"
            data-empty-state
          >
            <p class="text-[15px] text-dim">No thread open</p>
            <p class="max-w-sm text-[12.5px] leading-relaxed text-faint">
              Pick a session from the sidebar to resume it, or add a project to start a new one.
              Sessions are grouped by the directory they were started in.
            </p>
          </div>

          <div
            v-else-if="busy && !live.some((thread) => thread.id === activeId)"
            class="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center"
          >
            <Icon name="refresh" class="h-6 w-6 animate-spin text-accent" />
            <p class="text-[13px] text-dim">Resuming thread…</p>
          </div>

          <div
            v-else-if="!live.some((thread) => thread.id === activeId)"
            class="mx-auto my-auto flex flex-col items-center gap-3 p-6 text-center"
          >
            <p class="text-[13px] text-dim">This thread has no live sidecar</p>
            <button
              class="rounded-[6px] border border-line px-3 py-1 text-[12px] text-dim hover:border-line-strong hover:text-fg"
              @click="activeId !== null && select(activeId)"
            >
              Resume thread
            </button>
          </div>
        </main>

        <!--
          The terminal drawer (`docs/12` §11, decision D6), below the columns rather than inside a
          thread's: a shell is where the user works *while* the agent does, and the reason to have
          one is usually that the next thing they do is switch threads. `v-show`, not `v-if`: its
          emulators keep drawing while it is closed, so opening it again is not a replay.
        -->
        <TerminalPanel
          v-show="terminalsOpen && !settings"
          :tabs="terminals"
          :active="activeTerminal"
          :visible="terminalsOpen && !settings"
          @select="selectTerminal"
          @close="closeTerminal"
          @open="openTerminalBeside"
          @failed="error = $event"
        />
      </div>

      <!-- Resizable Right Panel Column (full-height to window top, matching Sidebar) -->
      <div
        v-if="panelOpen || diagnostics"
        class="relative flex h-full shrink-0"
        :style="{ width: `${rightPanelWidth}px` }"
        :class="{ 'transition-[width] duration-150 ease-out': !isDraggingRight }"
      >
        <!-- Resize border handle on left edge of right panel -->
        <div
          class="absolute top-0 -left-1"
          :class="[RESIZE_HANDLE_CLASS, isDraggingRight ? 'after:opacity-100' : '']"
          title="Drag to resize, double-click to reset"
          @pointerdown="startRightResize"
          @dblclick="resetRightWidth"
        />

        <div class="h-full w-full overflow-hidden">
          <DiagnosticsPanel
            v-if="diagnostics"
            class="h-full w-full min-w-0"
            :thread="activeId"
            :ready="activeStatus?.ready ?? null"
            :status="activeStatus"
            :counters="activeStatus?.counters ?? null"
            :activity="activeActivity"
            :blocked="blocked"
            :busy="busy"
            :workspace="workspace"
            :terminals="terminalsOpen"
            @close="diagnostics = false"
            @new-thread="workspace !== null && open(workspace)"
            @toggle-terminals="terminalsOpen = !terminalsOpen"
            @toggle-panel="diagnostics = false; panelOpen = true"
          />
          <RightPanel
            v-else-if="panelOpen"
            class="h-full w-full min-w-0"
            :thread="activeId"
            :label="activeRow?.title ?? null"
            :phases="activePhases"
            :rows="activeRows"
            :cwd="activeRow?.project ?? null"
            :live="activeLive"
            :busy="busy"
            :workspace="workspace"
            :terminals="terminalsOpen"
            :diagnostics="diagnostics"
            @jump="revealRow"
            @written="onPlanWritten"
            @failed="error = $event"
            @close="panelOpen = false"
            @new-thread="workspace !== null && open(workspace)"
            @toggle-terminals="terminalsOpen = !terminalsOpen"
            @toggle-diagnostics="diagnostics = !diagnostics"
          />
        </div>
      </div>
    </div>

    <!--
      The thread menu and the dialogs its actions need (`docs/12` §2.3). It lives here
      rather than in the sidebar because two of its actions decide *which* thread the window
      is showing: a fork mints a new id, a delete removes one.
    -->
    <!--
      The global agents panel (`docs/12` §9). It lives here rather than in a thread's column
      because it is app-wide: a subagent runs beside the conversation, and its transcript is
      not a second view of the thread on screen.
    -->
    <AgentsPanel
      v-if="agentsOpen"
      :rosters="rosters"
      :titles="threadTitles"
      :active-id="activeId"
      :live="live.map((thread) => thread.id)"
      :rows="activeRows"
      @reveal="revealIn"
      @close="agentsOpen = false"
    />

    <SearchOverlay
      v-if="searchOpen"
      :sessions="catalogue"
      @pick="onSearchPick"
      @close="searchOpen = false"
    />

    <ThreadActions
      :menu="menu"
      :context="menuContext"
      :dispatcher="dispatcher"
      :label="menuRow?.title ?? ''"
      @close="menu = null"
      @refresh="onActionRefresh"
      @select="select"
      @forked="onForked"
      @rename="startRename"
      @deleted="onDeleted"
      @notice="showActionNotice"
      @failed="error = $event"
    />

    <Modal
      v-if="projectToDelete"
      :title="`Delete ${projectToDelete.name}`"
      :busy="deletingProject"
      @close="projectToDelete = null"
    >
      <p class="mb-2 text-[12.5px] text-fg">
        Delete project <span class="font-semibold text-fg">{{ projectToDelete.name }}</span> and all its
        {{ projectToDelete.threads.length }} session{{ projectToDelete.threads.length === 1 ? '' : 's' }}?
        This cannot be undone.
      </p>
      <p class="mb-3 text-[11.5px] leading-relaxed text-faint">
        All session files recorded for this working directory will be permanently removed.
      </p>
      <div class="mt-4 flex items-center gap-2">
        <button
          :disabled="deletingProject"
          class="rounded-[6px] bg-err/15 border border-err/30 px-3 py-1.5 text-[12px] font-medium text-err hover:bg-err/25 disabled:opacity-40"
          @click="confirmDeleteProject"
        >
          {{ deletingProject ? "Deleting…" : "Delete project" }}
        </button>
        <button
          :disabled="deletingProject"
          class="rounded-[6px] px-3 py-1.5 text-[12px] text-dim hover:bg-raised hover:text-fg disabled:opacity-40"
          @click="projectToDelete = null"
        >
          Cancel
        </button>
      </div>
    </Modal>

    <!--
      One bottom-right stack for both engine notifications and app action receipts. Several can
      land at once, and one must not replace another before it has been read. The container takes
      no pointer events, so the column under it is still the column.
    -->
    <TransitionGroup
      name="toast"
      tag="div"
      class="pointer-events-none fixed bottom-3 right-3 z-30 flex w-[22rem] flex-col gap-1.5"
      data-notices
    >
      <div
        v-for="entry in notices"
        :key="entry.id"
        class="pointer-events-auto flex items-start gap-1.5 rounded-[8px] border bg-raised px-2.5 py-2 shadow-lg"
        :class="
          entry.tone === 'error'
            ? 'border-err/50'
            : entry.tone === 'warn'
              ? 'border-warn/50'
              : 'border-line-strong'
        "
        data-toast
      >
        <div class="min-w-0 flex-1">
          <p class="truncate text-[12px] font-medium text-fg">{{ entry.title }}</p>
          <p class="mt-0.5 line-clamp-3 text-[11.5px] leading-snug text-dim">{{ entry.body }}</p>
        </div>
        <button
          v-if="entry.thread !== null"
          class="shrink-0 rounded-[5px] px-1 text-[11px] text-dim hover:bg-surface hover:text-fg"
          title="show this thread"
          data-action="open-notice"
          @click="openNotice(entry.id)"
        >
          open
        </button>
        <button
          class="shrink-0 rounded-[5px] px-1 text-[11px] text-faint hover:bg-surface hover:text-fg"
          title="dismiss"
          data-action="dismiss-notice"
          @click="notices = dropNotice(notices, entry.id)"
        >
          dismiss
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
