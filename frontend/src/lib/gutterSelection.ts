export interface CaretPoint {
  node: Node;
  offset: number;
}

export function getCaretFromPoint(x: number, y: number, doc: Document = document): CaretPoint | null {
  const docWithCaret = doc as Document & {
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
    caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
  };
  if (typeof docWithCaret.caretRangeFromPoint === "function") {
    const range = docWithCaret.caretRangeFromPoint(x, y);
    if (range) {
      return { node: range.startContainer, offset: range.startOffset };
    }
  }
  if (typeof docWithCaret.caretPositionFromPoint === "function") {
    const pos = docWithCaret.caretPositionFromPoint(x, y);
    if (pos && pos.offsetNode) {
      return { node: pos.offsetNode, offset: pos.offset };
    }
  }
  return null;
}

export function extractTextNode(node: Node, fromStart = true): CaretPoint | null {
  if (node.nodeType === Node.TEXT_NODE) {
    const len = node.textContent?.length ?? 0;
    return { node, offset: fromStart ? 0 : len };
  }
  const textNodes: Text[] = [];
  const doc = node.ownerDocument ?? document;
  const filter = (doc.defaultView as unknown as { NodeFilter?: { SHOW_TEXT: number } })?.NodeFilter?.SHOW_TEXT ?? 4;
  const walker = doc.createTreeWalker(node, filter);
  let curr: Text | null;
  while ((curr = walker.nextNode() as Text | null)) {
    if (curr.textContent && curr.textContent.trim().length > 0) {
      textNodes.push(curr);
    }
  }
  if (textNodes.length === 0) return null;
  const target = fromStart ? textNodes[0] : textNodes[textNodes.length - 1];
  return { node: target, offset: fromStart ? 0 : (target.textContent?.length ?? 0) };
}

export function findTextCaretAt(
  probeX: number,
  probeY: number,
  container: HTMLElement,
  fromStart = true,
  doc: Document = document,
): CaretPoint | null {
  const deltas = [0, 8, -8, 16, -16, 24, -24, 32, -32];
  for (const dy of deltas) {
    const caret = getCaretFromPoint(probeX, probeY + dy, doc);
    if (caret && container.contains(caret.node)) {
      if (caret.node.nodeType === Node.TEXT_NODE) {
        return caret;
      }
      const resolved = extractTextNode(caret.node, fromStart);
      if (resolved) return resolved;
    }
  }

  const el = doc.elementFromPoint(probeX, probeY);
  if (el && container.contains(el)) {
    const resolved = extractTextNode(el, fromStart);
    if (resolved) return resolved;
  }
  return null;
}

export function attachGutterSelection(container: HTMLElement | null): () => void {
  if (!container || typeof window === "undefined") return () => {};

  let activeDragCleanup: (() => void) | null = null;

  function onMouseDown(e: MouseEvent): void {
    if (e.button !== 0 || e.defaultPrevented || e.shiftKey) return;
    const target = e.target as HTMLElement | null;
    if (!target || !container?.contains(target)) return;
    if (target.closest("button, a, input, textarea, select, [role='button'], summary, .select-none")) {
      return;
    }

    const doc = container.ownerDocument ?? document;
    const initialCaret = getCaretFromPoint(e.clientX, e.clientY, doc);
    const isDirectText =
      initialCaret &&
      initialCaret.node.nodeType === Node.TEXT_NODE &&
      initialCaret.node !== container &&
      container.contains(initialCaret.node);

    const col = (container.querySelector(".max-w-\\[46rem\\]") as HTMLElement | null) ?? container;
    const colRect = col.getBoundingClientRect();
    const fromStart = e.clientX <= (colRect.left + colRect.right) / 2;

    let probeX = e.clientX;
    if (probeX < colRect.left) probeX = colRect.left + 16;
    else if (probeX > colRect.right) probeX = colRect.right - 16;

    const startCaret = isDirectText ? initialCaret : findTextCaretAt(probeX, e.clientY, container, fromStart, doc);
    if (!startCaret) return;
    const initialAnchor = startCaret;

    const sel = doc.defaultView?.getSelection() ?? window.getSelection();
    if (!isDirectText) {
      e.preventDefault();
      if (!sel) return;
      try {
        sel.collapse(initialAnchor.node, initialAnchor.offset);
      } catch {
        return;
      }
    }

    if (activeDragCleanup) {
      activeDragCleanup();
      activeDragCleanup = null;
    }

    let lastScrollTime = 0;
    function autoScroll(clientY: number): void {
      if (!container) return;
      const now = performance.now();
      if (now - lastScrollTime < 16) return;
      lastScrollTime = now;
      const cRect = container.getBoundingClientRect();
      if (clientY < cRect.top + 30) {
        container.scrollTop -= 14;
      } else if (clientY > cRect.bottom - 30) {
        container.scrollTop += 14;
      }
    }

    function onMouseMove(moveEv: MouseEvent): void {
      if (!container) return;
      if (moveEv.buttons !== 1) {
        if (activeDragCleanup) {
          activeDragCleanup();
          activeDragCleanup = null;
        }
        return;
      }
      autoScroll(moveEv.clientY);

      const inGutter = moveEv.clientX < colRect.left || moveEv.clientX > colRect.right;
      if (!inGutter && isDirectText) {
        return;
      }

      let targetX = moveEv.clientX;
      if (targetX < colRect.left) targetX = colRect.left + 16;
      else if (targetX > colRect.right) targetX = colRect.right - 16;

      const currentSel = doc.defaultView?.getSelection() ?? window.getSelection();
      if (!currentSel) return;

      const anchorNode = currentSel.anchorNode && container.contains(currentSel.anchorNode) && currentSel.anchorNode !== container
        ? currentSel.anchorNode
        : initialAnchor.node;
      const anchorOffset = currentSel.anchorNode && container.contains(currentSel.anchorNode) && currentSel.anchorNode !== container
        ? currentSel.anchorOffset
        : initialAnchor.offset;

      const endCaret = findTextCaretAt(
        targetX,
        moveEv.clientY,
        container,
        moveEv.clientY < e.clientY,
        doc,
      );
      if (!endCaret) return;

      try {
        currentSel.setBaseAndExtent(
          anchorNode,
          anchorOffset,
          endCaret.node,
          endCaret.offset,
        );
      } catch {}
    }

    function onMouseUp(): void {
      if (activeDragCleanup) {
        activeDragCleanup();
        activeDragCleanup = null;
      }
    }

    window.addEventListener("mousemove", onMouseMove, { passive: true });
    window.addEventListener("mouseup", onMouseUp, { once: true });

    activeDragCleanup = () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }

  function onSelectionChange(): void {
    const doc = container?.ownerDocument ?? document;
    const sel = doc.defaultView?.getSelection() ?? window.getSelection();
    if (!sel || sel.isCollapsed) return;
    const anchorIsRoot = sel.anchorNode === container || sel.anchorNode === doc.body;
    const focusIsRoot = sel.focusNode === container || sel.focusNode === doc.body;
    if ((anchorIsRoot && sel.anchorOffset === 0) || (focusIsRoot && sel.focusOffset === 0)) {
      sel.removeAllRanges();
    }
  }

  container.addEventListener("mousedown", onMouseDown);
  document.addEventListener("selectionchange", onSelectionChange);

  return () => {
    container.removeEventListener("mousedown", onMouseDown);
    document.removeEventListener("selectionchange", onSelectionChange);
    if (activeDragCleanup) {
      activeDragCleanup();
      activeDragCleanup = null;
    }
  };
}
