export interface TooltipPosition {
  top: number;
  left: number;
  placement: "top" | "bottom";
}

export interface TooltipState {
  visible: boolean;
  text: string;
  top: number;
  left: number;
  placement: "top" | "bottom";
}

export function computeTooltipPosition(
  targetRect: { top: number; bottom: number; left: number; right: number; width: number; height: number },
  tooltipDimensions: { width: number; height: number },
  viewport: { width: number; height: number },
  gap = 6,
  margin = 8,
): TooltipPosition {
  const heightNeeded = tooltipDimensions.height + gap;
  const canFitBottom = targetRect.bottom + heightNeeded <= viewport.height - margin;
  const canFitTop = targetRect.top - heightNeeded >= margin;

  let placement: "top" | "bottom" = "bottom";
  let top = 0;

  if (canFitBottom) {
    placement = "bottom";
    top = targetRect.bottom + gap;
  } else if (canFitTop) {
    placement = "top";
    top = targetRect.top - gap - tooltipDimensions.height;
  } else {
    const spaceBelow = viewport.height - margin - targetRect.bottom;
    const spaceAbove = targetRect.top - margin;
    if (spaceAbove > spaceBelow) {
      placement = "top";
      top = Math.max(margin, targetRect.top - gap - tooltipDimensions.height);
    } else {
      placement = "bottom";
      top = Math.min(viewport.height - margin - tooltipDimensions.height, targetRect.bottom + gap);
    }
  }

  const rawLeft = targetRect.left + targetRect.width / 2 - tooltipDimensions.width / 2;
  const maxLeft = Math.max(margin, viewport.width - margin - tooltipDimensions.width);
  const left = Math.max(margin, Math.min(rawLeft, maxLeft));

  return { top, left, placement };
}

export function extractTooltip(startEl: Element | null): { text: string; target: HTMLElement } | null {
  let curr: Element | null = startEl;
  let depth = 0;
  while (curr && curr !== document.body && depth < 8) {
    if (curr instanceof HTMLElement) {
      const explicit = curr.getAttribute("data-tooltip")?.trim();
      if (explicit) {
        return { text: explicit, target: curr };
      }
      const rawTitle = curr.getAttribute("title")?.trim();
      if (rawTitle) {
        curr.setAttribute("data-tooltip", rawTitle);
        curr.removeAttribute("title");
        if (!curr.getAttribute("aria-label")) {
          curr.setAttribute("aria-label", rawTitle);
        }
        return { text: rawTitle, target: curr };
      }
    }
    curr = curr.parentElement;
    depth++;
  }
  return null;
}

export function sanitizeTitlesInSubtree(root: Element): void {
  const elements = root.querySelectorAll("[title]");
  for (let i = 0; i < elements.length; i++) {
    const el = elements[i];
    if (el instanceof HTMLElement) {
      const title = el.getAttribute("title")?.trim();
      if (title) {
        el.setAttribute("data-tooltip", title);
        el.removeAttribute("title");
        if (!el.getAttribute("aria-label")) {
          el.setAttribute("aria-label", title);
        }
      }
    }
  }
}

export function attachGlobalTooltipHandler(
  onUpdate: (state: TooltipState) => void,
  options?: { showDelay?: number; warmThreshold?: number; gap?: number; margin?: number },
): () => void {
  const showDelay = options?.showDelay ?? 220;
  const warmThreshold = options?.warmThreshold ?? 350;
  const gap = options?.gap ?? 6;
  const margin = options?.margin ?? 8;

  let currentTarget: HTMLElement | null = null;
  let currentText = "";
  let timer: ReturnType<typeof setTimeout> | null = null;
  let lastHideTime = 0;

  const hide = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (currentTarget) {
      lastHideTime = Date.now();
      currentTarget = null;
      currentText = "";
      onUpdate({ visible: false, text: "", top: 0, left: 0, placement: "bottom" });
    }
  };

  const show = (target: HTMLElement, text: string) => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    currentTarget = target;
    currentText = text;

    const isWarm = Date.now() - lastHideTime < warmThreshold;
    const delay = isWarm ? 0 : showDelay;

    const computeAndEmit = () => {
      if (!currentTarget || !document.body.contains(currentTarget)) {
        hide();
        return;
      }
      const rect = currentTarget.getBoundingClientRect();
      const approxCharWidth = 7;
      const approxWidth = Math.min(280, Math.max(40, text.length * approxCharWidth + 20));
      const approxHeight = text.length > 35 ? 36 : 24;

      const pos = computeTooltipPosition(
        rect,
        { width: approxWidth, height: approxHeight },
        { width: window.innerWidth, height: window.innerHeight },
        gap,
        margin,
      );

      onUpdate({
        visible: true,
        text: currentText,
        top: pos.top,
        left: pos.left,
        placement: pos.placement,
      });
    };

    if (delay === 0) {
      computeAndEmit();
    } else {
      timer = setTimeout(computeAndEmit, delay);
    }
  };

  const handlePointerOver = (e: PointerEvent) => {
    const result = extractTooltip(e.target as Element);
    if (!result) {
      if (currentTarget && !currentTarget.contains(e.target as Node)) {
        hide();
      }
      return;
    }
    if (result.target === currentTarget) return;
    show(result.target, result.text);
  };

  const handlePointerOut = (e: PointerEvent) => {
    if (!currentTarget) return;
    const related = e.relatedTarget as Node | null;
    if (related && currentTarget.contains(related)) return;
    hide();
  };

  const handlePointerDown = () => hide();
  const handleScroll = () => hide();
  const handleKeyDown = () => hide();

  window.addEventListener("pointerover", handlePointerOver, { passive: true });
  window.addEventListener("pointerout", handlePointerOut, { passive: true });
  window.addEventListener("pointerdown", handlePointerDown, { passive: true });
  window.addEventListener("scroll", handleScroll, { capture: true, passive: true });
  window.addEventListener("keydown", handleKeyDown, { passive: true });
  window.addEventListener("blur", hide, { passive: true });

  let observer: MutationObserver | null = null;
  if (typeof document !== "undefined" && document.body) {
    sanitizeTitlesInSubtree(document.body);
    if (typeof MutationObserver !== "undefined") {
      observer = new MutationObserver((mutations) => {
        for (const m of mutations) {
          for (let i = 0; i < m.addedNodes.length; i++) {
            const node = m.addedNodes[i];
            if (node instanceof HTMLElement) {
              if (node.hasAttribute("title")) {
                const title = node.getAttribute("title")?.trim();
                if (title) {
                  node.setAttribute("data-tooltip", title);
                  node.removeAttribute("title");
                  if (!node.getAttribute("aria-label")) {
                    node.setAttribute("aria-label", title);
                  }
                }
              }
              sanitizeTitlesInSubtree(node);
            }
          }
        }
      });
      observer.observe(document.body, { childList: true, subtree: true });
    }
  }

  return () => {
    if (timer) clearTimeout(timer);
    if (observer) observer.disconnect();
    window.removeEventListener("pointerover", handlePointerOver);
    window.removeEventListener("pointerout", handlePointerOut);
    window.removeEventListener("pointerdown", handlePointerDown);
    window.removeEventListener("scroll", handleScroll, true);
    window.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("blur", hide);
  };
}
