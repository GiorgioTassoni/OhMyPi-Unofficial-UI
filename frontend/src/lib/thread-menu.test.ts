/**
 * The thread menu's rules, asserted (`docs/12` §2.3).
 *
 * Each case here is a state the app really reaches: a thread the engine has not written to
 * disk yet, a session mid-turn, a live thread that must not have its file deleted out from
 * under it, and a store with no sidecar at all — where pinning has nobody to dispatch to.
 */

import { describe, expect, test } from "bun:test";
import { menuFor, visibleMenu, type ThreadContext } from "./thread-menu";

function context(extra: Partial<ThreadContext> = {}): ThreadContext {
  return {
    session: { pinned: false, cwd: "/home/me/Projects/App", path: "/home/me/.omp/agent/sessions/x/y.jsonl" },
    live: true,
    streaming: false,
    anyLive: true,
    ...extra,
  };
}

function item(ctx: ThreadContext, action: string) {
  return menuFor(ctx).find((entry) => entry.action === action);
}

describe("the thread menu", () => {
  test("everything is available for a cold session in a store with a live thread", () => {
    const ctx = context({ live: false });
    const disabled = menuFor(ctx).filter((entry) => entry.disabled).map((entry) => entry.action);
    // Only the two that are about a live process: handoff needs a sidecar to run in, and
    // stop has nothing to stop. `visibleMenu` hides the second rather than showing it dead.
    expect(disabled.sort()).toEqual(["handoff", "stop"]);
  });

  test("a memory-only thread offers neither fork nor export", () => {
    const ctx = context({ session: null, live: true });
    expect(item(ctx, "fork")?.disabled).toBe(true);
    expect(item(ctx, "fork")?.reason).toContain("no session file");
    expect(item(ctx, "export")?.disabled).toBe(true);
    // …but it can still be renamed and stopped, which are about the process, not the file.
    expect(item(ctx, "rename")?.disabled).toBe(false);
    expect(item(ctx, "stop")?.disabled).toBe(false);
  });

  test("deleting a live thread is offered because deletion stops it first", () => {
    const ctx = context({ live: true });
    expect(item(ctx, "delete")?.disabled).toBe(false);
    expect(item(ctx, "delete")?.label).toBe("Delete conversation…");
  });

  test("a memory-only thread can be deleted when not streaming", () => {
    const ctx = context({ session: null, live: true, streaming: false });
    expect(item(ctx, "delete")?.disabled).toBe(false);
    expect(item(ctx, "delete")?.label).toBe("Delete conversation…");
  });

  test("a memory-only thread can be deleted mid-turn because deletion stops it first", () => {
    const ctx = context({ session: null, live: true, streaming: true });
    expect(item(ctx, "delete")?.disabled).toBe(false);
  });

  test("handing off mid-turn is refused, because the engine refuses it", () => {
    const ctx = context({ live: true, streaming: true });
    expect(item(ctx, "handoff")?.disabled).toBe(true);
    expect(item(ctx, "handoff")?.reason).toBe("a turn is running");
  });

  test("pinning needs somebody to dispatch to", () => {
    const ctx = context({ anyLive: false, live: false });
    expect(item(ctx, "pin")?.disabled).toBe(true);
    expect(item(ctx, "pin")?.reason).toContain("open a thread first");
    expect(item(ctx, "pin")?.label).toBe("Pin");
  });

  test("a pinned session offers unpin", () => {
    const ctx = context({ session: { pinned: true, cwd: "/app", path: "/app/s.jsonl" } });
    expect(item(ctx, "pin")?.label).toBe("Unpin");
  });

  test("a session with no recorded directory cannot be revealed or copied", () => {
    const ctx = context({ session: { pinned: false, cwd: "", path: "/x/y.jsonl" }, live: false });
    expect(item(ctx, "reveal")?.disabled).toBe(true);
    expect(item(ctx, "copy-cwd")?.disabled).toBe(true);
  });

  test("stop is hidden rather than disabled when there is no sidecar", () => {
    const actions = visibleMenu(context({ live: false })).map((entry) => entry.action);
    expect(actions).not.toContain("stop");
    expect(menuFor(context({ live: false })).map((entry) => entry.action)).toContain("stop");
  });

  test("delete is the only destructive item", () => {
    const destructive = menuFor(context()).filter((entry) => entry.destructive);
    expect(destructive.map((entry) => entry.action)).toEqual(["delete"]);
  });
});
