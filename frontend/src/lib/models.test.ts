/**
 * The picker's rules, asserted (`docs/12` §7.1).
 *
 * These are the cases a 601-row list hides: a favourite that disappears because
 * its provider was dropped upstream, a search that reorders the providers, a
 * `⌥↓` on the last row that wraps a model to the top. Each test names the rule it
 * would catch being broken.
 */

import { describe, expect, test } from "bun:test";
import {
  type ModelOption,
  modelKey,
  moveFavourite,
  parseKey,
  providerGlyph,
  toggleFavourite,
  visibleGroups,
} from "./models";

/** The shape `get_available_models` actually returns, reduced to what we model. */
function model(provider: string, id: string, name = id, defaultEffort: string | null = null): ModelOption {
  return {
    provider,
    id,
    name,
    contextWindow: 200_000,
    reasoning: defaultEffort !== null,
    defaultEffort,
    efforts: defaultEffort === null ? [] : ["low", "medium", "high"],
  };
}

// The measured row: a gateway provider whose model id carries its own slash.
const fable = model("openrouter", "~anthropic/claude-fable-latest", "Claude Fable Latest", "high");
const sonnet = model("openrouter", "anthropic/claude-sonnet-4", "Claude Sonnet 4");
const gpt = model("openai", "gpt-5", "GPT-5");
const gemini = model("google", "gemini-3-pro", "Gemini 3 Pro");

const catalogue = [fable, sonnet, gpt, gemini];

describe("modelKey", () => {
  test("is the provider and the id, so a slash inside the id survives", () => {
    expect(modelKey(fable)).toBe("openrouter/~anthropic/claude-fable-latest");
  });
});

describe("modelKey", () => {
  test("tolerates the half a session can report", () => {
    // `get_state.model` may name a provider without an id; the chip still needs a
    // stable key to compare, and a partial key simply never matches a catalogue row.
    expect(modelKey({ id: "y" })).toBe("/y");
    expect(modelKey({})).toBe("/");
  });
});

describe("parseKey", () => {
  test("splits on the first slash, keeping the rest as the id", () => {
    expect(parseKey("openrouter/~anthropic/claude-fable-latest")).toEqual({
      provider: "openrouter",
      modelId: "~anthropic/claude-fable-latest",
    });
  });

  test("round-trips every row's key", () => {
    for (const row of catalogue) {
      expect(parseKey(modelKey(row))).toEqual({ provider: row.provider, modelId: row.id });
    }
  });

  test("rejects keys with no model, so a lookup cannot select a half-key", () => {
    for (const key of ["", "openrouter", "/gpt-5", "openrouter/", "/"]) {
      expect(parseKey(key)).toBeNull();
    }
  });
});

describe("visibleGroups", () => {
  test("without a query, groups providers in the catalogue's order", () => {
    // Notably openrouter first, not alphabetically: the engine's order is the
    // order the rows are already in, and any other rule moves groups on a keystroke.
    const { groups } = visibleGroups([gpt, gemini, sonnet], "", []);
    expect(groups.map((group) => group.provider)).toEqual(["openai", "google", "openrouter"]);
  });

  test("without a query, keeps the engine's row order inside a group", () => {
    const { groups } = visibleGroups(catalogue, "", []);
    expect(groups.find((group) => group.provider === "openrouter")?.models).toEqual([
      fable,
      sonnet,
    ]);
  });

  test("renders favourites first, in the order they were stored", () => {
    // Stored order is the opposite of catalogue order, and must win.
    const { favourites } = visibleGroups(catalogue, "", [modelKey(gpt), modelKey(fable)]);
    expect(favourites.map(modelKey)).toEqual([modelKey(gpt), modelKey(fable)]);
  });

  test("skips a stored key the catalogue does not have", () => {
    // A provider dropping a model must not leave a blank row in the group.
    const { favourites } = visibleGroups(catalogue, "", [
      "openrouter/retired-model",
      modelKey(sonnet),
    ]);
    expect(favourites.map(modelKey)).toEqual([modelKey(sonnet)]);
  });

  test("renders a stored key listed twice once", () => {
    const { favourites } = visibleGroups(catalogue, "", [modelKey(gpt), modelKey(gpt)]);
    expect(favourites.map(modelKey)).toEqual([modelKey(gpt)]);
  });

  test("still lists a favourite in its provider group", () => {
    // Otherwise starring a model would remove it from its own provider, and a
    // search for it (which hides the Favourites group) would find nothing.
    const { groups } = visibleGroups(catalogue, "", [modelKey(fable)]);
    expect(groups.find((group) => group.provider === "openrouter")?.models).toEqual([
      fable,
      sonnet,
    ]);
  });

  test("a query hides the Favourites group but keeps favourites searchable", () => {
    const { favourites, groups } = visibleGroups(catalogue, "fable", [modelKey(fable)]);
    expect(favourites).toEqual([]);
    expect(groups).toEqual([{ provider: "openrouter", models: [fable] }]);
  });

  test("matches name, id and provider, case-insensitively", () => {
    expect(visibleGroups(catalogue, "SONNET", []).groups).toEqual([
      { provider: "openrouter", models: [sonnet] },
    ]);
    expect(visibleGroups(catalogue, "gemini-3", []).groups).toEqual([
      { provider: "google", models: [gemini] },
    ]);
    // The gateway, so one query narrows to everything it serves.
    expect(visibleGroups(catalogue, "openrouter", []).groups).toEqual([
      { provider: "openrouter", models: [fable, sonnet] },
    ]);
  });

  test("trims the query, and whitespace alone is no query", () => {
    const { favourites, groups } = visibleGroups(catalogue, "  ", [modelKey(gpt)]);
    expect(favourites.map(modelKey)).toEqual([modelKey(gpt)]);
    expect(groups).toHaveLength(3);

    expect(visibleGroups(catalogue, "  fable ", []).groups).toEqual([
      { provider: "openrouter", models: [fable] },
    ]);
  });

  test("a query matching nothing returns an empty picker, not the whole catalogue", () => {
    const { favourites, groups } = visibleGroups(catalogue, "no-such-model", [modelKey(gpt)]);
    expect(favourites).toEqual([]);
    expect(groups).toEqual([]);
  });
});

