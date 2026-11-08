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

export function isTextNodeNearY(textNode: Text, targetY: number, doc: Document = document): boolean {
  if (!textNode.textContent || textNode.textContent.trim().length === 0) return false;
  try {
    const range = doc.createRange();
    range.selectNodeContents(textNode);
    const rects = range.getClientRects();
    if (rects.length > 0) {
      for (let i = 0; i < rects.length; i++) {
        const r = rects[i];
        if (r.width > 0 && r.height > 0 && r.top - 8 <= targetY && r.bottom + 8 >= targetY) {
          return true;
        }
      }
      return false;
    }
    const bRect = range.getBoundingClientRect();
    if (bRect.width > 0 && bRect.height > 0) {
      return bRect.top - 12 <= targetY && bRect.bottom + 12 >= targetY;
    }
  } catch {}
  return true;
}

export function findTextNodeInElementNearY(
  element: Element,
  targetY: number,
  preferStart: boolean,
  doc: Document = document,
): CaretPoint | null {
  const filter = (doc.defaultView as unknown as { NodeFilter?: { SHOW_TEXT: number } })?.NodeFilter?.SHOW_TEXT ?? 4;
  const walker = doc.createTreeWalker(element, filter);
  let curr: Text | null;
  let bestNode: Text | null = null;
  let minDistance = Infinity;

  while ((curr = walker.nextNode() as Text | null)) {
    if (!curr.textContent || curr.textContent.trim().length === 0) continue;
    try {
      const range = doc.createRange();
      range.selectNodeContents(curr);
      const rects = range.getClientRects();
      let matched = false;
      if (rects.length > 0) {
        for (let i = 0; i < rects.length; i++) {
          const r = rects[i];
          if (r.width > 0 && r.height > 0) {
            if (r.top - 8 <= targetY && r.bottom + 8 >= targetY) {
              bestNode = curr;
              matched = true;
              break;
            }
            const dist = Math.min(Math.abs(r.top - targetY), Math.abs(r.bottom - targetY));
            if (dist < minDistance && dist <= 40) {
              minDistance = dist;
              bestNode = curr;
            }
          }
        }
      } else {
        const bRect = range.getBoundingClientRect();
        if (bRect.width > 0 && bRect.height > 0) {
          if (bRect.top - 12 <= targetY && bRect.bottom + 12 >= targetY) {
            bestNode = curr;
            matched = true;
          } else {
            const dist = Math.min(Math.abs(bRect.top - targetY), Math.abs(bRect.bottom - targetY));
            if (dist < minDistance && dist <= 40) {
              minDistance = dist;
              bestNode = curr;
            }
          }
        }
      }
      if (matched) break;
    } catch {}
  }

  if (!bestNode) {
    if (typeof (element as HTMLElement).getBoundingClientRect === "function") {
      const eRect = element.getBoundingClientRect();
      if (eRect.width === 0 && eRect.height === 0) {
        return extractTextNode(element, preferStart);
      }
    }
    return null;
  }
  return {
    node: bestNode,
    offset: preferStart ? 0 : (bestNode.textContent?.length ?? 0),
  };
}

export function findLineCaret(
  targetY: number,
  isLeftGutter: boolean,
  col: HTMLElement,
  container: HTMLElement,
  doc: Document = document,
): CaretPoint | null {
  const colRect = col.getBoundingClientRect();

  if (colRect.height > 0) {
    if (targetY < colRect.top) {
      return extractTextNode(col, true);
    }
    if (targetY > colRect.bottom) {
      return extractTextNode(col, false);
    }
  }

  const colCenter = (colRect.left + colRect.right) / 2;
  const probeXs = isLeftGutter
    ? [
        colRect.left + 24,
        colRect.left + 48,
        colRect.left + 80,
        colRect.left + 140,
        colRect.left + 220,
        colCenter,
        colRect.right - 80,
      ]
    : [
        colRect.right - 24,
        colRect.right - 48,
        colRect.right - 80,
        colRect.right - 140,
        colRect.right - 220,
        colCenter,
        colRect.left + 80,
      ];

  const yDeltas = [0, 6, -6, 12, -12, 18, -18, 24, -24];

  for (const dy of yDeltas) {
    const y = targetY + dy;
    for (const px of probeXs) {
      const caret = getCaretFromPoint(px, y, doc);
      if (caret && container.contains(caret.node)) {
        if (caret.node.nodeType === Node.TEXT_NODE) {
          const textNode = caret.node as Text;
          if (isTextNodeNearY(textNode, targetY, doc)) {
            return {
              node: textNode,
              offset: isLeftGutter ? 0 : (textNode.textContent?.length ?? 0),
            };
          }
        } else if (caret.node instanceof Element && container.contains(caret.node)) {
          const near = findTextNodeInElementNearY(caret.node, targetY, isLeftGutter, doc);
          if (near) return near;
        }
      }
    }
  }

  for (const dy of yDeltas) {
    const y = targetY + dy;
    for (const px of probeXs) {
      const el = doc.elementFromPoint(px, y);
      if (el && container.contains(el) && el !== container && el !== col) {
        const near = findTextNodeInElementNearY(el, targetY, isLeftGutter, doc);
        if (near) return near;
      }
    }
  }

  return findTextNodeInElementNearY(col, targetY, isLeftGutter, doc) ?? extractTextNode(col, isLeftGutter);
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
      const resolved = findTextNodeInElementNearY(caret.node as Element, probeY, fromStart, doc);
      if (resolved) return resolved;
    }
  }

  const el = doc.elementFromPoint(probeX, probeY);
  if (el && container.contains(el) && el !== container) {
    const resolved = findTextNodeInElementNearY(el, probeY, fromStart, doc);
    if (resolved) return resolved;
  }
  return null;
}

