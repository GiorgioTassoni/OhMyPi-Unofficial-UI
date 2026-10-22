/**
 * Frontmatter parser for markdown documents and skill definitions.
 *
 * Extracts YAML frontmatter blocks delimited by leading `---` markers,
 * separating structured metadata (name, description, version, etc.) from the markdown body.
 */

export interface ParsedFrontmatter {
  hasFrontmatter: boolean;
  attributes: Record<string, string>;
  rawFrontmatter: string;
  body: string;
}

const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/;

export function parseFrontmatter(text: string): ParsedFrontmatter {
  if (!text) {
    return { hasFrontmatter: false, attributes: {}, rawFrontmatter: "", body: "" };
  }

  const match = text.match(FRONTMATTER_RE);
  if (!match) {
    return { hasFrontmatter: false, attributes: {}, rawFrontmatter: "", body: text };
  }

  const rawFrontmatter = match[1];
  const body = text.slice(match[0].length);
  const attributes: Record<string, string> = {};

  for (const line of rawFrontmatter.split(/\r?\n/)) {
    const kv = line.match(/^\s*([a-zA-Z0-9_-]+)\s*:\s*(.*)$/);
    if (kv) {
      const key = kv[1].toLowerCase();
      let val = kv[2].trim();
      if ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'"))) {
        val = val.slice(1, -1);
      }
      if (val && !attributes[key]) {
        attributes[key] = val;
      }
    }
  }

  return { hasFrontmatter: true, attributes, rawFrontmatter, body };
}
