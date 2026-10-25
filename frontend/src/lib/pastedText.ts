import type { Attachment } from "./attachments";

/**
 * Counts the number of newline characters in a string without allocating array of lines.
 */
export function countNewlines(text: string): number {
  let count = 0;
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) count++;
  }
  return count;
}

/**
 * Determines whether a pasted text string should be collapsed into an attachment
 * rather than bloating the composer and chat message with an enormous wall of text.
 * Triggers for 50+ lines, or large text blocks (>= 2500 chars with >= 20 lines).
 */
export function shouldAttachAsPastedText(text: string): boolean {
  const newlines = countNewlines(text);
  // Total lines is newlines + 1 (if text is non-empty)
  const lineCount = text.length > 0 ? newlines + 1 : 0;
  return lineCount >= 50 || (text.length >= 2500 && lineCount >= 20);
}

/**
 * Generates an appropriate file name for a pasted text block.
 */
export function generatePastedTextName(text: string, existingAttachments: readonly Attachment[]): string {
  const trimmed = text.trim();
  let ext = "txt";
  let base = "pasted-text";

  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    base = "pasted-data";
    ext = "json";
  } else if (trimmed.startsWith("<") && trimmed.endsWith(">")) {
    base = "pasted-markup";
    ext = "xml";
  } else if (
    trimmed.includes("import ") ||
    trimmed.includes("export ") ||
    trimmed.includes("function ") ||
    trimmed.includes("const ") ||
    trimmed.includes("class ")
  ) {
    base = "pasted-code";
    ext = "txt";
  }

  const existingNames = new Set(existingAttachments.map((a) => a.name));
  const candidate = `${base}.${ext}`;
  if (!existingNames.has(candidate)) {
    return candidate;
  }

  let counter = 2;
  while (existingNames.has(`${base}-${counter}.${ext}`)) {
    counter++;
  }
  return `${base}-${counter}.${ext}`;
}

/**
 * Creates a text attachment from pasted text.
 */
export function createPastedTextAttachment(text: string, existingAttachments: readonly Attachment[]): Attachment {
  const name = generatePastedTextName(text, existingAttachments);
  return {
    id: crypto.randomUUID(),
    kind: "text",
    name,
    content: text,
    bytes: new TextEncoder().encode(text).length,
  };
}
