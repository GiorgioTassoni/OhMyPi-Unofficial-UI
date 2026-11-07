import { describe, expect, test } from "bun:test";
import { row, said } from "./rows.fixture";
import { conversationTurns } from "./turnActivity";
import { turnContainingRow } from "./virtualTurns";

const turns = conversationTurns(
  Array.from({ length: 100 }, (_, turn) => [
    row({ role: "user", text: `Question ${turn}` }),
    said(`Answer ${turn}`),
  ]).flat(),
  false,
);

describe("virtualTurns", () => {
  test("locates turn containing a specific row index", () => {
    expect(turnContainingRow(turns, 0)).toBe(0);
    expect(turnContainingRow(turns, 1)).toBe(0);
    expect(turnContainingRow(turns, 153)).toBe(76);
    expect(turnContainingRow(turns, 999)).toBe(-1);
  });
});
