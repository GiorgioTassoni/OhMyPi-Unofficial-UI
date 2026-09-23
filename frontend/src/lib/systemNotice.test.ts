import { describe, expect, test } from "bun:test";
import { row } from "./rows.fixture";
import { systemNotice } from "./systemNotice";

describe("agent system notices", () => {
  test("a restored background-job delivery gets a short title and complete body", () => {
    const notice = systemNotice(row({
      role: "custom",
      customType: "async-result",
      text: "<system-notice>\nBackground job bg_15 has completed. Resume your work.\nBuild output\n</system-notice>",
    }));
    expect(notice).toEqual({
      headline: "Background job bg_15 has completed.",
      body: "Background job bg_15 has completed. Resume your work.\nBuild output",
    });
  });

  test("tagged notices without a known custom type are still shown", () => {
    expect(systemNotice(row({ role: "custom", text: "<system-notice>Pay attention</system-notice>" }))?.headline)
      .toBe("Pay attention");
  });

  test("an ordinary custom message stays an ordinary message", () => {
    expect(systemNotice(row({ role: "custom", text: "ordinary extension message" }))).toBeNull();
  });
});
