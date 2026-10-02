import { describe, expect, test } from "bun:test";
import { messageFor, type Attachment } from "./attachments";
import { displayMessage } from "./attachedText";

function file(name: string, content: string): Attachment {
  return { id: name, kind: "text", name, content, bytes: new TextEncoder().encode(content).length };
}

describe("text-file cards in user messages", () => {
  test("keeps the prompt visible and recovers two attached files", () => {
    const raw = messageFor("Please review these", [file("a.md", "# One"), file("b.txt", "Second")]);
    expect(displayMessage(raw)).toEqual({
      text: "Please review these",
      files: [
        { name: "a.md", content: "# One", bytes: 5 },
        { name: "b.txt", content: "Second", bytes: 6 },
      ],
    });
  });

  test("a text-only message becomes one card with no empty prompt bubble", () => {
    expect(displayMessage(messageFor("", [file("empty.txt", "")]))).toEqual({
      text: "", files: [{ name: "empty.txt", content: "", bytes: 0 }],
    });
  });

  test("paths stay in the text and text files stay at the end", () => {
    const path: Attachment = { id: "path", kind: "path", name: "other.md", path: "/tmp/other.md" };
    const raw = messageFor("Read both", [file("picked.md", "picked"), path]);
    expect(displayMessage(raw).text).toBe("Read both\n\n/tmp/other.md");
    expect(displayMessage(raw).files[0].name).toBe("picked.md");
  });

  test("backticks inside a file cannot close its block early", () => {
    const content = "before\n```\nafter";
    expect(displayMessage(messageFor("look", [file("code.md", content)])).files[0].content).toBe(content);
  });

  test("ordinary and incomplete lookalike text is never hidden", () => {
    for (const raw of ["Attached file: notes.md", "Attached file: \"notes.md\"\n```text\nunfinished", "hello"]) {
      expect(displayMessage(raw)).toEqual({ text: raw, files: [] });
    }
  });
});
