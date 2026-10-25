import { describe, expect, test } from "bun:test";
import { countNewlines, createPastedTextAttachment, generatePastedTextName, shouldAttachAsPastedText } from "./pastedText";
import type { Attachment } from "./attachments";

describe("pastedText", () => {
  test("countNewlines counts newlines without string allocation", () => {
    expect(countNewlines("")).toBe(0);
    expect(countNewlines("hello")).toBe(0);
    expect(countNewlines("hello\nworld")).toBe(1);
    expect(countNewlines("1\n2\n3\n4\n5")).toBe(4);
  });

  test("shouldAttachAsPastedText returns true for >= 50 lines", () => {
    const fortyNineLines = Array(49).fill("line").join("\n");
    expect(shouldAttachAsPastedText(fortyNineLines)).toBe(false);

    const fiftyLines = Array(50).fill("line").join("\n");
    expect(shouldAttachAsPastedText(fiftyLines)).toBe(true);

    const sixtyLines = Array(60).fill("line").join("\n");
    expect(shouldAttachAsPastedText(sixtyLines)).toBe(true);
  });

  test("shouldAttachAsPastedText returns true for large text blocks with >= 20 lines", () => {
    const longLine = "a".repeat(150);
    const twentyLines = Array(20).fill(longLine).join("\n"); // 3000+ chars
    expect(shouldAttachAsPastedText(twentyLines)).toBe(true);

    const shortContent = "abc\n".repeat(20); // ~80 chars, 20 lines
    expect(shouldAttachAsPastedText(shortContent)).toBe(false);
  });

  test("generatePastedTextName infers appropriate names and increments counters", () => {
    const existing: Attachment[] = [];

    // Plain text
    expect(generatePastedTextName("ordinary log text\nmore lines", existing)).toBe("pasted-text.txt");

    // JSON
    expect(generatePastedTextName('{"key": "value"}', existing)).toBe("pasted-data.json");

    // Code
    expect(generatePastedTextName("import { ref } from 'vue';\nconst x = 1;", existing)).toBe("pasted-code.txt");

    // Uniqueness
    const withExisting: Attachment[] = [
      { id: "1", kind: "text", name: "pasted-text.txt", content: "foo", bytes: 3 },
    ];
    expect(generatePastedTextName("another long text\nline", withExisting)).toBe("pasted-text-2.txt");

    const withTwoExisting: Attachment[] = [
      ...withExisting,
      { id: "2", kind: "text", name: "pasted-text-2.txt", content: "bar", bytes: 3 },
    ];
    expect(generatePastedTextName("third long text\nline", withTwoExisting)).toBe("pasted-text-3.txt");
  });

  test("createPastedTextAttachment creates a valid Attachment object", () => {
    const text = "line 1\nline 2\nline 3";
    const attachment = createPastedTextAttachment(text, []);
    expect(attachment.kind).toBe("text");
    expect(attachment.name).toBe("pasted-text.txt");
    if (attachment.kind === "text") {
      expect(attachment.content).toBe(text);
      expect(attachment.bytes).toBe(new TextEncoder().encode(text).length);
    }
    expect(typeof attachment.id).toBe("string");
  });
});