export function attachGutterSelection(container: HTMLElement | null): () => void {
  if (!container || typeof window === "undefined") return () => {};
  const root: HTMLElement = container;

  let activeDragCleanup: (() => void) | null = null;
  let lastValidSelection: { anchorNode: Node; anchorOffset: number; focusNode: Node; focusOffset: number } | null = null;
  let isClamping = false;

  const col = (root.querySelector(".max-w-\\[46rem\\]") as HTMLElement | null) ?? root;

  function isRootContainer(node: Node | null): boolean {
    if (!node) return false;
    const doc = root.ownerDocument ?? document;
    if (
      node === root ||
      node === col ||
      node === doc.body ||
      node === doc.documentElement ||
      node === doc
    ) {
      return true;
    }
    if (node instanceof HTMLElement) {
      if (node.hasAttribute("data-conversation") || node.classList.contains("max-w-[46rem]")) return true;
    }
    return false;
  }

  function onSelectStart(e: Event): void {
    let target = e.target as Node | null;
    if (!target) return;
    if (target.nodeType === Node.TEXT_NODE) {
      target = target.parentElement;
    }
    if (!(target instanceof Element)) {
      e.preventDefault();
      return;
    }
    const selectable = target.closest(
      ".select-text, textarea, input, .md, pre, code, kbd, samp, [data-dialog-panel], [data-widget]"
    );
    if (!selectable || !root.contains(selectable)) {
      e.preventDefault();
    }
  }

  function onMouseDown(e: MouseEvent): void {
    if (e.button !== 0 || e.defaultPrevented || e.shiftKey) return;
    const target = e.target as HTMLElement | null;
    if (!target || !root.contains(target)) return;
    if (target.closest("button, a, input, textarea, select, [role='button'], summary, .select-none")) {
      return;
    }

    const doc = root.ownerDocument ?? document;
    const initialCaret = getCaretFromPoint(e.clientX, e.clientY, doc);
    const isDirectText =
      initialCaret &&
      initialCaret.node.nodeType === Node.TEXT_NODE &&
      initialCaret.node !== root &&
      root.contains(initialCaret.node);

    const colRect = col.getBoundingClientRect();
    const isLeftGutter = e.clientX < colRect.left;

    const startCaret = isDirectText
      ? initialCaret
      : findLineCaret(e.clientY, isLeftGutter, col, root, doc) ??
        findTextCaretAt(e.clientX, e.clientY, root, isLeftGutter, doc);

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

    if (sel && sel.anchorNode && sel.focusNode) {
      lastValidSelection = {
        anchorNode: sel.anchorNode,
        anchorOffset: sel.anchorOffset,
        focusNode: sel.focusNode,
        focusOffset: sel.focusOffset,
      };
    } else {
      lastValidSelection = {
        anchorNode: initialAnchor.node,
        anchorOffset: initialAnchor.offset,
        focusNode: initialAnchor.node,
        focusOffset: initialAnchor.offset,
      };
    }

    if (activeDragCleanup) {
      activeDragCleanup();
      activeDragCleanup = null;
    }

    let lastScrollTime = 0;
    function autoScroll(clientY: number): void {
      const now = performance.now();
      if (now - lastScrollTime < 16) return;
      lastScrollTime = now;
      const cRect = root.getBoundingClientRect();
      if (clientY < cRect.top + 30) {
        root.scrollTop -= 14;
      } else if (clientY > cRect.bottom - 30) {
        root.scrollTop += 14;
      }
    }

    function onMouseMove(moveEv: MouseEvent): void {
      if (moveEv.buttons !== 1) {
        if (activeDragCleanup) {
          activeDragCleanup();
          activeDragCleanup = null;
        }
        return;
      }
      autoScroll(moveEv.clientY);

      const currentColRect = col.getBoundingClientRect();
      const currentIsLeft = moveEv.clientX < currentColRect.left;
      const currentIsRight = moveEv.clientX > currentColRect.right;
      const currentlyInGutter = currentIsLeft || currentIsRight;

      const currentSel = doc.defaultView?.getSelection() ?? window.getSelection();
      if (!currentSel) return;

      if (!currentlyInGutter) {
        if (
          currentSel.anchorNode &&
          currentSel.focusNode &&
          root.contains(currentSel.anchorNode) &&
          root.contains(currentSel.focusNode) &&
          !isRootContainer(currentSel.anchorNode) &&
          !isRootContainer(currentSel.focusNode)
        ) {
          lastValidSelection = {
            anchorNode: currentSel.anchorNode,
            anchorOffset: currentSel.anchorOffset,
            focusNode: currentSel.focusNode,
            focusOffset: currentSel.focusOffset,
          };
        }
        if (isDirectText) return;
      }

      const anchorNode =
        currentSel.anchorNode && root.contains(currentSel.anchorNode) && !isRootContainer(currentSel.anchorNode)
          ? currentSel.anchorNode
          : (lastValidSelection?.anchorNode ?? initialAnchor.node);
      const anchorOffset =
        currentSel.anchorNode && root.contains(currentSel.anchorNode) && !isRootContainer(currentSel.anchorNode)
          ? currentSel.anchorOffset
          : (lastValidSelection?.anchorOffset ?? initialAnchor.offset);

      const endCaret = currentlyInGutter
        ? (findLineCaret(moveEv.clientY, currentIsLeft, col, root, doc) ??
           (lastValidSelection ? { node: lastValidSelection.focusNode, offset: lastValidSelection.focusOffset } : null))
        : findTextCaretAt(moveEv.clientX, moveEv.clientY, root, moveEv.clientY < e.clientY, doc);

      if (!endCaret) return;

      try {
        isClamping = true;
        currentSel.setBaseAndExtent(anchorNode, anchorOffset, endCaret.node, endCaret.offset);
        lastValidSelection = {
          anchorNode,
          anchorOffset,
          focusNode: endCaret.node,
          focusOffset: endCaret.offset,
        };
      } catch {
      } finally {
        isClamping = false;
      }
    }

    function onMouseUp(): void {
      if (activeDragCleanup) {
        activeDragCleanup();
        activeDragCleanup = null;
      }
    }

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp, { once: true });

    activeDragCleanup = () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }

  function onSelectionChange(): void {
    if (isClamping) return;
    const doc = root.ownerDocument ?? document;
    const sel = doc.defaultView?.getSelection() ?? window.getSelection();
    if (!sel || sel.isCollapsed) return;

    const inContainer =
      (sel.anchorNode && root.contains(sel.anchorNode)) ||
      (sel.focusNode && root.contains(sel.focusNode));
    if (!inContainer) return;

    const anchorIsRoot = isRootContainer(sel.anchorNode);
    const focusIsRoot = isRootContainer(sel.focusNode);
    const focusEscaped = !sel.focusNode || !root.contains(sel.focusNode);
    const anchorEscaped = !sel.anchorNode || !root.contains(sel.anchorNode);

    if (anchorIsRoot || focusIsRoot || focusEscaped || anchorEscaped) {
      if (
        lastValidSelection &&
        root.contains(lastValidSelection.anchorNode) &&
        root.contains(lastValidSelection.focusNode) &&
        !isRootContainer(lastValidSelection.anchorNode) &&
        !isRootContainer(lastValidSelection.focusNode)
      ) {
        try {
          isClamping = true;
          sel.setBaseAndExtent(
            lastValidSelection.anchorNode,
            lastValidSelection.anchorOffset,
            lastValidSelection.focusNode,
            lastValidSelection.focusOffset,
          );
          return;
        } catch {
        } finally {
          isClamping = false;
        }
      }
      isClamping = true;
      try {
        sel.removeAllRanges();
      } finally {
        isClamping = false;
      }
    }
  }

  root.addEventListener("mousedown", onMouseDown);
  root.addEventListener("selectstart", onSelectStart);
  document.addEventListener("selectionchange", onSelectionChange);

  return () => {
    root.removeEventListener("mousedown", onMouseDown);
    root.removeEventListener("selectstart", onSelectStart);
    document.removeEventListener("selectionchange", onSelectionChange);
    if (activeDragCleanup) {
      activeDragCleanup();
      activeDragCleanup = null;
    }
  };
}
