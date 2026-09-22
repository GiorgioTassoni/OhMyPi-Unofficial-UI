/**
 * The engine's todo vocabulary, asserted (`docs/12` §8.1).
 *
 * The case that matters is the one this build has not seen: `status` is an open union, so an
 * unknown value has to reach the screen as itself. A panel that renders "unknown" hides a
 * real state behind a placeholder, which is worse than showing a word nobody recognises.
 */

import { describe, expect, test } from "bun:test";
import { phaseOpen, phaseProgress, todoLabel } from "./todos";

describe("the status vocabulary", () => {
  test("every status the engine emits today reads as its own word", () => {
    expect(["pending", "in_progress", "completed", "abandoned", "blocked"].map(todoLabel)).toEqual([
      "pending",
      "in progress",
      "done",
      "abandoned",
      "blocked",
    ]);
  });

  test("a status this build has not seen renders as itself", () => {
    // Not "unknown": an unseen state is still a state, and hiding it behind a placeholder is
    // how a real one goes unnoticed.
    expect(todoLabel("deferred")).toBe("deferred");
  });
});

describe("a phase", () => {
  test("counts the tasks the engine calls completed, and only those", () => {
    const tasks = [
      { status: "completed" },
      { status: "abandoned" },
      { status: "in_progress" },
    ];

    expect(phaseProgress(tasks)).toEqual({ done: 1, total: 3 });
  });

  test("stays open while it has work left, and folds when it is only a receipt", () => {
    expect(phaseOpen([{ status: "completed" }, { status: "pending" }])).toBe(true);
    expect(phaseOpen([{ status: "completed" }, { status: "abandoned" }])).toBe(false);
    // A phase the engine emptied is not worth opening either.
    expect(phaseOpen([])).toBe(false);
  });
});
