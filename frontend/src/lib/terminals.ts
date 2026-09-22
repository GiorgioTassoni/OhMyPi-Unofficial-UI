/**
 * The terminal panel's model (`docs/12` §11, decision D6).
 *
 * The panel is a set of tabs, each one a shell the *host* owns. What this module holds is
 * everything about them that is a decision rather than a rendering: what a tab is called, how
 * a shell that has ended reads, which tab is selected after one closes, and the two pieces of
 * byte plumbing the emulator needs — decoding what the host posts, and holding output that has
 * nowhere to go yet.
 *
 * The last of those is a real race rather than a precaution. A shell prints its prompt the
 * moment it is allocated, which is *before* the window that asked for it has an emulator to
 * put it in: the answer to `terminal_open` is what tells the window the tab's id, so the first
 * batch of output always arrives first. Output for a tab nobody is listening to is kept here
 * (the tail of it — a terminal shows the newest line, not the oldest) and drained when the
 * emulator mounts.
 */

import type { TerminalSnapshot } from "../bridge";

/** How much of a tab's output is held while it has no emulator. */
export const PENDING_CAP = 256 * 1024;

/** One tab, as the strip draws it. */
export interface TerminalTab {
  id: string;
  /** The workspace's own name: the directory the shell was started in. */
  label: string;
  /** The whole path, for the tab's title and the panel's own readout. */
  cwd: string;
  running: boolean;
  /**
   * How the shell ended, in the platform's own words, and `""` while it runs.
   *
   * A signal death reads as the platform names it (`Terminated`, `Hanged up`) rather than as
   * an invented exit code: the host reports no code for one, because there is none to report.
   */
  ended: string;
}

/**
 * What a directory is called in a tab.
 *
 * Its last segment, because that is what a person calls a project, and the whole path when
 * there is no segment to take (a root directory, which has no name of its own).
 */
export function labelFor(cwd: string): string {
  const trimmed = cwd.replace(/\/+$/, "");
  if (trimmed === "") return "/";
  const at = trimmed.lastIndexOf("/");
  return at === -1 ? trimmed : trimmed.slice(at + 1);
}

/** How a shell's ending reads. */
export function endedFor(terminal: TerminalSnapshot): string {
  const exit = terminal.exit;
  if (!exit) return "";
  if (exit.signal) return exit.signal;
  return `exited ${exit.code ?? 0}`;
}

/** One tab, from the host's own row. */
export function tabFor(terminal: TerminalSnapshot): TerminalTab {
  return {
    id: terminal.id,
    label: labelFor(terminal.cwd),
    cwd: terminal.cwd,
    running: terminal.running,
    ended: endedFor(terminal),
  };
}

/** The whole strip, from the host's own set — which is authoritative, so this replaces. */
export function tabsFrom(terminals: TerminalSnapshot[]): TerminalTab[] {
  return terminals.map(tabFor);
}

/**
 * The tab to show, given what the host says exists.
 *
 * The selection is the window's, but it has to be *defined*: the host publishes tabs this
 * window did not open (a terminal from before it subscribed, one opened by another window),
 * and a set with nothing selected is a drawer full of tabs and no terminal. Which is what it
 * was, until a browser check measured a 0×0 host and an emulator that never got a size.
 */
export function selectFrom(tabs: TerminalTab[], active: string | null): string | null {
  if (active !== null && tabs.some((tab) => tab.id === active)) return active;
  return tabs[0]?.id ?? null;
}

/**
 * Which tab is selected once `closed` is gone.
 *
 * The neighbour, not the first tab: closing one of several terminals should leave the user
 * where they were working rather than at the far end of the strip.
 */
export function selectAfterClose(
  tabs: TerminalTab[],
  closed: string,
  active: string | null,
): string | null {
  if (active !== closed) return active;

  const at = tabs.findIndex((tab) => tab.id === closed);
  const rest = tabs.filter((tab) => tab.id !== closed);
  if (rest.length === 0) return null;
  if (at === -1) return rest[0]?.id ?? null;

  return (rest[at] ?? rest[rest.length - 1])?.id ?? null;
}

/**
 * The bytes behind one `terminal-output` payload.
 *
 * `null` for a payload that is not base64. The host is the only writer on this channel, so
 * this cannot be user input — but a terminal that throws inside an event listener would take
 * the panel's subscription down with it, and a dropped batch is a smaller failure than a dead
 * panel.
 */
export function decodeOutput(data: string): Uint8Array | null {
  let binary: string;
  try {
    binary = atob(data);
  } catch {
    return null;
  }

  const bytes = new Uint8Array(binary.length);
  for (let at = 0; at < binary.length; at += 1) {
    bytes[at] = binary.charCodeAt(at);
  }
  return bytes;
}

/**
 * Output that arrived before its tab had an emulator.
 *
 * Per tab, and bounded: a shell that prints a megabyte while the emulator is being created
 * would otherwise be held whole, and what a terminal is *for* is the newest screenful.
 */
export class Pending {
  private queue = new Map<string, Uint8Array[]>();
  private totals = new Map<string, number>();

  /** Keep one batch, dropping the oldest bytes once the cap is reached. */
  push(id: string, chunk: Uint8Array): void {
    if (chunk.length === 0) return;

    const chunks = this.queue.get(id) ?? [];
    chunks.push(chunk);
    let total = (this.totals.get(id) ?? 0) + chunk.length;

    while (total > PENDING_CAP && chunks.length > 1) {
      total -= chunks.shift()?.length ?? 0;
    }

    this.queue.set(id, chunks);
    this.totals.set(id, total);
  }

  /** Everything held for a tab, and nothing held afterwards. */
  take(id: string): Uint8Array | null {
    const chunks = this.queue.get(id);
    if (chunks === undefined || chunks.length === 0) return null;

    const batch = new Uint8Array(this.totals.get(id) ?? 0);
    let at = 0;
    for (const chunk of chunks) {
      batch.set(chunk, at);
      at += chunk.length;
    }

    this.forget(id);
    return batch;
  }

  /** How many bytes are held for a tab. */
  size(id: string): number {
    return this.totals.get(id) ?? 0;
  }

  /** Give up on a tab, so a closed one is not held for the life of the window. */
  forget(id: string): void {
    this.queue.delete(id);
    this.totals.delete(id);
  }
}
