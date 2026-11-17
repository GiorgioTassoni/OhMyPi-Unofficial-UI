import { describe, expect, it } from "bun:test";
import type { AskQuestion } from "../bridge";
import {
  answerOf,
  emptySelection,
  emptyStates,
  selectOption,
  selectionFor,
  selectionReady,
  setCustomInput,
} from "./askDialog";

function makeQuestion(overrides: Partial<AskQuestion> = {}): AskQuestion {
  return {
    id: "q1",
    question: "Which framework?",
    options: [{ label: "Vue" }, { label: "React" }],
    multi: false,
    ...overrides,
  };
}

const multiQuestion = makeQuestion({ id: "q2", multi: true });

describe("askDialog selection", () => {
  it("replaces the choice of a single question", () => {
    const question = makeQuestion();
    const picked = selectOption(question, emptySelection(), "Vue");
    expect(picked.selected).toEqual(["Vue"]);

    const switched = selectOption(question, picked, "React");
    expect(switched.selected).toEqual(["React"]);
  });

  it("toggles a multi question and allows none", () => {
    const none = emptySelection();
    expect(selectionReady(multiQuestion, none)).toBe(true);

    const one = selectOption(multiQuestion, none, "Vue");
    expect(one.selected).toEqual(["Vue"]);

    const two = selectOption(multiQuestion, one, "React");
    expect(two.selected).toEqual(["Vue", "React"]);

    expect(selectOption(multiQuestion, two, "Vue").selected).toEqual(["React"]);
  });

  it("keeps a single question to a choice or custom input, never both", () => {
    const question = makeQuestion();
    const picked = selectOption(question, emptySelection(), "Vue");
    const custom = setCustomInput(question, picked, "Svelte");
    expect(custom.selected).toEqual([]);
    expect(custom.custom).toBe("Svelte");

    const back = selectOption(question, custom, "React");
    expect(back.selected).toEqual(["React"]);
    expect(back.custom).toBe("");
  });

  it("lets a multi question carry selected options and custom input at once", () => {
    const picked = selectOption(multiQuestion, emptySelection(), "Vue");
    const custom = setCustomInput(multiQuestion, picked, "Svelte");
    expect(custom.selected).toEqual(["Vue"]);
    expect(answerOf(multiQuestion, custom)).toEqual({
      id: "q2",
      selectedOptions: ["Vue"],
      customInput: "Svelte",
    });
  });
});

describe("askDialog readiness", () => {
  it("refuses an empty single question until a choice or custom input exists", () => {
    const question = makeQuestion();
    expect(selectionReady(question, emptySelection())).toBe(false);
    expect(selectionReady(question, setCustomInput(question, emptySelection(), "   "))).toBe(false);
    expect(selectionReady(question, setCustomInput(question, emptySelection(), "Svelte"))).toBe(true);
    expect(selectionReady(question, selectOption(question, emptySelection(), "Vue"))).toBe(true);
  });
});

describe("askDialog states", () => {
  it("keys state by question id, so repeated question text stays separate", () => {
    const first = makeQuestion({ id: "q1" });
    const second = makeQuestion({ id: "q2", question: "Which framework?" });
    const states = emptyStates([first, second]);

    states[first.id] = selectOption(first, states[first.id]!, "Vue");

    expect(selectionFor(states, first).selected).toEqual(["Vue"]);
    expect(selectionFor(states, second).selected).toEqual([]);
    expect(emptyStates([second])).not.toHaveProperty(first.id);
  });
});

describe("askDialog answers", () => {
  it("preserves option labels exactly and omits an empty custom input", () => {
    const question = makeQuestion({
      options: [{ label: "✓ Done selecting" }, { label: "  spaced label  " }],
    });
    const answer = answerOf(question, selectOption(question, emptySelection(), "  spaced label  "));
    expect(answer).toEqual({ id: "q1", selectedOptions: ["  spaced label  "] });
  });

  it("trims custom input and reports multi with no selection", () => {
    const question = makeQuestion({ id: "q3", multi: true });
    expect(answerOf(question, emptySelection())).toEqual({ id: "q3", selectedOptions: [] });
    expect(answerOf(question, setCustomInput(question, emptySelection(), "  Svelte  "))).toEqual({
      id: "q3",
      selectedOptions: [],
      customInput: "Svelte",
    });
  });
});
