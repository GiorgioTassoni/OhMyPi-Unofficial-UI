/**
 * The delivery rules, asserted (`docs/12` §13).
 *
 * What these defend is the part a template cannot: that the column on screen never interrupts,
 * that a background fact does, that both an extension's `notify` and a reported notification
 * are answered by the *same* rule, and that the stack of notices stays a stack rather than a
 * last-write-wins slot. Each is a rule a plausible edit could quietly break — the second
 * condition of the OS rule reads like decoration until a check for it exists.
 */

import { describe, expect, test } from "bun:test";
import {
  addNotice,
  dropNotice,
  marksUnread,
  noticeForAction,
  noticeFor,
  noticeFromChrome,
  shouldNotifyOs,
  showsNotice,
  toneForLevel,
} from "./notify";
import type { NotificationEvent } from "../bridge";

function event(extra: Partial<NotificationEvent> = {}): NotificationEvent {
  return {
    thread: "b",
    kind: "turn-finished",
    title: "Docs polish",
    body: "the turn finished",
    jobId: null,
    ...extra,
  };
}

describe("unread marks", () => {
  test("a turn that finished in another thread is unread", () => {
    expect(marksUnread(event(), "a")).toBe(true);
  });

  test("the thread on screen never goes unread", () => {
    expect(marksUnread(event({ thread: "a" }), "a")).toBe(false);
  });

  test("the other three kinds are not completion", () => {
    for (const kind of ["needs-you", "failed", "job-finished"] as const) {
      expect(marksUnread(event({ kind }), "a")).toBe(false);
    }
  });
});

describe("in-app notices", () => {
  test("the thread on screen never raises one, whatever the kind", () => {
    expect(showsNotice("a", "a")).toBe(false);
    expect(showsNotice("b", "a")).toBe(true);
    // With nothing on screen there is no column to be looking at, so everything shows.
    expect(showsNotice("a", null)).toBe(true);
  });

  test("the host's own words are carried through unchanged", () => {
    const notice = noticeFor(
      event({ kind: "failed", title: "bash", body: "exit status 2: no such file" }),
      "n1",
    );

    expect(notice).toEqual({
      id: "n1",
      thread: "b",
      title: "bash",
      body: "exit status 2: no such file",
      tone: "error",
    });
  });

  test("an extension's notify is the thread's name and the engine's sentence", () => {
    const notice = noticeFromChrome("b", "Docs polish", "wrote 12 files", "warn", "n2");

    expect(notice.title).toBe("Docs polish");
    expect(notice.body).toBe("wrote 12 files");
    expect(notice.tone).toBe("warn");
  });

  test("an unknown level word is an ordinary notice, not a failure", () => {
    expect(toneForLevel(null)).toBe("info");
    expect(toneForLevel("debug")).toBe("info");
  });

  test("an action receipt uses the toast stack without linking to a thread", () => {
    expect(noticeForAction("deleted Draft", "action-1")).toEqual({
      id: "action-1",
      thread: null,
      title: "Done",
      body: "deleted Draft",
      tone: "info",
    });
  });
});

describe("the OS notification", () => {
  test("another thread, with the window in the background: notify", () => {
    expect(shouldNotifyOs("b", "a", false)).toBe(true);
  });

  test("the thread on screen never reaches the operating system, focused or not", () => {
    expect(shouldNotifyOs("a", "a", true)).toBe(false);
    expect(shouldNotifyOs("a", "a", false)).toBe(false);
  });

  test("a focused window keeps it in-app", () => {
    expect(shouldNotifyOs("b", "a", true)).toBe(false);
  });
});

describe("the notice stack", () => {
  test("a second notice does not replace the first", () => {
    const first = noticeFor(event({ thread: "a" }), "n1");
    const second = noticeFor(event({ thread: "b" }), "n2");

    expect(addNotice(addNotice([], first, 3), second, 3).map((n) => n.id)).toEqual(["n1", "n2"]);
  });

  test("the stack is capped, newest kept", () => {
    const notices = [1, 2, 3].map((n) => noticeFor(event({ thread: `t${n}` }), `n${n}`));

    expect(addNotice(notices, noticeFor(event(), "n4"), 3).map((n) => n.id)).toEqual([
      "n2",
      "n3",
      "n4",
    ]);
  });

  test("dismissing one leaves the others", () => {
    const notices = [1, 2].map((n) => noticeFor(event({ thread: `t${n}` }), `n${n}`));

    expect(dropNotice(notices, "n1").map((n) => n.id)).toEqual(["n2"]);
    expect(dropNotice(notices, "gone")).toHaveLength(2);
  });
});
