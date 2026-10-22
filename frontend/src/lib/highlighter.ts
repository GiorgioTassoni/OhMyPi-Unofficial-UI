/**
 * Syntax highlighter using PrismJS.
 * Tokenizes code into line-by-line HTML spans styled via app theme CSS tokens.
 */
import Prism from "prismjs";
import "prismjs/components/prism-markup";
import "prismjs/components/prism-css";
import "prismjs/components/prism-clike";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-jsx";
import "prismjs/components/prism-tsx";
import "prismjs/components/prism-dart";
import "prismjs/components/prism-rust";
import "prismjs/components/prism-python";
import "prismjs/components/prism-json";
import "prismjs/components/prism-yaml";
import "prismjs/components/prism-toml";
import "prismjs/components/prism-bash";
import "prismjs/components/prism-sql";
import "prismjs/components/prism-go";
import "prismjs/components/prism-c";
import "prismjs/components/prism-cpp";
import "prismjs/components/prism-markdown";

// Enhance Dart grammar to highlight named constructor arguments and object properties (e.g. child:, builder:)
if (Prism.languages.dart && !Prism.languages.dart.property) {
  Prism.languages.insertBefore("dart", "punctuation", {
    property: {
      pattern: /\b[a-zA-Z_$][a-zA-Z0-9_$]*(?=\s*:)/,
      lookbehind: false,
    },
  });
}

export function escapeHtml(text: string): string {
  return String(Prism.util.encode(text));
}

import { cleanExtension } from "./fileViewMeta";

export { cleanExtension };

const EXTENSION_MAP: Record<string, string> = {
  ts: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "jsx",
  vue: "markup",
  html: "markup",
  xml: "markup",
  svg: "markup",
  css: "css",
  scss: "css",
  dart: "dart",
  rs: "rust",
  py: "python",
  json: "json",
  yml: "yaml",
  yaml: "yaml",
  toml: "toml",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  sql: "sql",
  go: "go",
  c: "c",
  h: "c",
  cpp: "cpp",
  hpp: "cpp",
  cc: "cpp",
  md: "markdown",
  markdown: "markdown",
};

function pushToken(token: string | Prism.Token, lang: string, lines: string[]): void {
  if (typeof token === "string") {
    const parts = token.split("\n");
    for (let i = 0; i < parts.length; i++) {
      if (i > 0) lines.push("");
      lines[lines.length - 1] += String(Prism.util.encode(parts[i]));
    }
    return;
  }

  const html = Prism.Token.stringify(token, lang);
  if (!html.includes("\n")) {
    lines[lines.length - 1] += html;
    return;
  }

  const openTagMatch = html.match(/^<span class="([^"]+)">/);
  if (openTagMatch && html.endsWith("</span>")) {
    const className = openTagMatch[1];
    const inner = html.slice(openTagMatch[0].length, -7);
    const parts = inner.split("\n");
    for (let i = 0; i < parts.length; i++) {
      if (i > 0) lines.push("");
      if (parts[i].length > 0) {
        lines[lines.length - 1] += `<span class="${className}">${parts[i]}</span>`;
      }
    }
  } else {
    const parts = html.split("\n");
    for (let i = 0; i < parts.length; i++) {
      if (i > 0) lines.push("");
      lines[lines.length - 1] += parts[i];
    }
  }
}

/**
 * Highlights a full source string into formatted HTML lines.
 * Guaranteed to keep HTML tags balanced across every line.
 */
export function highlightCode(code: string, extension: string): string[] {
  if (!code) return ["&nbsp;"];
  const clean = cleanExtension(extension);
  const lang = EXTENSION_MAP[clean] || clean;
  const grammar = Prism.languages[lang];
  if (!grammar) {
    return code.split(/\r?\n/).map((line) => (line.length ? escapeHtml(line) : "&nbsp;"));
  }

  const tokens = Prism.tokenize(code, grammar);
  const lines: string[] = [""];
  for (const token of tokens) {
    pushToken(token, lang, lines);
  }

  return lines.map((line) => (line.length ? line : "&nbsp;"));
}
