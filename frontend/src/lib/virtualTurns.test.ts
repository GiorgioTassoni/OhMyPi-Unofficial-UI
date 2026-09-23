import { describe, expect, test } from "bun:test";
import { row, said } from "./rows.fixture";
import { conversationTurns } from "./turnActivity";
import { turnContainingRow, turnLayout, visibleTurns } from "./virtualTurns";

const turns = conversationTurns(Array.from({ length: 100 }, (_, turn) => [
  row({ role: "user", text: `Question ${turn}` }), said(`Answer ${turn}`),
]).flat(), false);

describe("virtual turn window", () => {
  test("renders only turns near the viewport while preserving total scroll height", () => {
    const measured = new Map(turns.map((turn) => [turn.start, 100]));
    const layout = turnLayout(turns, measured, () => false);
    const window = visibleTurns(layout, 5_000, 600, 200);
    expect(layout.total).toBe(10_000);
    expect(window).toEqual({ start: 48, end: 58, before: 4_800, after: 4_200 });
    expect(window.end - window.start).toBeLessThan(turns.length);
  });

  test("locates an off-screen search hit's turn and clamps the final viewport", () => {
    const layout = turnLayout(turns, new Map(), () => false);
    expect(turnContainingRow(turns, 153)).toBe(76);
    expect(turnContainingRow(turns, 999)).toBe(-1);
    const last = visibleTurns(layout, layout.total, 600);
    expect(last.end).toBe(turns.length);
    expect(last.start).toBeLessThan(turns.length);
  });
});
