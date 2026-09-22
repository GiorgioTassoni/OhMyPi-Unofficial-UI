/**
 * The chips' text and geometry, asserted (`docs/12` §5.1, §7.1, §7.5, §7.6).
 *
 * The cases here are the ones a reviewer reads past: a model chip naming something
 * the session is not running, an effort level offered to a model that cannot take it,
 * a ring that draws an arc where there is no usage, a folder chip that is empty for
 * `/`. Each of those is one line in a template and invisible in review.
 */

import { describe, expect, test } from "bun:test";
import {
  APPROVAL_MODES,
  contextSummary,
  effortLabel,
  effortSupported,
  folderName,
  modeLabel,
  modeRow,
  modelChipLabel,
  percentLabel,
  ringDash,
} from "./chips";
import type { ModelOption } from "./models";

const fable: ModelOption = {
  provider: "openrouter",
  id: "~anthropic/claude-fable-latest",
  name: "Claude Fable Latest",
  contextWindow: 1_000_000,
  reasoning: true,
  defaultEffort: "high",
  efforts: ["low", "medium", "high", "xhigh", "max"],
};

const plain: ModelOption = {
  provider: "local",
  id: "tiny",
  name: "Tiny",
  contextWindow: 8_192,
  reasoning: false,
  defaultEffort: null,
  efforts: [],
};

describe("modelChipLabel", () => {
  test("prefers the engine's own display name", () => {
    expect(
      modelChipLabel({ provider: "openrouter", id: fable.id, name: "Claude Fable Latest" }, [fable]),
    ).toBe("Claude Fable Latest");
  });

  test("falls back to the catalogue when the session reports only ids", () => {
    // The state's model object is the full provider record; the name can be absent.
    expect(modelChipLabel({ provider: fable.provider, id: fable.id }, [fable])).toBe(
      "Claude Fable Latest",
    );
  });

  test("falls back to the id's tail when the catalogue has not been fetched", () => {
    expect(modelChipLabel({ provider: "openrouter", id: fable.id }, [])).toBe(
      "claude-fable-latest",
    );
  });

  test("says so rather than rendering an empty chip", () => {
    expect(modelChipLabel(null)).toBe("no model");
    expect(modelChipLabel({})).toBe("no model");
  });
});

describe("effortLabel", () => {
  test("names the level, or admits there is none", () => {
    expect(effortLabel("high")).toBe("Effort high");
    expect(effortLabel(null)).toBe("Effort —");
    expect(effortLabel("")).toBe("Effort —");
  });
});

describe("effortSupported", () => {
  test("off and auto are always available", () => {
    // Not thinking is not a capability, and `auto` is the engine's own classifier.
    expect(effortSupported("off", plain)).toBe(true);
    expect(effortSupported("auto", plain)).toBe(true);
  });

  test("a level the model does not list is refused, so the popover can say so", () => {
    expect(effortSupported("max", fable)).toBe(true);
    expect(effortSupported("minimal", fable)).toBe(false);
  });

  test("a model with no efforts at all supports none of the levels", () => {
    expect(effortSupported("low", plain)).toBe(false);
  });

  test("a reasoning model with empty efforts array supports all reasoning levels", () => {
    const customReasoning: ModelOption = {
      provider: "llama.cpp",
      id: "custom",
      name: "Custom",
      contextWindow: 128_000,
      reasoning: true,
      defaultEffort: null,
      efforts: [],
    };
    expect(effortSupported("low", customReasoning)).toBe(true);
    expect(effortSupported("medium", customReasoning)).toBe(true);
    expect(effortSupported("high", customReasoning)).toBe(true);
    expect(effortSupported("max", customReasoning)).toBe(true);
  });
});

describe("mode", () => {
  test("the ladder is the engine's three modes, in the documented order", () => {
    expect(APPROVAL_MODES.map((row) => row.mode)).toEqual(["always-ask", "write", "yolo"]);
  });

  test("only Full access is tinted", () => {
    expect(APPROVAL_MODES.filter((row) => row.warning).map((row) => row.mode)).toEqual(["yolo"]);
  });

  test("an unknown or absent mode is not silently shown as the default", () => {
    expect(modeRow("yolo")?.mode).toBe("yolo");
    expect(modeRow(null)).toBeNull();
    expect(modeLabel(null)).toBe("Mode —");
  });
});

describe("percentLabel", () => {
  test("one decimal below ten, whole numbers above", () => {
    expect(percentLabel(1.83)).toBe("1.8%");
    expect(percentLabel(42.4)).toBe("42%");
  });

  test("no usage is not zero percent", () => {
    expect(percentLabel(null)).toBe("—");
  });

  test("clamps rather than printing 103%", () => {
    expect(percentLabel(103)).toBe("100%");
    expect(percentLabel(-4)).toBe("0.0%");
  });
});

describe("ringDash", () => {
  test("no usage draws no arc", () => {
    const { dash, empty } = ringDash(0, 7);
    expect(empty).toBe(true);
    expect(dash.startsWith("0 ")).toBe(true);
  });

  test("half the window is half the circumference", () => {
    const { dash } = ringDash(50, 10);
    const [used, gap] = dash.split(" ").map(Number);
    expect(used).toBeCloseTo(Math.PI * 10, 1);
    expect(used + gap).toBeCloseTo(2 * Math.PI * 10, 1);
  });

  test("a full window is a closed ring, not an overflowing one", () => {
    const { dash } = ringDash(140, 7);
    const [used, gap] = dash.split(" ").map(Number);
    expect(used).toBeCloseTo(2 * Math.PI * 7, 1);
    expect(gap).toBeCloseTo(0, 1);
  });
});

describe("folderName", () => {
  test("the last segment, with or without the trailing separator", () => {
    expect(folderName("/home/me/Projects/OhMyPiApp")).toBe("OhMyPiApp");
    expect(folderName("/home/me/Projects/OhMyPiApp/")).toBe("OhMyPiApp");
  });

  test("the root and the empty path still say something", () => {
    // `/` has no last segment; an empty chip beside three others reads as a bug.
    expect(folderName("/")).toBe("/");
    expect(folderName("")).toBe("no folder");
  });

  test("a relative path is not treated as empty", () => {
    expect(folderName("src/components")).toBe("components");
  });
});

describe("contextSummary", () => {
  test("an empty session says so instead of claiming 0% of a window", () => {
    expect(contextSummary(0, 1_000_000)).toEqual({ text: "No usage yet", percent: null });
  });

  test("reports the pair and the fraction the bar draws", () => {
    expect(contextSummary(18_414, 1_000_000)).toEqual({
      text: "18,414 / 1,000,000 tokens",
      percent: 1.8414,
    });
  });
});
