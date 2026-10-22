import { describe, expect, test } from "bun:test";
import type { RowSnapshot } from "../bridge";
import { isSkillInvocation, parseSkillPrompt, skillInvocationArgs, skillInvocationName } from "./skillPrompt";

function makeRow(customType: string | null, text: string, role = "custom"): RowSnapshot {
  return {
    role,
    customType,
    text,
    timestamp: 1000,
    thinking: null,
    streaming: false,
    tool: null,
    attachments: [],
    jobs: [],
    command: null,
  };
}

describe("skillPrompt helper", () => {
  test("identifies skill-prompt custom messages", () => {
    const row = makeRow("skill-prompt", '[IMPORTANT: User invoked the "impeccable" skill; follow its instructions. Full skill below.]\n\nUser: check ui');
    expect(isSkillInvocation(row)).toBe(true);
    expect(skillInvocationName(row.text)).toBe("impeccable");
    expect(skillInvocationArgs(row.text)).toBe("check ui");
  });

  test("parses skill name and args via parseSkillPrompt", () => {
    const row = makeRow("skill-prompt", '[IMPORTANT: User invoked the "designer" skill]\n---\nUser: redesign header');
    const parsed = parseSkillPrompt(row);
    expect(parsed.isSkill).toBe(true);
    expect(parsed.name).toBe("designer");
    expect(parsed.args).toBe("redesign header");
  });

  test("extracts arguments from slash command format", () => {
    const row = makeRow("skill-prompt", '/skill:code-review please review src/index.ts');
    const parsed = parseSkillPrompt(row);
    expect(parsed.isSkill).toBe(true);
    expect(parsed.name).toBe("code-review");
    expect(parsed.args).toBe("please review src/index.ts");
  });

  test("handles skill prompt without user arguments", () => {
    const row = makeRow("skill-prompt", '[IMPORTANT: User invoked the "clean" skill; follow its instructions.]');
    const parsed = parseSkillPrompt(row);
    expect(parsed.isSkill).toBe(true);
    expect(parsed.name).toBe("clean");
    expect(parsed.args).toBeNull();
  });

  test("returns not a skill for normal user or assistant messages", () => {
    const row = makeRow(null, "hello there", "user");
    expect(isSkillInvocation(row)).toBe(false);
    expect(parseSkillPrompt(row).isSkill).toBe(false);
  });
});
