import { describe, expect, test } from "bun:test";
import { parseFrontmatter } from "./frontmatter";

describe("parseFrontmatter", () => {
  test("returns hasFrontmatter: false when no frontmatter is present", () => {
    const text = "# Heading\n\nSome text without frontmatter.";
    const result = parseFrontmatter(text);
    expect(result.hasFrontmatter).toBe(false);
    expect(result.body).toBe(text);
    expect(Object.keys(result.attributes).length).toBe(0);
  });

  test("parses skill YAML frontmatter correctly", () => {
    const text = `---
name: impeccable
description: "Use when the user wants to design or polish UI."
metadata:
  version: 4.7.0
---

## Setup
1. Run context.
`;
    const result = parseFrontmatter(text);
    expect(result.hasFrontmatter).toBe(true);
    expect(result.attributes.name).toBe("impeccable");
    expect(result.attributes.description).toBe("Use when the user wants to design or polish UI.");
    expect(result.attributes.version).toBe("4.7.0");
    expect(result.body.trim()).toBe("## Setup\n1. Run context.");
  });

  test("handles empty string safely", () => {
    const result = parseFrontmatter("");
    expect(result.hasFrontmatter).toBe(false);
    expect(result.body).toBe("");
  });
});
