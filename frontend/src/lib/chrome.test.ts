/**
 * The chrome surfaces, asserted.
 *
 * The failure this guards against is the one the shapes invite: an extension running in one
 * session drawing its status line in another session's column, or a `set_editor_text` from a
 * background session landing in the box the user is typing in. Both are invisible until they
 * happen in a window with two threads open, which is exactly what a unit test can hold still.
 */

import { describe, expect, test } from "bun:test";
import { applyChrome, chromeFor, draftTarget, NO_CHROME } from "./chrome";

describe("status and widget", () => {
  test("a push lands on the thread that made it, and on no other", () => {
    const after = applyChrome({}, "a", { kind: "status", text: "indexing" });

    expect(chromeFor(after, "a").status).toBe("indexing");
    expect(chromeFor(after, "b")).toBe(NO_CHROME);
  });

  test("null clears the surface it names, and leaves the other alone", () => {
    const both = applyChrome(
      applyChrome({}, "a", { kind: "status", text: "indexing" }),
      "a",
      { kind: "widget", lines: ["one", "two"] },
    );
    const cleared = applyChrome(both, "a", { kind: "status", text: null });

    expect(chromeFor(cleared, "a").status).toBeNull();
    expect(chromeFor(cleared, "a").widget).toEqual(["one", "two"]);
  });

  test("a widget is replaced whole, not appended to", () => {
    const first = applyChrome({}, "a", { kind: "widget", lines: ["one"] });
    const second = applyChrome(first, "a", { kind: "widget", lines: ["two", "three"] });

    expect(chromeFor(second, "a").widget).toEqual(["two", "three"]);
  });

  test("the four ops that are not state change nothing", () => {
    const before = applyChrome({}, "a", { kind: "status", text: "indexing" });

    for (const op of [
      { kind: "notify", message: "done", level: null },
      { kind: "title", title: "Renamed" },
      { kind: "editor-text", text: "draft" },
      { kind: "open-url", url: "https://example.com" },
    ] as const) {
      expect(applyChrome(before, "a", op)).toBe(before);
    }
  });
});

describe("the editor-text rule", () => {
  test("only the thread on screen may be written", () => {
    expect(draftTarget("a", "a")).toBe(true);
    expect(draftTarget("a", "b")).toBe(false);
  });

  test("with nothing on screen there is no box to write", () => {
    expect(draftTarget(null, "a")).toBe(false);
  });
});
