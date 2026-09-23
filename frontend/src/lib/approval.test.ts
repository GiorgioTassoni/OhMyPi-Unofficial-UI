/**
 * The approval prompt, asserted from the engine's real layouts.
 *
 * The strings here are copied from what `omp` v18.2.6 actually sent during the
 * measurements recorded in `docs/12` §10 (`Allow tool: bash` + `Command:`, and
 * `Allow tool: write` + `Path:`/`Content:`), not imagined. What the cases defend is
 * the display of a safety decision: a dropped line is a line the user never reads,
 * and a truncated body is an approval granted on text nobody saw.
 */

import { describe, expect, test } from "bun:test";
import {
  APPROVE,
  approvalOf,
  commandProgram,
  formatRemaining,
  parseApprovalPrompt,
  remainingMs,
} from "./approval";

const BASH = "Allow tool: bash\nCommand: touch /tmp/probe-bash.txt";
const WRITE = "Allow tool: write\nPath: /tmp/probe-write.txt\nContent:\nhello probe";

describe("commandProgram", () => {
  test("names the executable, not the shell tool or its arguments", () => {
    expect(commandProgram('node check-index.js; echo "exit:$?"')).toBe("node");
    expect(commandProgram("/usr/bin/node check-index.js")).toBe("node");
    expect(commandProgram("'node' check-index.js")).toBe("node");
  });

  test("does not offer a program grant for ambiguous command prefixes", () => {
    for (const command of ["", "VAR=1 node index.js", "| node index.js", "$(node index.js)"]) {
      expect(commandProgram(command)).toBeNull();
    }
  });
});

describe("parseApprovalPrompt", () => {
  test("reads the bash approval the engine sent", () => {
    const prompt = parseApprovalPrompt(BASH);
    expect(prompt).not.toBeNull();
    expect(prompt?.tool).toBe("bash");
    expect(prompt?.fields).toEqual([{ label: "Command", value: "touch /tmp/probe-bash.txt" }]);
    expect(prompt?.blocks).toEqual([]);
    expect(prompt?.notes).toEqual([]);
  });

  test("reads the write approval, with its content as a body", () => {
    const prompt = parseApprovalPrompt(WRITE);
    expect(prompt?.tool).toBe("write");
    expect(prompt?.fields).toEqual([{ label: "Path", value: "/tmp/probe-write.txt" }]);
    expect(prompt?.blocks).toEqual([{ label: "Content", body: "hello probe" }]);
  });

  test("keeps the reason and origin lines the wrapper adds", () => {
    const prompt = parseApprovalPrompt(
      "Allow tool: mcp__server__tool\nOrigin: MCP server tool\nReason: Prompt required by bash pattern: rm -rf",
    );
    expect(prompt?.fields).toEqual([
      { label: "Origin", value: "MCP server tool" },
      { label: "Reason", value: "Prompt required by bash pattern: rm -rf" },
    ]);
  });

  test("a multi-line body keeps its lines, blank ones included", () => {
    const prompt = parseApprovalPrompt("Allow tool: eval\nLanguage: typescript\nCode:\nconst a = 1;\n\nawait a;");
    expect(prompt?.blocks).toEqual([{ label: "Code", body: "const a = 1;\n\nawait a;" }]);
    expect(prompt?.fields).toEqual([{ label: "Language", value: "typescript" }]);
  });

  test("a body is never truncated by a line that looks like a label", () => {
    // The safe direction: content that mentions "Path:" is content, and the user is
    // approving *that text*, so swallowing a later label beats cutting it short.
    const prompt = parseApprovalPrompt("Allow tool: write\nPath: /etc/x\nContent:\nPath: /etc/passwd\nReason: none");
    expect(prompt?.blocks).toEqual([{ label: "Content", body: "Path: /etc/passwd\nReason: none" }]);
    expect(prompt?.fields).toEqual([{ label: "Path", value: "/etc/x" }]);
  });

  test("an unlabelled line is kept as a note rather than dropped", () => {
    const prompt = parseApprovalPrompt(
      "Allow tool: ast-edit\nPattern: $A\nReplacement: $B\n+2 more ops",
    );
    expect(prompt?.notes).toEqual(["+2 more ops"]);
    expect(prompt?.fields).toEqual([
      { label: "Pattern", value: "$A" },
      { label: "Replacement", value: "$B" },
    ]);
  });

  test("a label the engine might add later is not silently lost", () => {
    const prompt = parseApprovalPrompt("Allow tool: bash\nWorking directory: /tmp\nCommand: ls");
    expect(prompt?.notes).toEqual(["Working directory: /tmp"]);
    expect(prompt?.fields).toEqual([{ label: "Command", value: "ls" }]);
  });

  test("a title that is not an approval parses to null", () => {
    for (const title of [
      "",
      "Pick a model",
      "Allow tool:",
      "allow tool: bash\nCommand: ls",
      "Allow tool: bash",
    ]) {
      if (title === "Allow tool: bash") {
        // The minimal real prompt: a tool and nothing else is still an approval.
        expect(parseApprovalPrompt(title)?.tool).toBe("bash");
        continue;
      }
      expect(parseApprovalPrompt(title)).toBeNull();
    }
  });

  test("a blank line between fields does not become a note", () => {
    const prompt = parseApprovalPrompt("Allow tool: bash\n\nCommand: ls");
    expect(prompt?.notes).toEqual([]);
    expect(prompt?.fields).toEqual([{ label: "Command", value: "ls" }]);
  });
});

describe("approvalOf", () => {
  const dialog = { kind: "select", title: BASH, options: [APPROVE, "Deny"] };

  test("recognizes the engine's approval pair", () => {
    expect(approvalOf(dialog)?.tool).toBe("bash");
  });

  test("anything that is not that pair is left to the generic dialog", () => {
    // An extension's own select, a confirm, and the pair in the wrong place.
    expect(approvalOf({ ...dialog, options: ["Fast", "Slow"] })).toBeNull();
    expect(approvalOf({ ...dialog, options: ["Deny", APPROVE] })).toBeNull();
    expect(approvalOf({ ...dialog, options: [APPROVE] })).toBeNull();
    expect(approvalOf({ ...dialog, kind: "confirm" })).toBeNull();
    expect(approvalOf({ kind: "select", title: "Allow tool: bash but no command", options: [APPROVE, "Deny"] })?.tool)
      .toBe("bash but no command");
  });
});

describe("remainingMs", () => {
  test("counts down from when the host saw the dialog, not from the duration", () => {
    // The engine sends a duration it armed before sending; without the anchor the
    // countdown would sit still.
    expect(remainingMs(60_000, 1_000, 1_000)).toBe(60_000);
    expect(remainingMs(60_000, 1_000, 11_000)).toBe(50_000);
  });

  test("stops at zero rather than going negative", () => {
    expect(remainingMs(5_000, 0, 9_999)).toBe(0);
  });

  test("no deadline, no countdown — which is every approval", () => {
    expect(remainingMs(null, 0, 1_000)).toBeNull();
  });
});

describe("formatRemaining", () => {
  test("seconds while the wait is short, minutes when it is long", () => {
    expect(formatRemaining(42_000)).toBe("resolves itself in 42s unless you answer");
    expect(formatRemaining(600_000)).toBe("resolves itself in 10m unless you answer");
  });
});
