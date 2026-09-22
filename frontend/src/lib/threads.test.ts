/**
 * The sidebar's model, asserted (`docs/12` §2).
 *
 * These are the cases a reviewer reads past in a template: a brand-new thread with no file
 * behind it yet, a fork that has to sit under its parent, a thread that finished while the
 * user was elsewhere, a cold session whose last turn was interrupted, and a session that
 * recorded no working directory at all. Each is invisible until it is wrong on screen.
 */

import { describe, expect, test } from "bun:test";
import { displayName, dotFor, projectName, sidebarModel } from "./threads";
import type { SessionSummary, ThreadSnapshot } from "../bridge";

const NOW = 1_700_000_000_000;

function session(id: string, extra: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id,
    path: `/home/me/.omp/agent/sessions/-Projects-App/2026-09-20T00-00-00-000Z_${id}.jsonl`,
    bucket: "-Projects-App",
    cwd: "/home/me/Projects/App",
    title: null,
    titleSource: null,
    parentId: null,
    createdAt: "2026-09-20T00:00:00.000Z",
    modifiedAt: NOW,
    messageCount: 4,
    size: 1024,
    firstMessage: "hello",
    status: "complete",
    pinned: false,
    suspended: false,
    ...extra,
  };
}

function live(id: string, extra: Partial<ThreadSnapshot> = {}): ThreadSnapshot {
  return {
    id,
    workspace: "/home/me/Projects/App",
    title: null,
    streaming: false,
    pendingApprovals: 0,
    error: null,
    ...extra,
  };
}

describe("the dot", () => {
  test("a thread with no sidecar is hollow, whatever it did last time", () => {
    expect(dotFor(undefined)).toBe("cold");
  });

  test("an approval outranks streaming, because it is the thing that needs the user", () => {
    expect(dotFor(live("a", { streaming: true, pendingApprovals: 1 }))).toBe("attention");
    expect(dotFor(live("a", { streaming: true }))).toBe("streaming");
    expect(dotFor(live("a"))).toBe("idle");
  });

  test("a failure outranks a live process that is otherwise quiet", () => {
    expect(dotFor(live("a", { error: "turn failed" }))).toBe("error");
  });
});

describe("the display name", () => {
  test("the engine's chain: title, then first message, then a timestamp", () => {
    expect(displayName("Fix the parser", "hello", NOW, null)).toBe("Fix the parser");
    expect(displayName(null, "explain the diff", NOW, null)).toBe("explain the diff");
    expect(displayName(null, "(no messages)", NOW, null)).toStartWith("Untitled · ");
  });

  test("a multi-line title is one displayable line", () => {
    expect(displayName("first\nsecond", "hello", NOW, null)).toBe("first");
  });

  test("a whitespace title falls through rather than becoming an empty row", () => {
    expect(displayName("   ", "hello", NOW, null)).toBe("hello");
  });
});

describe("the project name", () => {
  test("the last segment is what the sidebar shows", () => {
    expect(projectName("/home/me/Projects/App")).toBe("App");
    expect(projectName("/home/me/Projects/App/")).toBe("App");
    expect(projectName("")).toBe("No project");
  });
});

