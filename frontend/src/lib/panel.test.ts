/**
 * The right panel's model, asserted (`docs/12` §8).
 *
 * These are the cases a reviewer would notice if they were wrong: a file written once and
 * then edited twice (one row, "created", three touches, the *latest* diff), a read that
 * changed nothing (no row), the list not reordering itself mid-turn, and a task whose words
 * are nowhere in the transcript (no jump rather than a wrong one).
 */

import { describe, expect, test } from "bun:test";
import { artifacts, changedFiles, relativeTo, taskRow } from "./panel";
import { row } from "./rows.fixture";
import type { ToolSnapshot } from "../bridge";

function tool(extra: Partial<ToolSnapshot> = {}): ToolSnapshot {
  return {
    toolCallId: "call-1",
    toolName: "edit",
    intent: null,
    args: "",
    details: "",
    output: "",
    isError: false,
    finished: true,
    ...extra,
  };
}

describe("the changed files", () => {
  test("a write then two edits is one file, written, with the latest diff", () => {
    const files = changedFiles([
      row({
        tool: tool({
          toolName: "write",
          args: '{"path":"a.ts","content":"x"}',
          details: '{"resolvedPath":"/app/a.ts","madeExecutable":false}',
        }),
      }),
      row({
        tool: tool({
          args: '{"path":"/app/a.ts"}',
          details: '{"path":"/app/a.ts","diff":"@@ -1 +1 @@\\n-a\\n+b"}',
        }),
      }),
      row({
        tool: tool({
          args: '{"path":"/app/a.ts"}',
          details: '{"path":"/app/a.ts","diff":"@@ -2 +2 @@\\n-b\\n+c"}',
        }),
      }),
    ]);

    expect(files).toHaveLength(1);
    expect(files[0]).toEqual({
      path: "/app/a.ts",
      kind: "written",
      touches: 3,
      diff: "@@ -2 +2 @@\n-b\n+c",
      row: 2,
    });
  });

  test("a batched edit lists every file and claims no per-file diff", () => {
    // The engine joins the per-file diffs into one string for a batch, so showing it against
    // each file would attribute a change to a file the diff does not describe.
    const files = changedFiles([
      row({
        tool: tool({
          args: '{"edits":[]}',
          details:
            '{"diff":"@@ one @@\\n@@ two @@","perFileResults":[{"path":"/a.ts"},{"path":"/b.ts"}]}',
        }),
      }),
    ]);

    expect(files.map((file) => [file.path, file.diff])).toEqual([
      ["/a.ts", null],
      ["/b.ts", null],
    ]);
  });

  test("ast_edit reports the files it replaced, not its scope", () => {
    const files = changedFiles([
      row({
        tool: tool({
          toolName: "ast_edit",
          args: '{"pat":"console.log($$$)","paths":["src"]}',
          details:
            '{"filesTouched":2,"scopePath":"src","fileReplacements":[{"path":"/src/a.ts","count":1},{"path":"/src/b.ts","count":2}]}',
        }),
      }),
    ]);

    expect(files.map((file) => file.path)).toEqual(["/src/a.ts", "/src/b.ts"]);
    expect(files.every((file) => file.kind === "edited")).toBe(true);
  });

  test("a move changed the file it landed in and the one it left", () => {
    const files = changedFiles([
      row({
        tool: tool({
          args: '{"path":"/old.ts","dest":"/new.ts"}',
          details: '{"path":"/new.ts","sourcePath":"/old.ts","diff":"@@"}',
        }),
      }),
    ]);

    expect(files.map((file) => file.path)).toEqual(["/new.ts", "/old.ts"]);
  });

  test("a call that has not ended, or that failed, changed nothing yet", () => {
    const pending = row({
      tool: tool({ args: '{"path":"/a.ts"}', finished: false }),
    });
    const failed = row({
      tool: tool({ args: '{"path":"/b.ts"}', isError: true }),
    });

    expect(changedFiles([pending, failed])).toEqual([]);
  });

  test("two files keep the order they were first touched in", () => {
    const rows = [
      row({ tool: tool({ toolName: "write", args: '{"path":"/b.ts","content":""}' }) }),
      row({ tool: tool({ args: '{"path":"/a.ts"}' }) }),
      // Touching /b.ts again must not move it: a list that reorders while a turn streams
      // is a list nobody can click.
      row({ tool: tool({ args: '{"path":"/b.ts"}' }) }),
    ];

    expect(changedFiles(rows).map((file) => [file.path, file.touches])).toEqual([
      ["/b.ts", 2],
      ["/a.ts", 1],
    ]);
  });

  test("a read is not a change, and a path-less call is not a file", () => {
    const files = changedFiles([
      row({ tool: tool({ toolName: "read", args: '{"path":"/a.ts"}' }) }),
      row({ tool: tool({ toolName: "bash", args: '{"command":"ls"}', details: '{"stdout":"x"}' }) }),
      row({ tool: tool({ toolName: "edit", args: "not json" }) }),
      row({ tool: tool({ args: '{"file_path":"/from-file-path.ts"}' }) }),
    ]);

    expect(files.map((file) => file.path)).toEqual(["/from-file-path.ts"]);
    expect(files[0]?.diff).toBeNull();
  });
});

describe("the artifacts", () => {
  test("a truncated result is listed with the id it spilled to", () => {
    const found = artifacts([
      row({ tool: tool({ toolName: "bash", output: "…" }) }),
      row({
        tool: tool({
          toolName: "bash",
          details: '{"meta":{"truncation":{"artifactId":"7f3c","bytes":900}},"command":"rg x"}',
        }),
      }),
    ]);

    expect(found).toEqual([{ id: "7f3c", tool: "bash", row: 1 }]);
  });

  test("a result that was not truncated contributes nothing", () => {
    expect(artifacts([row({ tool: tool({ output: "short" }) })])).toEqual([]);
    expect(artifacts([row({ tool: tool({ details: '{"meta":{}}' }) })])).toEqual([]);
  });
});

describe("finding a task's row", () => {
  const todo = (args: string) => row({ tool: tool({ toolName: "todo", args }) });

  test("the earliest mention is where a reader wants to be taken", () => {
    const rows = [
      todo('{"phases":[{"name":"Build","tasks":[{"content":"Wire the panel"}]}]}'),
      row({ tool: tool({ toolName: "bash", args: '{"command":"ls"}' }) }),
      todo('{"phases":[{"name":"Build","tasks":[{"content":"Wire the panel","status":"completed"}]}]}'),
    ];

    expect(taskRow(rows, "Wire the panel")).toBe(0);
  });

  test("a task the transcript never mentions has no row", () => {
    expect(taskRow([todo('{"phases":[]}')], "Wire the panel")).toBeNull();
    expect(taskRow([], "Wire the panel")).toBeNull();
    expect(taskRow([todo('{"phases":[]}')], "   ")).toBeNull();
  });
});

describe("shortening a path", () => {
  test("a file inside the workspace is shown relative to it", () => {
    expect(relativeTo("/app/src/a.ts", "/app")).toBe("src/a.ts");
    expect(relativeTo("/app/src/a.ts", "/app/")).toBe("src/a.ts");
  });

  test("a path that does not share the root is left whole", () => {
    expect(relativeTo("/other/a.ts", "/app")).toBe("/other/a.ts");
    // `/application` is not inside `/app`, which a prefix test without the separator would miss.
    expect(relativeTo("/application/a.ts", "/app")).toBe("/application/a.ts");
    expect(relativeTo("/app/a.ts", null)).toBe("/app/a.ts");
  });
});
