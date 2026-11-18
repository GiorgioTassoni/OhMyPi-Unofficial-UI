import { invoke, listenPayload, type UnlistenFn } from "./core";

/**
 * One live terminal tab.
 *
 * A fact about a process rather than about a thread: the shell the user opened, in a
 * workspace, which outlives every session switch. Nothing here describes the agent — its
 * `bash` runs without a terminal (`PI_NO_PTY=1`) and cannot see this one.
 */
export interface TerminalSnapshot {
  /** Minted by the host; every other terminal command takes it. */
  id: string;
  /** The directory the shell was started in — the tab's label. */
  cwd: string;
  /** Whether the shell is still running. A tab outlives its shell so the last screenful stays. */
  running: boolean;
  /** How it ended, once it has. */
  exit: { code: number | null; signal: string | null } | null;
  /** The shell's process id, while that means something. */
  pid: number | null;
}

/** One batch of a terminal's output, base64 because terminal bytes are not text. */
export interface TerminalOutput {
  id: string;
  data: string;
}

/** Every terminal the app has open, in the order its tabs were opened. */
export async function terminals(): Promise<TerminalSnapshot[]> {
  return invoke<TerminalSnapshot[]>("terminals");
}

/**
 * Open a terminal in `cwd`, running the user's own shell.
 *
 * `cols` and `rows` are what the emulator measured: the kernel passes the size to the shell,
 * which is what makes a full-screen program in here draw correctly.
 */
export async function terminalOpen(
  cwd: string,
  cols: number,
  rows: number,
): Promise<TerminalSnapshot> {
  return invoke<TerminalSnapshot>("terminal_open", { cwd, cols, rows });
}

/** Send typed or pasted input to a terminal. */
export async function terminalWrite(id: string, data: string): Promise<void> {
  return invoke<void>("terminal_write", { id, data });
}

/** Tell a terminal's shell its window changed size. */
export async function terminalResize(id: string, cols: number, rows: number): Promise<void> {
  return invoke<void>("terminal_resize", { id, cols, rows });
}

/** Close a tab, ending its shell and everything it started. Returns the remaining tabs. */
export async function terminalClose(id: string): Promise<TerminalSnapshot[]> {
  return invoke<TerminalSnapshot[]>("terminal_close", { id });
}

/** Subscribe to every terminal's output. One channel: batches are tagged with their tab. */
export async function onTerminalOutput(
  handler: (output: TerminalOutput) => void,
): Promise<UnlistenFn> {
  return listenPayload<TerminalOutput>("terminal-output", handler);
}

/** Subscribe to the tab set, whenever a terminal is opened, exits or is closed. */
export async function onTerminals(
  handler: (terminals: TerminalSnapshot[]) => void,
): Promise<UnlistenFn> {
  return listenPayload<TerminalSnapshot[]>("terminals-updated", handler);
}
