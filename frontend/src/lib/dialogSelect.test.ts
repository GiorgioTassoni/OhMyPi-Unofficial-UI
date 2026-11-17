import { describe, expect, it } from "bun:test";
import { dialogSummaryTitle, formatCollapsibleTitle } from "./dialogSelect";

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
