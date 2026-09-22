/**
 * The terminal panel's model (`docs/12` §11, decision D6).
 *
 * The rows here are the host's own shapes (`dto::TerminalSnapshot`), which is why the exit
 * cases are written out in full: a shell that returned and a shell that was signalled are two
 * different things on screen, and the host reports no exit code for the second because the
 * kernel has none to give.
 */

import { describe, expect, test } from "bun:test";

import type { TerminalSnapshot } from "../bridge";
import {
  decodeOutput,
  endedFor,
  labelFor,
  Pending,
  PENDING_CAP,
  selectAfterClose,
  selectFrom,
  tabFor,
  tabsFrom,
} from "./terminals";

function terminal(over: Partial<TerminalSnapshot> = {}): TerminalSnapshot {
  return {
    id: "pty-1",
    cwd: "/home/user/Projects/OhMyPiApp",
    running: true,
    exit: null,
    pid: 4242,
    ...over,
  };
}

describe("what a tab is called", () => {
  test("a directory is its last segment", () => {
    expect(labelFor("/home/user/Projects/OhMyPiApp")).toBe("OhMyPiApp");
    expect(labelFor("OhMyPiApp")).toBe("OhMyPiApp");
  });

  test("a path with nothing to take is the path", () => {
    expect(labelFor("/")).toBe("/");
    expect(labelFor("///")).toBe("/");
    expect(labelFor("/home/user/")).toBe("user");
    expect(labelFor("")).toBe("/");
  });
});

describe("how a shell reads once it has ended", () => {
  test("a running shell says nothing about ending", () => {
    expect(endedFor(terminal())).toBe("");
  });

  test("a returned shell reads as its code", () => {
    const ended = terminal({ running: false, exit: { code: 7, signal: null } });
    expect(endedFor(ended)).toBe("exited 7");
  });

  test("a signalled shell reads as the platform names the signal", () => {
    // The host sends no code here — portable-pty carries a placeholder one for a signal
    // death, and "exited 1" for a `kill -TERM` would be a fabricated number.
    const ended = terminal({ running: false, exit: { code: null, signal: "Terminated" } });
    expect(endedFor(ended)).toBe("Terminated");
  });
});

describe("the strip", () => {
  test("the host's set is the strip, in its order", () => {
    const tabs = tabsFrom([
      terminal(),
      terminal({ id: "pty-2", cwd: "/home/user/Projects/PhantomGit" }),
    ]);

    expect(tabs.map((tab) => tab.id)).toEqual(["pty-1", "pty-2"]);
    expect(tabs.map((tab) => tab.label)).toEqual(["OhMyPiApp", "PhantomGit"]);
  });

  test("a set with nothing selected selects its first tab", () => {
    // The host can publish tabs this window never opened — a terminal that outlived the window
    // that started it, or one opened later. Leaving it unselected is a drawer with a tab and no
    // terminal, which is what the browser pass caught.
    const tabs = tabsFrom([terminal(), terminal({ id: "pty-2" })]);
    expect(selectFrom(tabs, null)).toBe("pty-1");
    expect(selectFrom([], null)).toBeNull();
  });

  test("a selection that still exists is left alone", () => {
    const tabs = tabsFrom([terminal(), terminal({ id: "pty-2" })]);
    expect(selectFrom(tabs, "pty-2")).toBe("pty-2");
  });

  test("a selection the host no longer lists falls back to the first tab", () => {
    const tabs = tabsFrom([terminal(), terminal({ id: "pty-2" })]);
    expect(selectFrom(tabs, "pty-404")).toBe("pty-1");
  });

  test("closing a tab that is not selected leaves the selection alone", () => {
    const tabs = tabsFrom([terminal(), terminal({ id: "pty-2" })]);
    expect(selectAfterClose(tabs, "pty-2", "pty-1")).toBe("pty-1");
  });

  test("closing the selected tab selects its neighbour", () => {
    const tabs = tabsFrom([
      terminal(),
      terminal({ id: "pty-2" }),
      terminal({ id: "pty-3" }),
    ]);

    expect(selectAfterClose(tabs, "pty-2", "pty-2")).toBe("pty-3");
    // The last one has no neighbour to its right, so its left one is where the user lands.
    expect(selectAfterClose(tabs, "pty-3", "pty-3")).toBe("pty-2");
    expect(selectAfterClose(tabs, "pty-1", "pty-1")).toBe("pty-2");
  });

  test("closing the only tab selects nothing", () => {
    expect(selectAfterClose(tabsFrom([terminal()]), "pty-1", "pty-1")).toBeNull();
  });

  test("a tab the strip no longer lists still leaves a defined selection", () => {
    const tabs = tabsFrom([terminal(), terminal({ id: "pty-2" })]);
    expect(selectAfterClose(tabs, "pty-404", "pty-404")).toBe("pty-1");
  });

  test("one row carries both the label and the way it ended", () => {
    const tab = tabFor(terminal({ running: false, exit: { code: 0, signal: null } }));
    expect(tab).toEqual({
      id: "pty-1",
      label: "OhMyPiApp",
      cwd: "/home/user/Projects/OhMyPiApp",
      running: false,
      ended: "exited 0",
    });
  });
});

