import type { RowSnapshot } from "../bridge";

export interface SystemNotice {
  headline: string;
  body: string;
}

/** Engine-delivered agent notices are messages, not `notice:<level>` UI events. */
export function systemNotice(row: RowSnapshot): SystemNotice | null {
  if (row.customType !== "async-result" && !/^\s*<system-notice>/i.test(row.text)) return null;

  const body = row.text
    .replace(/^\s*<system-notice>\s*/i, "")
    .replace(/\s*<\/system-notice>\s*$/i, "")
    .trim();
  const firstLine = body.split("\n", 1)[0]?.trim() ?? "";
  const sentenceEnd = firstLine.search(/\.(?:\s|$)/);
  const headline = sentenceEnd >= 0 ? firstLine.slice(0, sentenceEnd + 1) : firstLine;
  return { headline: headline || "New system message", body };
}
