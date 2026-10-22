import { describe, expect, test } from "bun:test";
import type { ToolSnapshot } from "../bridge";
import { wholeFileRead } from "./toolView";

function makeTool(name: string, args: Record<string, unknown>, finished = true, isError = false): ToolSnapshot {
  return {
    toolCallId: "call_1",
    toolName: name,
    intent: null,
    args: JSON.stringify(args),
    details: "",
    output: "file content here",
    isError,
    finished,
  };
}

describe("wholeFileRead", () => {
  test("identifies whole file read without range or offset parameters", () => {
    const tool = makeTool("read", { path: "src/main.ts" });
    const res = wholeFileRead(tool);
    expect(res.isWhole).toBe(true);
    expect(res.path).toBe("src/main.ts");
  });

  test("supports file_path property", () => {
    const tool = makeTool("read", { file_path: "/etc/hosts" });
    const res = wholeFileRead(tool);
    expect(res.isWhole).toBe(true);
    expect(res.path).toBe("/etc/hosts");
  });

  test("rejects partial reads with line range or offset", () => {
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts", offset: 100 })).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts", limit: 50 })).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts", start_line: 10 })).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts", end_line: 20 })).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts", line_range: [1, 20] })).isWhole).toBe(false);
  });

  test("rejects unfinished or failed reads or non-read tools", () => {
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts" }, false)).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("read", { path: "src/a.ts" }, true, true)).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("write", { path: "src/a.ts" })).isWhole).toBe(false);
    expect(wholeFileRead(makeTool("bash", { command: "cat file.txt" })).isWhole).toBe(false);
  });
});
