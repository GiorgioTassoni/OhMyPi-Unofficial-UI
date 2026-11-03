export interface QuotePopupPosition {
  x: number;
  y: number;
  above: boolean;
  text: string;
}

export function computeQuotePopup(
  selection: Selection | null,
  container: HTMLElement | null,
): QuotePopupPosition | null {
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const text = selection.toString().trim();
  if (!text) return null;
  if (!container || !container.contains(selection.anchorNode) || !container.contains(selection.focusNode)) {
    return null;
  }
  const range = selection.getRangeAt(0);
  const rect = range.getBoundingClientRect();
  const boundary = container.closest("section") ?? container.parentElement ?? container;
  const parent = boundary.getBoundingClientRect();
  const above = rect.top - parent.top >= 44;
  return {
    x: Math.min(Math.max(rect.left + rect.width / 2 - parent.left, 70), Math.max(70, parent.width - 70)),
    y: above ? rect.top - parent.top - 8 : rect.bottom - parent.top + 8,
    above,
    text,
  };
}
