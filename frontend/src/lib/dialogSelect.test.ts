import { describe, expect, it } from "bun:test";
import {
  cleanOptionLabel,
  dialogSummaryTitle,
  formatCollapsibleTitle,
  isCustomInputOption,
  isDoneOption,
  isMultiSelectDialog,
  isOptionSelected,
  questionKey,
  titleParts,
} from "./dialogSelect";
import type { UiRequestSnapshot } from "../bridge";

function makeDialog(overrides: Partial<UiRequestSnapshot> = {}): UiRequestSnapshot {
  return {
    id: "dlg_1",
    kind: "select",
    title: "Pick an option",
    message: "",
    options: ["Alpha", "Beta", "Gamma"],
    optionDetails: [],
    prefill: null,
    placeholder: null,
    timeoutMs: null,
    ...overrides,
  };
}

describe("dialogSelect helpers", () => {
  describe("isDoneOption", () => {
    it("recognizes various Done selecting patterns", () => {
      expect(isDoneOption("Done selecting")).toBe(true);
      expect(isDoneOption("✓ Done selecting")).toBe(true);
      expect(isDoneOption("✔ Done selecting")).toBe(true);
      expect(isDoneOption("[x] Done selecting")).toBe(true);
      expect(isDoneOption("Fine selezione")).toBe(true);
      expect(isDoneOption("Linear")).toBe(false);
      expect(isDoneOption("Other (type your own)")).toBe(false);
    });
  });

  describe("isCustomInputOption", () => {
    it("recognizes Other custom input options", () => {
      expect(isCustomInputOption("Other (type your own)")).toBe(true);
      expect(isCustomInputOption("Other")).toBe(true);
      expect(isCustomInputOption("Altro")).toBe(true);
      expect(isCustomInputOption("Altro (scrivi la tua risposta)")).toBe(true);
      expect(isCustomInputOption("Linear")).toBe(false);
    });
  });

  describe("cleanOptionLabel", () => {
    it("strips checkmarks, bullets, and normalizes Done selecting", () => {
      expect(cleanOptionLabel("✓ Done selecting")).toBe("Done selecting");
      expect(cleanOptionLabel("• Linear")).toBe("Linear");
      expect(cleanOptionLabel("[x] Vercel")).toBe("Vercel");
      expect(cleanOptionLabel("✓ Raycast")).toBe("Raycast");
      expect(cleanOptionLabel("Stripe")).toBe("Stripe");
    });
  });

  describe("titleParts", () => {
    it("extracts selected badge when present", () => {
      const parts = titleParts("(1 selected) Which frameworks to use?");
      expect(parts.selectedBadge).toBe("1 selected");
      expect(parts.text).toBe("Which frameworks to use?");
    });

    it("extracts italian selected badge", () => {
      const parts = titleParts("(2 selezionati) Su quali prodotti devo tarare?");
      expect(parts.selectedBadge).toBe("2 selezionati");
      expect(parts.text).toBe("Su quali prodotti devo tarare?");
    });

    it("returns null badge when no count is in title", () => {
      const parts = titleParts("Su quali prodotti devo tarare il livello?");
      expect(parts.selectedBadge).toBe(null);
      expect(parts.text).toBe("Su quali prodotti devo tarare il livello?");
    });

    it("extracts question batch progress from trailing (2/2)", () => {
      const parts = titleParts("Does the comment ban cover stylesheets, or only executable code? (2/2)");
      expect(parts.stepBadge).toBe("2 of 2");
      expect(parts.text).toBe("Does the comment ban cover stylesheets, or only executable code?");
    });

    it("extracts question batch progress from leading [1/3]", () => {
      const parts = titleParts("[1/3] Which deployment provider do you use?");
      expect(parts.stepBadge).toBe("1 of 3");
      expect(parts.text).toBe("Which deployment provider do you use?");
    });

    it("strips dumped CLI choices and detects custom response follow-up", () => {
      const raw = "Does the comment ban cover stylesheets? (2/2)\n\n○ Executable code only\n○ Everything\n🔘 Other (type your own)";
      const parts = titleParts(raw);
      expect(parts.stepBadge).toBe("2 of 2");
      expect(parts.isCustomResponse).toBe(true);
      expect(parts.text).toBe("Does the comment ban cover stylesheets?");
    });
  });

  describe("questionKey", () => {
    it("normalizes question title across round-trips with changing selected count", () => {
      const key0 = questionKey("thread_1", "Su quali prodotti devo tarare? (scegli 2-3)");
      const key1 = questionKey("thread_1", "(1 selected) Su quali prodotti devo tarare? (scegli 2-3)");
      const key2 = questionKey("thread_1", "(2 selected) Su quali prodotti devo tarare? (scegli 2-3)");

      expect(key0).toBe("thread_1:Su quali prodotti devo tarare? (scegli 2-3)");
      expect(key1).toBe(key0);
      expect(key2).toBe(key0);
    });
  });

  describe("isMultiSelectDialog", () => {
    it("detects multi-select from options with Done selecting", () => {
      const dlg = makeDialog({
        options: ["Linear", "Vercel", "✓ Done selecting", "Other (type your own)"],
      });
      expect(isMultiSelectDialog(dlg)).toBe(true);
    });

    it("detects multi-select from (1 selected) in title", () => {
      const dlg = makeDialog({
        title: "(1 selected) Pick your favorites",
        options: ["Linear", "Vercel"],
      });
      expect(isMultiSelectDialog(dlg)).toBe(true);
    });

    it("detects multi-select from phrasing like scegli 2-3", () => {
      const dlg = makeDialog({
        title: "Su quali prodotti devo tarare il livello di finitura? (scegli 2-3)",
        options: ["Linear", "Vercel"],
      });
      expect(isMultiSelectDialog(dlg)).toBe(true);
    });

    it("detects multi-select from optionDetails with selected field", () => {
      const dlg = makeDialog({
        title: "Preferences",
        options: ["A", "B"],
        optionDetails: [{ selected: true }, { selected: false }],
      });
      expect(isMultiSelectDialog(dlg)).toBe(true);
    });

    it("returns false for regular single-choice dialogs", () => {
      const dlg = makeDialog({
        title: "Choose default branch",
        options: ["main", "master", "develop"],
      });
      expect(isMultiSelectDialog(dlg)).toBe(false);
    });
  });

  describe("isOptionSelected", () => {
    it("checks explicit optionDetails.selected", () => {
      const dlg = makeDialog({
        options: ["Linear", "Vercel"],
        optionDetails: [{ selected: true }, { selected: false }],
      });
      expect(isOptionSelected(dlg, "Linear", 0)).toBe(true);
      expect(isOptionSelected(dlg, "Vercel", 1)).toBe(false);
    });

    it("checks checkmark prefix in option label", () => {
      const dlg = makeDialog({
        options: ["✓ Linear", "Vercel"],
      });
      expect(isOptionSelected(dlg, "✓ Linear", 0)).toBe(true);
      expect(isOptionSelected(dlg, "Vercel", 1)).toBe(false);
    });

    it("checks selectedSet", () => {
      const dlg = makeDialog({
        options: ["Linear", "Vercel", "Raycast"],
      });
      const selectedSet = new Set(["Linear", "Raycast"]);
      expect(isOptionSelected(dlg, "Linear", 0, selectedSet)).toBe(true);
      expect(isOptionSelected(dlg, "Vercel", 1, selectedSet)).toBe(false);
      expect(isOptionSelected(dlg, "Raycast", 2, selectedSet)).toBe(true);
    });
  });

  describe("formatCollapsibleTitle", () => {
    it("preserves short single-line titles", () => {
      expect(formatCollapsibleTitle("Which database do you want?")).toBe("Which database do you want?");
    });

    it("collapses multiple whitespaces and newlines into single spaces", () => {
      expect(formatCollapsibleTitle("Which database\n\n  do you want?")).toBe("Which database do you want?");
    });

    it("truncates long titles with an ellipsis at maxChars cap", () => {
      const longTitle = "This is a very long question asking about architecture, database schema, migration pipelines, and deployment settings.";
      const truncated = formatCollapsibleTitle(longTitle, 40);
      expect(truncated.length).toBeLessThanOrEqual(41); // 40 chars + ellipsis
      expect(truncated.endsWith("…")).toBe(true);
      expect(truncated).toBe("This is a very long question asking abou…");
    });
  });

  describe("dialogSummaryTitle", () => {
    it("formats command approvals with Run: prefix", () => {
      const title = dialogSummaryTitle({
        titleText: "",
        approvalTool: "bash",
        command: "cargo test --workspace",
      });
      expect(title).toBe("Run: cargo test --workspace");
    });

    it("formats general tool approvals with Approval: prefix", () => {
      const title = dialogSummaryTitle({
        titleText: "",
        approvalTool: "web_search",
      });
      expect(title).toBe("Approval: web_search");
    });

    it("formats question titles directly", () => {
      const title = dialogSummaryTitle({
        titleText: "Which frontend framework do you prefer?",
        kind: "select",
      });
      expect(title).toBe("Which frontend framework do you prefer?");
    });

    it("falls back to message or kind when title text is empty", () => {
      const title1 = dialogSummaryTitle({
        titleText: "",
        message: "Are you sure?",
        kind: "confirm",
      });
      expect(title1).toBe("Are you sure?");

      const title2 = dialogSummaryTitle({
        titleText: "",
        kind: "confirm",
      });
      expect(title2).toBe("Confirmation");
    });
  });
});
