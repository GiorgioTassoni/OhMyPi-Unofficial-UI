import { describe, expect, test } from "bun:test";
import { createDisclosureMemory } from "./disclosureMemory";

describe("disclosureMemory", () => {
  test("returns default when no state recorded", () => {
    const memory = createDisclosureMemory();
    expect(memory.get("turn", 1, true)).toBe(true);
    expect(memory.get("turn", 1, false)).toBe(false);
  });

  test("remembers set states for different kinds and rows", () => {
    const memory = createDisclosureMemory();
    memory.set("call", 5, true);
    memory.set("call", 6, false);
    memory.set("group", 5, false);

    expect(memory.get("call", 5, false)).toBe(true);
    expect(memory.get("call", 6, true)).toBe(false);
    expect(memory.get("group", 5, true)).toBe(false);
    expect(memory.get("call", 7, true)).toBe(true);
  });

  test("evicts oldest entry when exceeding maximum capacity", () => {
    const memory = createDisclosureMemory(3);
    memory.set("call", 1, true);
    memory.set("call", 2, true);
    memory.set("call", 3, true);

    expect(memory.get("call", 1, false)).toBe(true);

    memory.set("call", 4, true);

    expect(memory.get("call", 1, false)).toBe(false);
    expect(memory.get("call", 2, false)).toBe(true);
    expect(memory.get("call", 3, false)).toBe(true);
    expect(memory.get("call", 4, false)).toBe(true);
  });

  test("re-setting key promotes it and prevents early eviction", () => {
    const memory = createDisclosureMemory(3);
    memory.set("call", 1, true);
    memory.set("call", 2, true);
    memory.set("call", 3, true);

    memory.set("call", 1, false);
    memory.set("call", 4, true);

    expect(memory.get("call", 1, true)).toBe(false);
    expect(memory.get("call", 2, false)).toBe(false);
    expect(memory.get("call", 3, false)).toBe(true);
    expect(memory.get("call", 4, false)).toBe(true);
  });
});
