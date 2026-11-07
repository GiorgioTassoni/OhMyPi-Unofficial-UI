import { describe, expect, it } from "bun:test";
import {
  attachGutterSelection,
  extractTextNode,
  findTextCaretAt,
  getCaretFromPoint,
} from "./gutterSelection";

describe("gutterSelection", () => {
  it("extracts text node from a direct text node", () => {
    const textNode = document.createTextNode("Hello world");
    const result = extractTextNode(textNode, true);
    expect(result).not.toBeNull();
    expect(result?.node).toBe(textNode);
    expect(result?.offset).toBe(0);

    const endResult = extractTextNode(textNode, false);
    expect(endResult?.offset).toBe(11);
  });

  it("extracts text node from an element with nested children", () => {
    const div = document.createElement("div");
    const p1 = document.createElement("p");
    p1.textContent = "First line";
    const p2 = document.createElement("p");
    p2.textContent = "Second line";
    div.appendChild(p1);
    div.appendChild(p2);

    const first = extractTextNode(div, true);
    expect(first?.node.textContent).toBe("First line");
    expect(first?.offset).toBe(0);

    const last = extractTextNode(div, false);
    expect(last?.node.textContent).toBe("Second line");
    expect(last?.offset).toBe("Second line".length);
  });

  it("returns null for empty element without text", () => {
    const div = document.createElement("div");
    expect(extractTextNode(div)).toBeNull();
  });

  it("getCaretFromPoint handles caretRangeFromPoint", () => {
    const p = document.createElement("p");
    p.textContent = "Caret test";
    const range = document.createRange();
    range.setStart(p.firstChild!, 2);

    const fakeDoc = {
      caretRangeFromPoint: () => range,
    } as unknown as Document;

    const res = getCaretFromPoint(10, 20, fakeDoc);
    expect(res).not.toBeNull();
    expect(res?.node).toBe(p.firstChild!);
    expect(res?.offset).toBe(2);
  });

  it("getCaretFromPoint handles caretPositionFromPoint fallback", () => {
    const text = document.createTextNode("Position test");
    const fakeDoc = {
      caretPositionFromPoint: () => ({
        offsetNode: text,
        offset: 5,
      }),
    } as unknown as Document;

    const res = getCaretFromPoint(10, 20, fakeDoc);
    expect(res?.node).toBe(text);
    expect(res?.offset).toBe(5);
  });

  it("findTextCaretAt finds text node inside container", () => {
    const container = document.createElement("div");
    const p = document.createElement("p");
    const text = document.createTextNode("Sample message text");
    p.appendChild(text);
    container.appendChild(p);

    const range = document.createRange();
    range.setStart(text, 0);

    const fakeDoc = {
      caretRangeFromPoint: () => range,
      elementFromPoint: () => p,
    } as unknown as Document;

    const found = findTextCaretAt(50, 100, container, true, fakeDoc);
    expect(found?.node).toBe(text);
  });

  it("attachGutterSelection attaches and removes listeners", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);

    const cleanup = attachGutterSelection(container);
    expect(typeof cleanup).toBe("function");

    cleanup();
    document.body.removeChild(container);
  });

  it("clears selection when anchorNode or focusNode is root container at offset 0", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);

    const cleanup = attachGutterSelection(container);

    const sel = window.getSelection();
    if (sel) {
      const range = document.createRange();
      range.setStart(container, 0);
      const text = document.createTextNode("some text");
      container.appendChild(text);
      range.setEnd(text, 4);
      sel.removeAllRanges();
      sel.addRange(range);

      const eventConstructor = document.defaultView?.Event ?? Event;
      document.dispatchEvent(new eventConstructor("selectionchange"));
      expect(sel.rangeCount).toBe(0);
    }

    cleanup();
    document.body.removeChild(container);
  });
});
