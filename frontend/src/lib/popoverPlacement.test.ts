import { describe, expect, test } from "bun:test";
import { needsPlacement } from "./popoverPlacement";

function dispatch(target: EventTarget, type: string): Event {
  let seen: Event | null = null;
  const listener = (event: Event): void => {
    seen = event;
  };
  target.addEventListener(type, listener);
  target.dispatchEvent(new window.Event(type));
  target.removeEventListener(type, listener);
  if (seen === null) throw new Error(`${type} was not dispatched`);
  return seen;
}

describe("needsPlacement", () => {
  test("re-places for a scroll of the trigger's own scroller", () => {
    const scroller = document.createElement("div");
    const trigger = document.createElement("button");
    scroller.append(trigger);
    document.body.append(scroller);

    expect(needsPlacement(dispatch(scroller, "scroll"), null, trigger)).toBe(true);
    scroller.remove();
  });

  test("ignores scrolling inside the panel", () => {
    const panel = document.createElement("div");
    const row = document.createElement("button");
    panel.append(row);
    document.body.append(panel);

    expect(needsPlacement(dispatch(row, "scroll"), panel, null)).toBe(false);
    expect(needsPlacement(dispatch(panel, "scroll"), panel, null)).toBe(false);
    panel.remove();
  });

  test("ignores a scroll in a scroller that does not hold the trigger", () => {
    const elsewhere = document.createElement("div");
    const trigger = document.createElement("button");
    document.body.append(elsewhere, trigger);

    expect(needsPlacement(dispatch(elsewhere, "scroll"), null, trigger)).toBe(false);
    elsewhere.remove();
    trigger.remove();
  });

  test("re-places when the window resizes, whose target is not a Node", () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);

    expect(needsPlacement(dispatch(window, "resize"), null, trigger)).toBe(true);
    trigger.remove();
  });
});
