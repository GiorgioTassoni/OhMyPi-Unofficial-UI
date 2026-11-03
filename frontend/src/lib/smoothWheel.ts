export function smoothWheel(
  el: HTMLElement,
  step = 64,
  tau = 55,
): () => void {
  const scheduleFrame =
    typeof requestAnimationFrame !== "undefined"
      ? requestAnimationFrame
      : (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 16) as unknown as number;
  const cancelFrame =
    typeof cancelAnimationFrame !== "undefined"
      ? cancelAnimationFrame
      : (id: number) => clearTimeout(id);

  const reduced =
    typeof matchMedia !== "undefined"
      ? matchMedia("(prefers-reduced-motion: reduce)")
      : { matches: false };
  let target = el.scrollTop;
  let expectedScroll = el.scrollTop;
  let raf = 0;
  let last = 0;

  const tick = (t: number): void => {
    const dt = last ? Math.min(50, t - last) : 16;
    last = t;
    const d = target - el.scrollTop;
    if (Math.abs(d) < 0.5) {
      el.scrollTop = target;
      expectedScroll = target;
      raf = 0;
      last = 0;
      return;
    }
    el.scrollTop += d * (reduced.matches ? 1 : 1 - Math.exp(-dt / tau));
    expectedScroll = el.scrollTop;
    raf = scheduleFrame(tick);
  };

  const onScroll = (): void => {
    if (!raf) {
      target = el.scrollTop;
      return;
    }
    if (Math.abs(el.scrollTop - expectedScroll) > 16) {
      cancelFrame(raf);
      raf = 0;
      last = 0;
      target = el.scrollTop;
    }
  };

  const onPointerDown = (): void => {
    if (raf) {
      cancelFrame(raf);
      raf = 0;
      last = 0;
      target = el.scrollTop;
    }
  };

  const onWheel = (e: WheelEvent): void => {
    if (e.ctrlKey || e.shiftKey) return;
    if (e.deltaMode === 0 && Math.abs(e.deltaY) < 100) return;
    const maxScroll = el.scrollHeight - el.clientHeight;
    if (maxScroll <= 0) return;
    for (let n = e.target as HTMLElement | null; n && n !== el; n = n.parentElement) {
      if (
        /(auto|scroll)/.test(getComputedStyle(n).overflowY) &&
        n.scrollHeight > n.clientHeight + 1
      ) {
        return;
      }
    }
    e.preventDefault();
    const base = raf ? target : el.scrollTop;
    target = Math.min(maxScroll, Math.max(0, base + Math.sign(e.deltaY) * step));
    if (!raf) {
      last = 0;
      expectedScroll = el.scrollTop;
      raf = scheduleFrame(tick);
    }
  };

  el.addEventListener("scroll", onScroll, { passive: true });
  el.addEventListener("pointerdown", onPointerDown, { passive: true });
  el.addEventListener("wheel", onWheel, { passive: false });

  return () => {
    el.removeEventListener("scroll", onScroll);
    el.removeEventListener("pointerdown", onPointerDown);
    el.removeEventListener("wheel", onWheel);
    if (raf) cancelFrame(raf);
  };
}