describe("the host's bytes", () => {
  test("base64 comes back as the bytes the shell wrote", () => {
    // "hello\r\n" plus a colour escape and a UTF-8 multibyte character: a terminal's output is
    // neither ASCII nor text, which is why the host base64s it.
    const raw = new Uint8Array([104, 101, 108, 108, 111, 13, 10, 27, 91, 51, 49, 109, 226, 156, 147]);
    const decoded = decodeOutput(btoa(String.fromCharCode(...raw)));

    expect(decoded).toEqual(raw);
  });

  test("a payload that is not base64 is dropped rather than thrown", () => {
    expect(decodeOutput("not base64 !!")).toBeNull();
  });
});

describe("output that arrives before its emulator", () => {
  test("a batch is held and handed over once, in order", () => {
    const pending = new Pending();
    pending.push("pty-1", new Uint8Array([1, 2]));
    pending.push("pty-1", new Uint8Array([3]));

    expect(pending.size("pty-1")).toBe(3);
    expect(pending.take("pty-1")).toEqual(new Uint8Array([1, 2, 3]));
    // Taken, not read: a second drain must not replay a shell's prompt into a fresh screen.
    expect(pending.take("pty-1")).toBeNull();
    expect(pending.size("pty-1")).toBe(0);
  });

  test("tabs are held apart", () => {
    const pending = new Pending();
    pending.push("pty-1", new Uint8Array([1]));
    pending.push("pty-2", new Uint8Array([2]));

    expect(pending.take("pty-2")).toEqual(new Uint8Array([2]));
    expect(pending.take("pty-1")).toEqual(new Uint8Array([1]));
  });

  test("the cap keeps the newest bytes, because that is what a terminal shows", () => {
    const pending = new Pending();
    const chunk = new Uint8Array(100 * 1024).fill(7);
    for (let at = 0; at < 4; at += 1) pending.push("pty-1", chunk);

    const held = pending.size("pty-1");
    expect(held).toBeLessThanOrEqual(PENDING_CAP);
    expect(held).toBeGreaterThan(0);

    const batch = pending.take("pty-1");
    expect(batch?.length).toBe(held);
  });

  test("a forgotten tab is not held for the life of the window", () => {
    const pending = new Pending();
    pending.push("pty-1", new Uint8Array([1]));
    pending.forget("pty-1");

    expect(pending.size("pty-1")).toBe(0);
    expect(pending.take("pty-1")).toBeNull();
  });

  test("an empty batch is not a batch", () => {
    const pending = new Pending();
    pending.push("pty-1", new Uint8Array(0));

    expect(pending.size("pty-1")).toBe(0);
    expect(pending.take("pty-1")).toBeNull();
  });
});
