import { describe, expect, test } from "bun:test";
import { nativeDropIntent, type DropContext, type DropTargetRect } from "./dragDrop";

const chat: DropTargetRect = { left: 300, top: 100, right: 1100, bottom: 900 };

const chrome: DropContext = { active: true, target: chat, pixelsPerCss: 1 };
const elsewhere: DropContext = { ...chrome, active: false };

describe("nativeDropIntent", () => {
  test("a drop with no hover before it attaches, because the drop carries its own position", () => {
    const intent = nativeDropIntent(
      { type: "drop", position: { x: 700, y: 500 }, paths: ["/home/me/shot.png"] },
      chrome,
    );
    expect(intent.paths).toEqual(["/home/me/shot.png"]);
    expect(intent.hovering).toBe(false);
  });

  test("a drop in another panel is not the conversation's", () => {
    const intent = nativeDropIntent(
      { type: "drop", position: { x: 60, y: 500 }, paths: ["/home/me/shot.png"] },
      chrome,
    );
    expect(intent.paths).toEqual([]);
  });

  test("a thread that is not the active one ignores the drop entirely", () => {
    const intent = nativeDropIntent(
      { type: "drop", position: { x: 700, y: 500 }, paths: ["/home/me/shot.png"] },
      elsewhere,
    );
    expect(intent).toEqual({ hovering: false, paths: [] });
  });

  test("hover follows the pointer and is cleared by leave", () => {
    const over = { type: "over", position: { x: 700, y: 500 } } as const;
    const away = { type: "over", position: { x: 1200, y: 500 } } as const;
    expect(nativeDropIntent(over, chrome).hovering).toBe(true);
    expect(nativeDropIntent(away, chrome).hovering).toBe(false);
    expect(nativeDropIntent({ type: "leave" }, chrome).hovering).toBe(false);
  });

  test("a viewport-sized target does not restrict a drop", () => {
    const intent = nativeDropIntent(
      { type: "drop", position: { x: 3, y: 3 }, paths: ["/home/me/a.txt"] },
      { ...chrome, target: null },
    );
    expect(intent.paths).toEqual(["/home/me/a.txt"]);
  });

  test("device-pixel positions are converted to CSS pixels", () => {
    const event = { type: "drop", position: { x: 1900, y: 600 }, paths: ["/home/me/a.txt"] } as const;
    expect(nativeDropIntent(event, { ...chrome, pixelsPerCss: 2 }).paths).toEqual(["/home/me/a.txt"]);
    expect(nativeDropIntent(event, chrome).paths).toEqual([]);
  });
});
