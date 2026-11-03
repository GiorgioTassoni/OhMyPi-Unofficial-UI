import { describe, expect, test } from "bun:test";
import { isHexColor, parseTextSegments } from "./colors";

describe("isHexColor", () => {
  test("identifies standard 6-digit hex colors", () => {
    expect(isHexColor("#5aa2f0")).toBe(true);
    expect(isHexColor("#ef6b70")).toBe(true);
    expect(isHexColor("#0c0d0f")).toBe(true);
    expect(isHexColor("#FFFFFF")).toBe(true);
    expect(isHexColor("#000000")).toBe(true);
  });

  test("identifies 8-digit hex colors with alpha", () => {
    expect(isHexColor("#5aa2f0ff")).toBe(true);
    expect(isHexColor("#00000080")).toBe(true);
  });

  test("identifies valid 3-digit shorthand colors", () => {
    expect(isHexColor("#fff")).toBe(true);
    expect(isHexColor("#000")).toBe(true);
    expect(isHexColor("#333")).toBe(true);
    expect(isHexColor("#f00")).toBe(true);
    expect(isHexColor("#abc")).toBe(true);
  });

  test("rejects ambiguous numeric 3-digit issue references", () => {
    expect(isHexColor("#123")).toBe(false);
    expect(isHexColor("#456")).toBe(false);
    expect(isHexColor("#789")).toBe(false);
  });

  test("rejects invalid lengths or non-hex characters", () => {
    expect(isHexColor("")).toBe(false);
    expect(isHexColor("5aa2f0")).toBe(false);
    expect(isHexColor("#12")).toBe(false);
    expect(isHexColor("#12345")).toBe(false);
    expect(isHexColor("#1234567")).toBe(false);
    expect(isHexColor("#heading")).toBe(false);
    expect(isHexColor("#zzz")).toBe(false);
  });
});

describe("parseTextSegments", () => {
  test("parses mixed text with hex color occurrences", () => {
    const text = 'Primary: #5aa2f0, alert: (#ef6b70), issue #123';
    const segments = parseTextSegments(text);

    expect(segments).toEqual([
      { text: "Primary: ", isColor: false },
      { text: "#5aa2f0", isColor: true },
      { text: ", alert: (", isColor: false },
      { text: "#ef6b70", isColor: true },
      { text: "), issue #123", isColor: false },
    ]);
  });

  test("returns single non-color segment when no valid colors exist", () => {
    const text = "Normal text without any colors or with #issue1";
    expect(parseTextSegments(text)).toEqual([{ text, isColor: false }]);
  });

  test("handles empty string safely", () => {
    expect(parseTextSegments("")).toEqual([{ text: "", isColor: false }]);
  });
});
