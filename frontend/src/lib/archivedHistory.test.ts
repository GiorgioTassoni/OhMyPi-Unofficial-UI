import { describe, expect, test } from "bun:test";
import { applyRowPatch } from "./rows";
import { row, said } from "./rows.fixture";
import type { RowSnapshot } from "../bridge";
import { conversationTurns } from "./turnActivity";

describe("archived history handling", () => {
  test("combining archived rows and active rows preserves chronological order", () => {
    // Suppose active context has Turn 3
    const activeRows: RowSnapshot[] = [
      row({ role: "user", text: "prompt 3" }),
      said("answer 3"),
    ];

    // User loads earlier batch: Turn 2
    const batch1: RowSnapshot[] = [
      row({ role: "user", text: "prompt 2" }),
      said("answer 2"),
    ];
    let archivedRows = [...batch1];
    let combined = [...archivedRows, ...activeRows];

    expect(combined.map((r) => r.text)).toEqual([
      "prompt 2",
      "answer 2",
      "prompt 3",
      "answer 3",
    ]);

    // User loads earlier batch: Turn 1
    const batch2: RowSnapshot[] = [
      row({ role: "user", text: "prompt 1" }),
      said("answer 1"),
    ];
    archivedRows = [...batch2, ...archivedRows];
    combined = [...archivedRows, ...activeRows];

    expect(combined.map((r) => r.text)).toEqual([
      "prompt 1",
      "answer 1",
      "prompt 2",
      "answer 2",
      "prompt 3",
      "answer 3",
    ]);

    // Verify turn parsing across all turns
    const turns = conversationTurns(combined, false);
    expect(turns.length).toBe(3);
    expect(turns[0].user?.row.text).toBe("prompt 1");
    expect(turns[1].user?.row.text).toBe("prompt 2");
    expect(turns[2].user?.row.text).toBe("prompt 3");
  });

  test("live patches apply to active rows without corrupting prepended archived rows", () => {
    const archivedRows: RowSnapshot[] = [
      row({ role: "user", text: "archived prompt" }),
      said("archived answer"),
    ];

    let activeRows: RowSnapshot[] = [
      row({ role: "user", text: "active prompt" }),
      said("streaming..."),
    ];

    // Host emits a patch updating the streaming assistant response in activeRows
    const patch = { from: 1, rows: [said("streaming complete answer")] };
    const outcome = applyRowPatch(activeRows, patch);

    expect(outcome.stale).toBe(false);
    activeRows = outcome.rows;

    const combined = [...archivedRows, ...activeRows];
    expect(combined.map((r) => r.text)).toEqual([
      "archived prompt",
      "archived answer",
      "active prompt",
      "streaming complete answer",
    ]);
  });

  test("measuredHeights keys shift correctly by prepended row count", () => {
    const measuredHeights = new Map<number, number>();
    measuredHeights.set(0, 150); // height for active turn starting at index 0

    const prependedRowCount = 4; // 4 rows prepended from earlier turns
    const shifted = new Map<number, number>();
    for (const [start, height] of measuredHeights) {
      shifted.set(start + prependedRowCount, height);
    }

    expect(shifted.has(0)).toBe(false);
    expect(shifted.get(4)).toBe(150);
  });
});
