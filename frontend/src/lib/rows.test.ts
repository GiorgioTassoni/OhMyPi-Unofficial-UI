/**
 * Folding the host's conversation patches into the rows on screen (`docs/12` §3.4).
 *
 * The three states that matter are the ones a template cannot show: a patch that rewrites
 * the trailing row while a turn streams, a patch that starts at zero (a reset or a resumed
 * thread), and a patch that starts *beyond* what we hold — the one shape that cannot be
 * applied, because the host only ever publishes what changed and the missing rows are
 * already gone from the wire.
 */

import { said } from "./rows.fixture";

import { describe, expect, test } from "bun:test";
import { applyRowPatch } from "./rows";

describe("applying a conversation patch", () => {
  test("an append lands after what is already there", () => {
    const outcome = applyRowPatch([said("first")], { from: 1, rows: [said("second")] });

    expect(outcome.stale).toBe(false);
    expect(outcome.rows.map((entry) => entry.text)).toEqual(["first", "second"]);
  });

  test("a streaming rewrite replaces the trailing row instead of adding one", () => {
    const outcome = applyRowPatch([said("one"), said("two")], { from: 1, rows: [said("two and more")] });

    expect(outcome.rows.map((entry) => entry.text)).toEqual(["one", "two and more"]);
  });

  test("a patch from zero replaces everything", () => {
    const outcome = applyRowPatch([said("gone")], { from: 0, rows: [said("resumed")] });

    expect(outcome.rows.map((entry) => entry.text)).toEqual(["resumed"]);
  });

  test("an empty patch from the end changes nothing", () => {
    const rows = [said("only")];
    const outcome = applyRowPatch(rows, { from: 1, rows: [] });

    expect(outcome.rows).toEqual(rows);
    expect(outcome.stale).toBe(false);
  });

  test("a gap is reported rather than filled in with a wrong conversation", () => {
    const rows = [said("held")];
    const outcome = applyRowPatch(rows, { from: 5, rows: [said("later")] });

    expect(outcome.stale).toBe(true);
    // The rows are handed back untouched: the caller re-reads, and a truncated merge here
    // would show a conversation with a hole in it.
    expect(outcome.rows).toEqual(rows);
  });
});
