import type { AskAnswer, AskQuestion } from "../bridge";

export interface AskSelectionState {
  selected: string[];
  custom: string;
}

export function emptySelection(): AskSelectionState {
  return { selected: [], custom: "" };
}

export function emptyStates(questions: AskQuestion[]): Record<string, AskSelectionState> {
  return Object.fromEntries(questions.map((question) => [question.id, emptySelection()]));
}

export function selectionFor(
  states: Record<string, AskSelectionState>,
  question: AskQuestion,
): AskSelectionState {
  return states[question.id] ?? emptySelection();
}

export function selectOption(
  question: AskQuestion,
  state: AskSelectionState,
  label: string,
): AskSelectionState {
  if (question.multi) {
    const selected = state.selected.includes(label)
      ? state.selected.filter((entry) => entry !== label)
      : [...state.selected, label];
    return { ...state, selected };
  }
  return { selected: [label], custom: "" };
}

export function setCustomInput(
  question: AskQuestion,
  state: AskSelectionState,
  text: string,
): AskSelectionState {
  return question.multi
    ? { ...state, custom: text }
    : { selected: [], custom: text };
}

export function selectionReady(question: AskQuestion, state: AskSelectionState): boolean {
  return question.multi || state.selected.length === 1 || state.custom.trim() !== "";
}

export function answerOf(question: AskQuestion, state: AskSelectionState): AskAnswer {
  const custom = state.custom.trim();
  return {
    id: question.id,
    selectedOptions: [...state.selected],
    ...(custom === "" ? {} : { customInput: custom }),
  };
}
