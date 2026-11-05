import { describe, expect, test, beforeEach } from "bun:test";
import {
  readDesignChoiceEnabled,
  saveDesignChoiceEnabled,
} from "./designChoiceSetting";

describe("mcp design choice setting", () => {
  const store = new Map<string, string>();
  const mockStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => store.set(k, v),
    removeItem: (k: string) => store.delete(k),
    clear: () => store.clear(),
  };

  beforeEach(() => {
    store.clear();
    (globalThis as unknown as { localStorage: typeof mockStorage }).localStorage = mockStorage;
  });

  test("mcp design choice is enabled by default when unset", () => {
    expect(readDesignChoiceEnabled()).toBe(true);
  });

  test("saving disabled state persists and reads false", () => {
    saveDesignChoiceEnabled(false);
    expect(readDesignChoiceEnabled()).toBe(false);
  });

  test("saving enabled state persists and reads true", () => {
    saveDesignChoiceEnabled(true);
    expect(readDesignChoiceEnabled()).toBe(true);
  });
});
