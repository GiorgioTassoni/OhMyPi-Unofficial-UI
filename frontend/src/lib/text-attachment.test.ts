import { describe, expect, test } from "bun:test";
import { messageFor } from "./attachments";
import { MAX_TEXT_FILE_BYTES, readTextAttachment } from "./text-attachment";

describe("selected text files", () => {
  test("Markdown and code reach the message as UTF-8 text", async () => {
    const selected = new File(["# Plan\nShip it"], "plan.md", { type: "text/markdown" });
    const attachment = await readTextAttachment(selected, "id-1");
    expect(attachment).toEqual({
      id: "id-1", kind: "text", name: "plan.md", content: "# Plan\nShip it", bytes: selected.size,
    });
    expect(messageFor("review", [attachment])).toContain("# Plan\nShip it");
  });

  test("oversized, binary, and invalid UTF-8 files are refused before sending", async () => {
    await expect(readTextAttachment(new File(["x".repeat(MAX_TEXT_FILE_BYTES + 1)], "huge.txt"), "id"))
      .rejects.toThrow("limited");
    await expect(readTextAttachment(new File([new Uint8Array([0xff])], "bad.txt"), "id"))
      .rejects.toThrow("UTF-8");
    await expect(readTextAttachment(new File(["a\0b"], "binary.txt"), "id"))
      .rejects.toThrow("binary");
    await expect(readTextAttachment(new File(["%PDF-1.7"], "paper.pdf", { type: "application/pdf" }), "id"))
      .rejects.toThrow("plain-text");
  });
});
