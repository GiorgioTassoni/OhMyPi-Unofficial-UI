import { describe, expect, test } from "bun:test";
import { stripAnsi } from "./ansi";

describe("stripAnsi", () => {
  test("strips standard ANSI 256-color escape sequences", () => {
    const raw = "\x1b[38;5;243mO\x1b[39m 🐴 \x1b[38;5;248mponytail: \x1b[39m\x1b[39m⚡ FULL\x1b[39m";
    expect(stripAnsi(raw)).toBe("O 🐴 ponytail: ⚡ FULL");
  });

  test("strips replacement glyphs like ▯ preceding bracket codes", () => {
    const raw = "▯[38;5;243mO▯[39m 🐴 ▯[38;5;248mponytail: ▯[39m▯[39m⚡ FULL▯[39m";
    expect(stripAnsi(raw)).toBe("O 🐴 ponytail: ⚡ FULL");
  });

  test("handles normal strings without changes", () => {
    expect(stripAnsi("Ready")).toBe("Ready");
    expect(stripAnsi("")).toBe("");
  });
});