describe("the model", () => {
  test("sessions group by the cwd they recorded, not by their directory", () => {
    const groups = sidebarModel({
      sessions: [session("a"), session("b", { cwd: "/home/me/Projects/Other" })],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups.map((group) => group.name).sort()).toEqual(["App", "Other"]);
    expect(groups.find((group) => group.name === "Other")?.threads.map((row) => row.id)).toEqual(["b"]);
  });

  test("a thread that exists only in memory is still listed", () => {
    // Measured engine behaviour: a new session stays memory-only until it has an assistant
    // message, so right after the user opens a thread there is no file to list.
    const groups = sidebarModel({
      sessions: [],
      live: [live("fresh", { title: "brand new" })],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups).toHaveLength(1);
    expect(groups[0]?.threads.map((row) => row.title)).toEqual(["brand new"]);
    expect(groups[0]?.threads[0]?.live).toBe(true);
  });

  test("a generated live title replaces the catalogue's first-message fallback", () => {
    const groups = sidebarModel({
      sessions: [session("named", { firstMessage: "the entire original prompt" })],
      live: [live("named", { title: "Concise Generated Title" })],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads[0]?.title).toBe("Concise Generated Title");
  });

  test("a fork is indented under the thread it came from", () => {
    const groups = sidebarModel({
      sessions: [
        session("parent", { modifiedAt: NOW - 1000 }),
        session("child", { parentId: "parent", modifiedAt: NOW }),
      ],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads.map((row) => [row.id, row.depth])).toEqual([
      ["parent", 0],
      ["child", 1],
    ]);
  });

  test("a fork of a fork is still under its own parent", () => {
    // Forking the fork is an ordinary thing to do, and the ids are what the row is keyed
    // on: an indented row that goes missing takes its whole subtree with it.
    const groups = sidebarModel({
      sessions: [
        session("first", { modifiedAt: NOW }),
        session("second", { parentId: "first", modifiedAt: NOW - 1 }),
        session("third", { parentId: "second", modifiedAt: NOW - 2 }),
      ],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads.map((row) => [row.id, row.depth])).toEqual([
      ["first", 0],
      ["second", 1],
      ["third", 2],
    ]);
  });

  test("two sessions that name each other as parent do not hang the sidebar", () => {
    const groups = sidebarModel({
      sessions: [
        session("a", { parentId: "b", modifiedAt: NOW }),
        session("b", { parentId: "a", modifiedAt: NOW - 1 }),
      ],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads.map((row) => [row.id, row.depth])).toEqual([
      ["a", 0],
      ["b", 1],
    ]);
  });

  test("a fork whose parent is not in this project is not hidden behind it", () => {
    const groups = sidebarModel({
      sessions: [session("child", { parentId: "elsewhere" })],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads.map((row) => [row.id, row.depth])).toEqual([["child", 0]]);
  });

  test("a pinned thread comes first, then newest", () => {
    const groups = sidebarModel({
      sessions: [
        session("old", { modifiedAt: NOW - 5000 }),
        session("new", { modifiedAt: NOW }),
        session("pinned", { modifiedAt: NOW - 9000, pinned: true }),
      ],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads.map((row) => row.id)).toEqual(["pinned", "new", "old"]);
  });

  test("a cold session carries what happened last time, a live one does not", () => {
    const groups = sidebarModel({
      sessions: [session("cut", { status: "interrupted" }), session("done", { status: "complete" })],
      live: [live("done")],
      unread: [],
      hidden: [],
      now: NOW,
    });

    const rows = groups[0]?.threads ?? [];
    expect(rows.find((row) => row.id === "cut")?.note).toBe("interrupted");
    expect(rows.find((row) => row.id === "done")?.note).toBeNull();
  });

  test("a project the user hid does not appear", () => {
    const groups = sidebarModel({
      sessions: [session("a")],
      live: [],
      unread: [],
      hidden: ["/home/me/Projects/App"],
      now: NOW,
    });

    expect(groups).toEqual([]);
  });

  test("unread is carried through, and only for the ids that are unread", () => {
    const groups = sidebarModel({
      sessions: [session("a"), session("b")],
      live: [],
      unread: ["b"],
      hidden: [],
      now: NOW,
    });

    const rows = groups[0]?.threads ?? [];
    expect(rows.find((row) => row.id === "b")?.unread).toBe(true);
    expect(rows.find((row) => row.id === "a")?.unread).toBe(false);
  });
});

describe("suspended", () => {
  test("a thread the host released says so", () => {
    const groups = sidebarModel({
      sessions: [session("released", { suspended: true })],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads[0]?.suspended).toBe(true);
  });

  test("a thread with a process never says it", () => {
    // The host's set and a live sidecar cannot both be the truth, and the process is the one
    // on screen: a row that said "suspended" over a streaming thread would deny its own dot.
    const groups = sidebarModel({
      sessions: [session("released", { suspended: true })],
      live: [live("released", { streaming: true })],
      unread: [],
      hidden: [],
      now: NOW,
    });

    const row = groups[0]?.threads[0];
    expect(row?.suspended).toBe(false);
    expect(row?.dot).toBe("streaming");
  });

  test("a session the host has not released is not labelled", () => {
    const groups = sidebarModel({
      sessions: [session("cold")],
      live: [],
      unread: [],
      hidden: [],
      now: NOW,
    });

    expect(groups[0]?.threads[0]?.suspended).toBe(false);
  });
});