describe("toggleFavourite", () => {
  test("adds an unknown key to the end", () => {
    expect(toggleFavourite([modelKey(gpt)], modelKey(fable))).toEqual([
      modelKey(gpt),
      modelKey(fable),
    ]);
  });

  test("removes a key that is already stored", () => {
    expect(toggleFavourite([modelKey(gpt), modelKey(fable)], modelKey(gpt))).toEqual([
      modelKey(fable),
    ]);
  });

  test("leaves the caller's list untouched", () => {
    // The component emits the result; mutating the prop would edit the host's
    // copy behind its back.
    const stored = [modelKey(gpt)];
    toggleFavourite(stored, modelKey(gpt));
    expect(stored).toEqual([modelKey(gpt)]);
  });
});

describe("moveFavourite", () => {
  const three = [modelKey(fable), modelKey(sonnet), modelKey(gpt)];

  test("moves a key one place in either direction", () => {
    expect(moveFavourite(three, modelKey(sonnet), -1)).toEqual([
      modelKey(sonnet),
      modelKey(fable),
      modelKey(gpt),
    ]);
    expect(moveFavourite(three, modelKey(sonnet), 1)).toEqual([
      modelKey(fable),
      modelKey(gpt),
      modelKey(sonnet),
    ]);
  });

  test("is a no-op at either end instead of wrapping", () => {
    // A wrap reads as a lost row: the user pressed up and the model left the top.
    expect(moveFavourite(three, modelKey(fable), -1)).toEqual(three);
    expect(moveFavourite(three, modelKey(gpt), 1)).toEqual(three);
  });

  test("clamps a larger move to the end it was heading for", () => {
    // What drag-and-drop sends: `targetIndex - sourceIndex`, which can overshoot
    // when a drop lands beyond the row it started on.
    expect(moveFavourite(three, modelKey(fable), 9)).toEqual([
      modelKey(sonnet),
      modelKey(gpt),
      modelKey(fable),
    ]);
    expect(moveFavourite(three, modelKey(gpt), -9)).toEqual([
      modelKey(gpt),
      modelKey(fable),
      modelKey(sonnet),
    ]);
  });

  test("lands the key on the target's index, whichever way it travels", () => {
    // The drop contract: same delta, both directions, exact position.
    for (const [from, to] of [
      [0, 2],
      [2, 0],
      [0, 1],
      [1, 2],
    ] as const) {
      const moved = moveFavourite(three, three[from], to - from);
      expect(moved[to]).toBe(three[from]);
    }
  });

  test("a key the store does not hold is not appended", () => {
    expect(moveFavourite(three, "openrouter/ghost", 1)).toEqual(three);
  });

  test("leaves the caller's list untouched", () => {
    const stored = [...three];
    moveFavourite(stored, modelKey(gpt), -1);
    expect(stored).toEqual(three);
  });
});

describe("providerGlyph", () => {
  test("takes the initials of a hyphenated provider", () => {
    expect(providerGlyph("amazon-bedrock")).toBe("AB");
    expect(providerGlyph("x-ai")).toBe("XA");
  });

  test("takes the first two letters of a single-word provider", () => {
    expect(providerGlyph("openrouter")).toBe("OP");
    expect(providerGlyph("x")).toBe("X");
  });

  test("normalises case and ignores stray separators", () => {
    expect(providerGlyph("OpenRouter")).toBe("OP");
    expect(providerGlyph("  google  ")).toBe("GO");
  });

  test("an absent provider still gets a badge", () => {
    expect(providerGlyph("")).toBe("?");
  });
});
