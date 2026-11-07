import { beforeEach, describe, expect, it } from "bun:test";
import {
  attachGlobalTooltipHandler,
  computeTooltipPosition,
  extractTooltip,
  sanitizeTitlesInSubtree,
  type TooltipState,
} from "./tooltip";

describe("tooltip logic", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  describe("computeTooltipPosition", () => {
    it("places tooltip below target when space is plentiful", () => {
      const target = { top: 100, bottom: 130, left: 200, right: 260, width: 60, height: 30 };
      const tip = { width: 100, height: 26 };
      const viewport = { width: 1000, height: 800 };

      const pos = computeTooltipPosition(target, tip, viewport, 6, 8);
      expect(pos.placement).toBe("bottom");
      expect(pos.top).toBe(136);
      expect(pos.left).toBe(180);
    });

    it("places tooltip above target when bottom edge has insufficient room", () => {
      const target = { top: 760, bottom: 790, left: 200, right: 260, width: 60, height: 30 };
      const tip = { width: 100, height: 26 };
      const viewport = { width: 1000, height: 800 };

      const pos = computeTooltipPosition(target, tip, viewport, 6, 8);
      expect(pos.placement).toBe("top");
      expect(pos.top).toBe(728);
      expect(pos.left).toBe(180);
    });

    it("clamps horizontal position to left margin when target is near left edge", () => {
      const target = { top: 100, bottom: 130, left: 5, right: 45, width: 40, height: 30 };
      const tip = { width: 120, height: 26 };
      const viewport = { width: 1000, height: 800 };

      const pos = computeTooltipPosition(target, tip, viewport, 6, 8);
      expect(pos.left).toBe(8);
    });

    it("clamps horizontal position to right margin when target is near right edge", () => {
      const target = { top: 100, bottom: 130, left: 960, right: 995, width: 35, height: 30 };
      const tip = { width: 120, height: 26 };
      const viewport = { width: 1000, height: 800 };

      const pos = computeTooltipPosition(target, tip, viewport, 6, 8);
      expect(pos.left).toBe(872);
    });
  });

  describe("extractTooltip and title suppression", () => {
    it("extracts explicit data-tooltip without modifying title", () => {
      const btn = document.createElement("button");
      btn.setAttribute("data-tooltip", "Action label");
      document.body.appendChild(btn);

      const res = extractTooltip(btn);
      expect(res).not.toBeNull();
      expect(res?.text).toBe("Action label");
      expect(res?.target).toBe(btn);
    });

    it("converts title to data-tooltip, strips title, and sets aria-label", () => {
      const btn = document.createElement("button");
      btn.setAttribute("title", "File viewer");
      document.body.appendChild(btn);

      const res = extractTooltip(btn);
      expect(res).not.toBeNull();
      expect(res?.text).toBe("File viewer");
      expect(btn.hasAttribute("title")).toBe(false);
      expect(btn.getAttribute("data-tooltip")).toBe("File viewer");
      expect(btn.getAttribute("aria-label")).toBe("File viewer");
    });

    it("finds tooltip on ancestor element when hovering child icon", () => {
      const btn = document.createElement("button");
      btn.setAttribute("title", "Close tab");
      const icon = document.createElement("span");
      btn.appendChild(icon);
      document.body.appendChild(btn);

      const res = extractTooltip(icon);
      expect(res).not.toBeNull();
      expect(res?.text).toBe("Close tab");
      expect(res?.target).toBe(btn);
      expect(btn.hasAttribute("title")).toBe(false);
    });

    it("returns null when neither target nor ancestors have tooltip metadata", () => {
      const div = document.createElement("div");
      document.body.appendChild(div);
      expect(extractTooltip(div)).toBeNull();
    });
  });

  describe("sanitizeTitlesInSubtree", () => {
    it("converts all title attributes in subtree to data-tooltip", () => {
      const container = document.createElement("div");
      container.innerHTML = `
        <button id="b1" title="One">B1</button>
        <span id="s1" title="Two">S1</span>
        <div id="d1">No title</div>
      `;
      document.body.appendChild(container);

      sanitizeTitlesInSubtree(container);

      const b1 = document.getElementById("b1")!;
      const s1 = document.getElementById("s1")!;
      const d1 = document.getElementById("d1")!;

      expect(b1.hasAttribute("title")).toBe(false);
      expect(b1.getAttribute("data-tooltip")).toBe("One");
      expect(b1.getAttribute("aria-label")).toBe("One");

      expect(s1.hasAttribute("title")).toBe(false);
      expect(s1.getAttribute("data-tooltip")).toBe("Two");

      expect(d1.hasAttribute("data-tooltip")).toBe(false);
    });
  });

  describe("attachGlobalTooltipHandler", () => {
    it("invokes onUpdate with visible true when hovering over a tooltip element", async () => {
      const btn = document.createElement("button");
      btn.setAttribute("data-tooltip", "Quick action");
      btn.getBoundingClientRect = () => ({
        top: 50,
        bottom: 80,
        left: 100,
        right: 160,
        width: 60,
        height: 30,
        x: 100,
        y: 50,
        toJSON: () => {},
      });
      document.body.appendChild(btn);

      const holder: { current: TooltipState | null } = { current: null };
      const cleanup = attachGlobalTooltipHandler((state) => {
        holder.current = state;
      }, { showDelay: 10 });

      const event = new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("pointerover", { bubbles: true });
      btn.dispatchEvent(event);

      await new Promise((resolve) => setTimeout(resolve, 30));

      expect(holder.current).not.toBeNull();
      expect(holder.current?.visible).toBe(true);
      expect(holder.current?.text).toBe("Quick action");

      const downEvent = new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("pointerdown", { bubbles: true });
      window.dispatchEvent(downEvent);

      expect(holder.current?.visible).toBe(false);

      cleanup();
    });
  });
});
