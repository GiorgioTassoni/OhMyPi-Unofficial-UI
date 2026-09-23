import { describe, expect, test } from "bun:test";
import type { ToolSnapshot } from "../bridge";
import { activityDiff, activityDiffCounts } from "./activityDiff";

function tool(toolName: string, args: string, details: string): ToolSnapshot {
  return { toolCallId: "call", toolName, intent: null, args, details, output: "", isError: false, finished: true };
}

describe("activity edit diffs", () => {
  test("shows only changed lines and per-file stats for a batch edit", () => {
    const diff = activityDiff(tool("edit", "{}", JSON.stringify({
      diff: "joined",
      perFileResults: [
        { path: "/a.ts", diff: "@@ -1,3 +1,3 @@\n same\n-old\n+new\n same" },
        { path: "/b.ts", diff: "@@ -4 +4 @@\n-before\n+after" },
      ],
    })));
    expect(diff.files.map((file) => [file.path, file.added, file.removed])).toEqual([
      ["/a.ts", 1, 1], ["/b.ts", 1, 1],
    ]);
    expect(diff.files[0].pieces.some((piece) => piece.kind === "diff" && piece.text.includes("same"))).toBe(false);
    expect([diff.added, diff.removed, diff.batch]).toEqual([2, 2, null]);
    expect(activityDiffCounts(tool("edit", "{}", JSON.stringify({
      perFileResults: [
        { path: "/a.ts", diff: "@@ -1 +1 @@\n-old\n+new" },
        { path: "/b.ts", diff: "-before\n+after" },
      ],
    })))).toEqual({ added: 2, removed: 2 });
  });

  test("never displays a whole-file write as a diff", () => {
    const diff = activityDiff(tool("write", '{"path":"/new.ts","content":"entire file"}', '{"resolvedPath":"/new.ts"}'));
    expect(diff.files[0].pieces).toEqual([]);
    expect(diff.added).toBeNull();
  });
});
