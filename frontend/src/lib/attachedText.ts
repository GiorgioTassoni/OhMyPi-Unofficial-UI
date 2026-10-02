/** Recover browser-picked text files from the exact suffix `messageFor` persists. */
export interface AttachedTextFile {
  name: string;
  content: string;
  bytes: number;
}

export interface DisplayMessage {
  text: string;
  files: AttachedTextFile[];
}

const FILE_BLOCK = /(?:^|\n\n)Attached file: ("(?:\\.|[^"\\])*")\n(`{3,})text\n([\s\S]*?)\n\2(?=\n\nAttached file: |$)/g;

/** Only collapse complete file blocks at the end; ordinary user text stays untouched. */
export function displayMessage(raw: string): DisplayMessage {
  const matches = [...raw.matchAll(FILE_BLOCK)];
  if (matches.length === 0) return { text: raw, files: [] };

  const start = matches[0].index;
  let cursor = start;
  const files: AttachedTextFile[] = [];
  for (const match of matches) {
    if (match.index !== cursor) return { text: raw, files: [] };
    let name: unknown;
    try {
      name = JSON.parse(match[1]);
    } catch {
      return { text: raw, files: [] };
    }
    if (typeof name !== "string") return { text: raw, files: [] };
    const content = match[3];
    files.push({ name, content, bytes: new TextEncoder().encode(content).length });
    cursor += match[0].length;
  }
  if (cursor !== raw.length) return { text: raw, files: [] };

  return { text: raw.slice(0, start).trimEnd(), files };
}
