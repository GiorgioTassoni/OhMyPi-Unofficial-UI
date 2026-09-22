/**
 * Cross-thread search's model, asserted (`docs/12` §7.4).
 *
 * The cases a reviewer reads past: a match near the start or end of a long message (where an
 * ellipsis has to appear on one side only), a query that appears nowhere (no snippet rather
 * than an empty one), a hit in a thread the catalogue no longer lists, more matches in one
 * thread than the group shows, and a hit whose text lives in a tool card rather than a
 * message — which is exactly why the jump target is not an ordinal.
 */

import { row } from "./rows.fixture";

import { describe, expect, test } from "bun:test";
import { groupHits, jumpTarget, kindLabel, snippet } from "./search";
import type { SearchHit, SessionSummary } from "../bridge";

function hit(extra: Partial<SearchHit> = {}): SearchHit {
  return { thread: "a", kind: "answer", ordinal: 1, text: "the tokenizer re-reads the buffer", ...extra };
}

function session(id: string, extra: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id,
    path: `/sessions/${id}.jsonl`,
    bucket: "-tmp-app",
    cwd: "/tmp/app",
    title: `thread ${id}`,
    titleSource: "user",
    parentId: null,
    createdAt: null,
    modifiedAt: 0,
    messageCount: 1,
    size: 1,
    firstMessage: "hello",
    status: "complete",
    pinned: false,
    suspended: false,
    ...extra,
  };
}

describe("the snippet", () => {
  test("the match is returned with its original casing", () => {
    const found = snippet("the Tokenizer is slow", "tokenizer");
    expect(found?.match).toBe("Tokenizer");
    expect(found?.before).toBe("the ");
  });

  test("a match in the middle of a long message is windowed on both sides", () => {
    const text = `${"a".repeat(500)} NEEDLE ${"b".repeat(500)}`;
    const found = snippet(text, "needle");
    expect(found?.match).toBe("NEEDLE");
    expect(found?.before.startsWith("…")).toBe(true);
    expect(found?.after.endsWith("…")).toBe(true);
    expect((found?.before.length ?? 0) + (found?.after.length ?? 0)).toBeLessThan(200);
  });

  test("a match at the very start has no leading ellipsis", () => {
    const found = snippet("needle at the front of a long message", "needle");
    expect(found?.before).toBe("");
    expect(found?.match).toBe("needle");
  });

  test("no match means no snippet, not an empty one", () => {
    expect(snippet("nothing here", "elsewhere")).toBeNull();
    expect(snippet("nothing here", "   ")).toBeNull();
  });

  test("a window never splits a character", () => {
    // Cut by code unit and this returns replacement characters in the middle of a result.
    const text = `${"🙂".repeat(200)} needle ${"🙂".repeat(200)}`;
    const found = snippet(text, "needle");
    expect(found?.match).toBe("needle");
    expect(found?.before).not.toInclude("\uFFFD");
    expect(found?.after).not.toInclude("\uFFFD");
  });
});

describe("grouping", () => {
  test("a thread's position is its best hit's position", () => {
    const groups = groupHits(
      [hit({ thread: "b" }), hit({ thread: "a" }), hit({ thread: "b", ordinal: 2 })],
      [session("a"), session("b")],
    );

    expect(groups.map((group) => group.thread)).toEqual(["b", "a"]);
    expect(groups[0]?.hits).toHaveLength(2);
  });

  test("a group says when it is showing only some of its matches", () => {
    const hits = Array.from({ length: 6 }, (_, index) => hit({ thread: "a", ordinal: index }));
    const groups = groupHits(hits, [session("a")], 3);

    expect(groups[0]?.hits).toHaveLength(3);
    expect(groups[0]?.more).toBe(true);
  });

  test("a thread the catalogue does not have is still shown, by its first message or id", () => {
    const groups = groupHits([hit({ thread: "gone" })], []);
    expect(groups[0]?.title).toBe("gone");
  });

  test("the title comes from the catalogue, falling back to the first message", () => {
    const groups = groupHits(
      [hit({ thread: "a" }), hit({ thread: "b" })],
      [session("a", { title: null, firstMessage: "the first thing I said" }), session("b")],
    );

    expect(groups.find((group) => group.thread === "a")?.title).toBe("the first thing I said");
    expect(groups.find((group) => group.thread === "b")?.title).toBe("thread b");
  });
});

describe("the jump target", () => {
  const rows = [
    row({ role: "user", text: "why is the parser slow?" }),
    row({ role: "assistant", text: "because the tokenizer re-reads the buffer" }),
    row({ tool: { toolCallId: "c1", toolName: "bash", intent: null, args: '{"command":"rg token"}', result: "hit", status: "done", diff: null } as never }),
  ];

  test("a message hit finds the row that still holds its text", () => {
    expect(jumpTarget(rows, hit())).toBe(1);
  });

  test("a truncated hit matches on what is real, not on the marker", () => {
    const truncated = hit({ text: "because the tokenizer re-reads the buffer… [truncated]" });
    expect(jumpTarget(rows, truncated)).toBe(1);
  });

  test("a tool hit falls back to the card's tool name", () => {
    const tool = hit({ kind: "tool", text: 'bash {"command":"rg token"}' });
    expect(jumpTarget(rows, tool)).toBe(2);
  });

  test("a hit that is nowhere on screen jumps nowhere rather than to row zero", () => {
    expect(jumpTarget(rows, hit({ text: "text from a message this thread never rendered" }))).toBeNull();
  });
});

describe("kind labels", () => {
  test("only the kinds a reader would misread get a label", () => {
    expect(kindLabel("answer")).toBeNull();
    expect(kindLabel("prompt")).toBeNull();
    expect(kindLabel("thinking")).toBe("reasoning");
    expect(kindLabel("tool")).toBe("tool call");
    expect(kindLabel("result")).toBe("tool result");
  });
});
