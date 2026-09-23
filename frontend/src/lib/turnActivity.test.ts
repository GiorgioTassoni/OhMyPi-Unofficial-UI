import { describe, expect, test } from "bun:test";
import type { ToolSnapshot } from "../bridge";
import { row, said } from "./rows.fixture";
import { conversationTurns, workLabel } from "./turnActivity";

function call(toolName: string, args: string, details = "", extra: Partial<ToolSnapshot> = {}) {
  return row({ role: "tool", tool: {
    toolCallId: `${toolName}-${args}`, toolName, intent: null, args, details,
    output: "", isError: false, finished: true, ...extra,
  } });
}

describe("conversation turn activity", () => {
  test("keeps each answer outside the grouped work and uses real timestamps", () => {
    const rows = [
      row({ role: "user", text: "please check", timestamp: 1_000 }),
      call("read", '{"path":"one.ts"}'),
      call("read", '{"path":"two.ts"}'),
      call("bash", '{"command":"bun test"}'),
      call("bash", '{"command":"bun run build"}'),
      call("edit", '{"path":"one.ts"}', '{"path":"one.ts","diff":"@@ -1 +1 @@\\n-old\\n+new"}'),
      call("edit", '{"path":"two.ts"}', '{"path":"two.ts","diff":"@@ -1 +1 @@\\n-a\\n+b"}'),
      call("edit", '{"path":"three.ts"}', '{"path":"three.ts","diff":"@@ -1 +1 @@\\n-c\\n+d"}'),
      said("All done", { timestamp: 63_000 }),
      row({ role: "user", text: "next", timestamp: 64_000 }),
      call("read", '{"path":"four.ts"}', "", { finished: false }),
    ];
    const turns = conversationTurns(rows, true);
    expect(turns).toHaveLength(2);
    expect(turns[0].answer?.row.text).toBe("All done");
    expect(workLabel(turns[0])).toBe("Worked for 1m 02s");
    expect(turns[0].items.filter((item) => item.kind === "group").map((item) => item.group.label))
      .toEqual(["Read 2 files", "Ran 2 commands", "Edited 3 files"]);
    expect(turns[0].items[2].kind === "group" ? turns[0].items[2].group.added : null).toBe(3);
    expect(workLabel(turns[1])).toBe("Working…");
  });

  test("unknown timing stays neutral and unknown tools are not dropped", () => {
    const turns = conversationTurns([
      row({ role: "user", text: "hi" }), call("extension_magic", "{}"), said("okay"),
    ], false);
    expect(workLabel(turns[0])).toBe("Completed turn");
    expect(turns[0].items[0].kind === "group" && turns[0].items[0].group.label).toBe("Used 1 tool");
  });

  test("does not invent line counts for a write without a diff", () => {
    const turns = conversationTurns([
      row({ role: "user", text: "write" }),
      call("write", '{"path":"new.ts","content":"hello"}', '{"resolvedPath":"new.ts"}'),
      said("done"),
    ], false);
    const item = turns[0].items[0];
    expect(item.kind === "group" && item.group.label).toBe("Edited 1 file");
    expect(item.kind === "group" && item.group.added).toBeNull();
  });

  test("only adjacent calls group; a thought or another tool splits the run", () => {
    const turns = conversationTurns([
      row({ role: "user", text: "check" }),
      call("read", '{"path":"one.ts"}'),
      call("read", '{"path":"two.ts"}'),
      said("thinking", { thinking: "considering" }),
      call("read", '{"path":"three.ts"}'),
      call("bash", '{"command":"test"}'),
      call("read", '{"path":"four.ts"}'),
      said("done"),
    ], false);
    expect(turns[0].items.map((item) => item.kind === "group" ? item.group.label : "assistant"))
      .toEqual(["Read 2 files", "assistant", "Read 1 file", "Ran 1 command", "Read 1 file"]);
    expect(turns[0].items[0].kind === "group" ? turns[0].items[0].group.added : 0).toBeNull();
  });
});
