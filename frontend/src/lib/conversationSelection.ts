export function hasSelectionWithin(container: HTMLElement | null): boolean {
  if (!container) return false;
  const selection = container.ownerDocument.getSelection();
  if (!selection) return false;
  for (let i = 0; i < selection.rangeCount; i += 1) {
    const range = selection.getRangeAt(i);
    if (!range.collapsed && range.intersectsNode(container)) return true;
  }
  return false;
}

export function attachConversationSelection(container: HTMLElement | null): () => void {
  if (!container) return () => {};
  const root = container;
  const onMouseDown = (event: MouseEvent): void => {
    if (event.button !== 0) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.parentElement !== root || !target.hasAttribute("data-selection-surface")) return;
    event.preventDefault();
    root.ownerDocument.getSelection()?.removeAllRanges();
  };
  root.addEventListener("mousedown", onMouseDown, true);
  return () => root.removeEventListener("mousedown", onMouseDown, true);
}
