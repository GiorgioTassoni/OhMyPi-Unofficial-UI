import type { RowSnapshot } from "../bridge";

export interface ParsedSkillPrompt {
  isSkill: boolean;
  name: string;
  args: string | null;
}

/**
 * Identify whether a conversation row represents a skill prompt invocation.
 */
export function isSkillInvocation(row: RowSnapshot): boolean {
  if (row.role === "assistant" || row.role === "tool" || row.role === "toolResult") {
    return false;
  }
  return (
    row.customType === "skill-prompt" ||
    row.text.includes('User invoked the "') ||
    row.text.startsWith("[IMPORTANT: User invoked the") ||
    /^\/skill:[a-zA-Z0-9_-]+/i.test(row.text)
  );
}

/**
 * Extract the skill name from a skill invocation row text.
 */
export function skillInvocationName(text: string): string {
  const match = text.match(/User invoked the "([^"]+)" skill/i);
  if (match) return match[1];
  const tokenMatch = text.match(/\/skill:([a-zA-Z0-9_-]+)/);
  if (tokenMatch) return tokenMatch[1];
  return "skill";
}

/**
 * Extract user instructions / arguments passed alongside the skill invocation.
 */
export function skillInvocationArgs(text: string): string | null {
  // 1. Template format: "\nUser: <args>"
  const match = text.match(/\nUser:\s*([\s\S]+)$/);
  if (match && match[1].trim()) return match[1].trim();

  // 2. Prose format: "arguments: <args>"
  const argsMatch = text.match(/arguments:\s*"?([^"\n]+)"?/i);
  if (argsMatch && argsMatch[1].trim()) return argsMatch[1].trim();

  // 3. Raw slash command format: "/skill:name <args>"
  const tokenMatch = text.match(/\/skill:[a-zA-Z0-9_-]+\s+([^\n]+)/i);
  if (tokenMatch && tokenMatch[1].trim()) return tokenMatch[1].trim();

  return null;
}

/**
 * Parsed skill prompt view for conversation rendering.
 */
export function parseSkillPrompt(row: RowSnapshot): ParsedSkillPrompt {
  if (!isSkillInvocation(row)) {
    return { isSkill: false, name: "", args: null };
  }
  return {
    isSkill: true,
    name: skillInvocationName(row.text),
    args: skillInvocationArgs(row.text),
  };
}
