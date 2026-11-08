import { describe, expect, it } from "bun:test";
import {
  attachGutterSelection,
  extractTextNode,
  findLineCaret,
  findTextCaretAt,
  findTextNodeInElementNearY,
  getCaretFromPoint,
  isTextNodeNearY,
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

  it("isTextNodeNearY returns false for empty text nodes", () => {
    const emptyNode = document.createTextNode("   ");
    expect(isTextNodeNearY(emptyNode, 100)).toBe(false);
  });

  it("isTextNodeNearY handles text node with mock rects", () => {
    const text = document.createTextNode("Sample line");
    const doc = document.implementation.createHTMLDocument();
    doc.body.appendChild(text);

    const range = doc.createRange();
    range.selectNodeContents(text);
    range.getClientRects = () => [
      {
        top: 90,
        bottom: 110,
        left: 20,
        right: 150,
        width: 130,
        height: 20,
        x: 20,
        y: 90,
        toJSON: () => {},
      },
    ] as unknown as DOMRectList;

    const origCreateRange = doc.createRange.bind(doc);
    doc.createRange = () => range;

    expect(isTextNodeNearY(text, 100, doc)).toBe(true);
    expect(isTextNodeNearY(text, 50, doc)).toBe(false);
    expect(isTextNodeNearY(text, 150, doc)).toBe(false);

    doc.createRange = origCreateRange;
  });

  it("findTextNodeInElementNearY finds matching child text node near coordinate", () => {
    const div = document.createElement("div");
    const p1 = document.createElement("p");
    p1.textContent = "Line one";
    const p2 = document.createElement("p");
    p2.textContent = "Line two";
    div.appendChild(p1);
    div.appendChild(p2);

    const t1 = p1.firstChild as Text;
    const t2 = p2.firstChild as Text;

    const doc = document.implementation.createHTMLDocument();
    doc.body.appendChild(div);

    const origCreateRange = doc.createRange.bind(doc);
    doc.createRange = () => {
      const r = origCreateRange();
      const origSelect = r.selectNodeContents.bind(r);
      r.selectNodeContents = (node: Node) => {
        origSelect(node);
        if (node === t1) {
          r.getClientRects = () => [
            { top: 50, bottom: 70, left: 10, right: 100, width: 90, height: 20, x: 10, y: 50, toJSON: () => {} },
          ] as unknown as DOMRectList;
        } else if (node === t2) {
          r.getClientRects = () => [
            { top: 100, bottom: 120, left: 10, right: 100, width: 90, height: 20, x: 10, y: 100, toJSON: () => {} },
          ] as unknown as DOMRectList;
        }
      };
      return r;
    };

    const foundT2 = findTextNodeInElementNearY(div, 105, true, doc);
    expect(foundT2).not.toBeNull();
    expect(foundT2?.node).toBe(t2);
    expect(foundT2?.offset).toBe(0);

    const foundT1 = findTextNodeInElementNearY(div, 60, false, doc);
    expect(foundT1).not.toBeNull();
    expect(foundT1?.node).toBe(t1);
    expect(foundT1?.offset).toBe("Line one".length);

    doc.createRange = origCreateRange;
  });

  it("findLineCaret clamps to top when targetY is above colRect", () => {
    const container = document.createElement("div");
    const col = document.createElement("div");
    const p = document.createElement("p");
    p.textContent = "First row";
    col.appendChild(p);
    container.appendChild(col);

    col.getBoundingClientRect = () => ({
      top: 100,
      bottom: 500,
      left: 200,
      right: 800,
      width: 600,
      height: 400,
      x: 200,
      y: 100,
      toJSON: () => {},
    });

    const res = findLineCaret(50, true, col, container, document);
    expect(res).not.toBeNull();
    expect(res?.node.textContent).toBe("First row");
    expect(res?.offset).toBe(0);
  });

  it("findLineCaret clamps to bottom when targetY is below colRect", () => {
    const container = document.createElement("div");
    const col = document.createElement("div");
    const p = document.createElement("p");
    p.textContent = "Last row";
    col.appendChild(p);
    container.appendChild(col);

    col.getBoundingClientRect = () => ({
      top: 100,
      bottom: 500,
      left: 200,
      right: 800,
      width: 600,
      height: 400,
      x: 200,
      y: 100,
      toJSON: () => {},
    });

    const res = findLineCaret(600, false, col, container, document);
    expect(res).not.toBeNull();
    expect(res?.node.textContent).toBe("Last row");
    expect(res?.offset).toBe("Last row".length);
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

  it("clears selection when anchorNode or focusNode is root container at offset > 0", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);

    const cleanup = attachGutterSelection(container);

    const sel = window.getSelection();
    if (sel) {
      const text = document.createTextNode("some text");
      container.appendChild(text);
      const range = document.createRange();
      range.setStart(text, 0);
      range.setEnd(container, 1);
      sel.removeAllRanges();
      sel.addRange(range);

      const eventConstructor = document.defaultView?.Event ?? Event;
      document.dispatchEvent(new eventConstructor("selectionchange"));
      expect(sel.rangeCount).toBe(0);
    }

    cleanup();
    document.body.removeChild(container);
  });

  it("clears selection when focusNode escapes outside container", () => {
    const container = document.createElement("div");
    const outside = document.createElement("div");
    document.body.appendChild(container);
    document.body.appendChild(outside);

    const cleanup = attachGutterSelection(container);

    const sel = window.getSelection();
    if (sel) {
      const textInside = document.createTextNode("inside");
      const textOutside = document.createTextNode("outside");
      container.appendChild(textInside);
      outside.appendChild(textOutside);

      const range = document.createRange();
      range.setStart(textInside, 0);
      range.setEnd(textOutside, 3);
      sel.removeAllRanges();
      sel.addRange(range);

      const eventConstructor = document.defaultView?.Event ?? Event;
      document.dispatchEvent(new eventConstructor("selectionchange"));
      expect(sel.rangeCount).toBe(0);
    }

    cleanup();
    document.body.removeChild(container);
    document.body.removeChild(outside);
  });

  it("prevents default on selectstart when target is not selectable", () => {
    const container = document.createElement("div");
    container.setAttribute("data-conversation", "true");
    document.body.appendChild(container);

    const cleanup = attachGutterSelection(container);

    const EventCtor = document.defaultView?.Event ?? Event;
    let defaultPrevented = false;
    const event = new EventCtor("selectstart", { bubbles: true, cancelable: true });
    event.preventDefault = () => {
      defaultPrevented = true;
    };

    container.dispatchEvent(event);
    expect(defaultPrevented).toBe(true);

    cleanup();
    document.body.removeChild(container);
  });

  it("allows selectstart when target is inside .select-text", () => {
    const container = document.createElement("div");
    container.setAttribute("data-conversation", "true");
    const content = document.createElement("div");
    content.className = "select-text";
    const p = document.createElement("p");
    p.textContent = "allowed";
    content.appendChild(p);
    container.appendChild(content);
    document.body.appendChild(container);

    const cleanup = attachGutterSelection(container);

    const EventCtor = document.defaultView?.Event ?? Event;
    let defaultPrevented = false;
    const event = new EventCtor("selectstart", { bubbles: true, cancelable: true });
    event.preventDefault = () => {
      defaultPrevented = true;
    };

    p.dispatchEvent(event);
    expect(defaultPrevented).toBe(false);

    cleanup();
    document.body.removeChild(container);
  });
});
