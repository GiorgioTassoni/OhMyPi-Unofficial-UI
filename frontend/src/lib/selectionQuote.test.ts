import { describe, expect, test } from "bun:test";
import { computeQuotePopup } from "./selectionQuote";

describe("computeQuotePopup", () => {
  test("returns null if selection is missing, collapsed, or empty", () => {
    const container = document.createElement("div");
    expect(computeQuotePopup(null, container)).toBeNull();

    const emptySel = { rangeCount: 0, isCollapsed: true, toString: () => "" } as unknown as Selection;
    expect(computeQuotePopup(emptySel, container)).toBeNull();

    const blankSel = {
      rangeCount: 1,
      isCollapsed: false,
      toString: () => "   \n\t  ",
      anchorNode: container,
      focusNode: container,
    } as unknown as Selection;
    expect(computeQuotePopup(blankSel, container)).toBeNull();
  });

  test("returns null if nodes are outside container", () => {
    const container = document.createElement("div");
    const outside = document.createElement("div");
    const sel = {
      rangeCount: 1,
      isCollapsed: false,
      toString: () => "hello",
      anchorNode: outside,
      focusNode: outside,
    } as unknown as Selection;

    expect(computeQuotePopup(sel, container)).toBeNull();
  });

  test("computes coordinates relative to container boundary", () => {
    const section = document.createElement("section");
    const container = document.createElement("div");
    const textNode = document.createTextNode("quoted text");
    container.appendChild(textNode);
    section.appendChild(container);
    document.body.appendChild(section);

    section.getBoundingClientRect = () => ({
      left: 100,
      top: 100,
      right: 900,
      bottom: 700,
      width: 800,
      height: 600,
      x: 100,
      y: 100,
      toJSON: () => {},
    });

    const mockRange = {
      getBoundingClientRect: () => ({
        left: 200,
        top: 250,
        right: 400,
        bottom: 270,
        width: 200,
        height: 20,
        x: 200,
        y: 250,
        toJSON: () => {},
      }),
    } as unknown as Range;

    const sel = {
      rangeCount: 1,
      isCollapsed: false,
      toString: () => "quoted text",
      anchorNode: textNode,
      focusNode: textNode,
      getRangeAt: () => mockRange,
    } as unknown as Selection;

    const popup = computeQuotePopup(sel, container);
    expect(popup).not.toBeNull();
    expect(popup?.text).toBe("quoted text");
    expect(popup?.above).toBe(true);
    expect(popup?.x).toBe(200);
    expect(popup?.y).toBe(142);

    document.body.removeChild(section);
  });
});
