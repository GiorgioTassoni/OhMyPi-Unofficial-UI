/**
 * The composer's keyboard map, asserted.
 *
 * Every case here is one a user will hit and a reviewer will not notice: a
 * `⇧Enter` that sends instead of breaking the line, an empty message that fires, or
 * an IME commit that ships half a word.
 */

import { describe, expect, test } from "bun:test";
import { operationFor } from "./composer";

const idle = { streaming: false, draft: "", attachments: 0, palette: false };
const streaming = { streaming: true, draft: "", attachments: 0, palette: false };
/** The palette is up: the draft is a command name being typed. */
const picking = { ...idle, draft: "/compact", palette: true };

describe("operationFor", () => {
  test("Enter sends a prompt when idle", () => {
    expect(operationFor({ key: "Enter" }, { ...idle, draft: "hello" })).toBe("prompt");
  });

  test("Enter steers while a turn is running", () => {
    expect(operationFor({ key: "Enter" }, { ...streaming, draft: "hello" })).toBe("steer");
  });

  test("Alt+Enter queues behind the running turn", () => {
    expect(operationFor({ key: "Enter", alt: true }, { ...streaming, draft: "hello" })).toBe(
      "follow-up",
    );
  });

  test("Alt+Enter while idle is still just a prompt", () => {
    // The modifier means "queue" only when there is something to queue behind.
    expect(operationFor({ key: "Enter", alt: true }, { ...idle, draft: "hello" })).toBe("prompt");
  });

  test("Shift+Enter breaks the line, and never sends", () => {
    for (const state of [idle, streaming, { ...streaming, draft: "hello" }]) {
      expect(operationFor({ key: "Enter", shift: true }, state)).toBe("newline");
    }
  });

  test("an empty or whitespace-only draft never sends", () => {
    for (const draft of ["", "   ", "\n", "\t\n "]) {
      expect(operationFor({ key: "Enter" }, { ...idle, draft })).toBe("none");
      expect(operationFor({ key: "Enter" }, { ...streaming, draft })).toBe("none");
    }
  });

  test("an attachment with no words is still a message", () => {
    // Measured at v18.2.6: `prompt` with an empty `message` and one image answers
    // `success` and starts a turn. A composer that refused to send it would make
    // "look at this" impossible to say with a screenshot.
    expect(operationFor({ key: "Enter" }, { ...idle, attachments: 1 })).toBe("prompt");
    expect(operationFor({ key: "Enter" }, { ...streaming, attachments: 1 })).toBe("steer");
    expect(operationFor({ key: "Enter", alt: true }, { ...streaming, attachments: 1 })).toBe(
      "follow-up",
    );
  });

  test("Escape clears attachments with the draft", () => {
    expect(operationFor({ key: "Escape" }, { ...idle, attachments: 2 })).toBe("clear");
  });

  test("the palette takes Enter, and nothing is sent while a name is being typed", () => {
    // The bug this rules out: dispatching `/compact` while the user is still choosing,
    // because the row was highlighted and Enter looked like a send.
    expect(operationFor({ key: "Enter" }, picking)).toBe("palette-accept");
    expect(operationFor({ key: "Enter" }, { ...streaming, ...picking })).toBe("palette-accept");
  });

  test("Tab completes without running anything", () => {
    expect(operationFor({ key: "Tab" }, picking)).toBe("palette-complete");
    // ⇧Tab belongs to the browser: leaving the field is legitimate.
    expect(operationFor({ key: "Tab", shift: true }, picking)).toBe("none");
  });

  test("the palette walks its rows", () => {
    expect(operationFor({ key: "ArrowDown" }, picking)).toBe("palette-down");
    expect(operationFor({ key: "ArrowUp" }, picking)).toBe("palette-up");
  });

  test("Escape dismisses the palette and keeps the text", () => {
    // Not `clear`: the draft is a half-typed command name the user wants back once the
    // list is out of the way.
    expect(operationFor({ key: "Escape" }, picking)).toBe("palette-close");
    expect(operationFor({ key: "Escape" }, { ...picking, streaming: true })).toBe(
      "palette-close",
    );
  });

  test("a modifier escapes the palette", () => {
    // `⌥Enter` still queues behind a running turn, and `⇧Enter` still breaks the line.
    expect(operationFor({ key: "Enter", alt: true }, { ...picking, streaming: true })).toBe(
      "follow-up",
    );
    expect(operationFor({ key: "Enter", shift: true }, picking)).toBe("newline");
  });

  test("typing anything else goes to the box", () => {
    for (const key of ["a", "Backspace", "1", "/", " "]) {
      expect(operationFor({ key }, picking)).toBe("none");
    }
  });

  test("an IME commit is never a send", () => {
    // The one that silently breaks input for everyone using an IME.
    expect(
      operationFor({ key: "Enter", composing: true }, { ...streaming, draft: "日本語" }),
    ).toBe("none");
    expect(operationFor({ key: "Enter", composing: true }, { ...idle, draft: "日本語" })).toBe(
      "none",
    );
  });

  test("Escape clears an idle draft", () => {
    expect(operationFor({ key: "Escape" }, { ...idle, draft: "half a thought" })).toBe("clear");
  });

  test("Escape with nothing to clear does nothing", () => {
    expect(operationFor({ key: "Escape" }, idle)).toBe("none");
  });

  test("Escape stops a running turn, draft or no draft", () => {
    // Stopping is the urgent reading while a turn runs, and the draft survives it.
    expect(operationFor({ key: "Escape" }, streaming)).toBe("abort");
    expect(operationFor({ key: "Escape" }, { ...streaming, draft: "text typed meanwhile" })).toBe(
      "abort",
    );
  });

  test("every other key passes through", () => {
    for (const key of ["a", "ArrowUp", "Tab", "Backspace", "Enterx"]) {
      expect(operationFor({ key }, { ...streaming, draft: "hello" })).toBe("none");
    }
  });
});
