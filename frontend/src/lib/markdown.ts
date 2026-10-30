/**
 * Assistant text, rendered as markdown — under the assumption that it is hostile.
 *
 * `docs/12` §3.1 asks for markdown in an assistant row. The text comes from a
 * language model, so it is untrusted input, and this webview can invoke host
 * commands: an injected `<script>` or an `onerror` handler would not be a broken
 * layout, it would be code execution with the app's privileges. Two rules, on the
 * two ways markdown can become an action rather than a picture:
 *
 * 1. **Nothing here navigates; links are opened by the host.** A link is an `<a>`
 *    with **no `href`** — only a `data-href` that a click hands to
 *    `bridge.openExternal`, and the host's allowlist decides. An `href` would let
 *    the webview navigate *itself*: the conversation replaced by a web page with the
 *    host bridge still attached, which no sanitizer setting prevents. An image
 *    renders as its reference rather than as an `<img>`, because loading one is a
 *    request the model asked the app to make (remote tracking, or a local-file
 *    probe).
 * 2. **DOMPurify, with an explicit allowlist.** One enforcing layer, not two: raw
 *    HTML in the model's answer is passed through and sanitized rather than
 *    dropped, because dropping it loses whatever the model wrote inside a `<table>`
 *    or a `<details>` — content the allowlist already makes safe.
 *    `img` is not in the allowlist, which is why rule 1 holds for raw HTML too,
 *    and the CSP (`tauri.conf.json`, `img-src 'self' data:`) is a third barrier
 *    that does not depend on this file.
 *
 * `markdown.test.ts` pins all of it: a screenshot of a leaked payload is
 * indistinguishable from one of inert text, so this is asserted, not eyeballed.
 */

import DOMPurify from "dompurify";
import { marked } from "marked";
import markedKatex from "marked-katex-extension";

/**
 * The elements a conversation message is allowed to produce.
 *
 * `a` is here, `href` is not (see [`ALLOWED_ATTR`]): our own anchors carry a
 * `data-href` the host resolves, and the model's raw HTML cannot produce a
 * navigable link.
 */
const ALLOWED_TAGS = [
  "a",
  "p",
  "br",
  "hr",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "ul",
  "ol",
  "li",
  "blockquote",
  "details",
  "summary",
  "pre",
  "code",
  "em",
  "strong",
  "del",
  "span",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  // KaTeX / MathML elements
  "math",
  "semantics",
  "mrow",
  "mi",
  "mo",
  "mn",
  "ms",
  "mspace",
  "mtext",
  "annotation",
  "mover",
  "munder",
  "munderover",
  "msup",
  "msub",
  "msubsup",
  "mfrac",
  "mroot",
  "msqrt",
  "mtable",
  "mtr",
  "mtd",
];

/**
 * Attributes allowed through. `href` is deliberately absent: the model's raw HTML
 * must not be able to produce a navigable anchor, and our own `data-href` is the
 * only URL carrier the sanitizer lets survive.
 */
const ALLOWED_ATTR = ["class", "title", "data-href", "open", "style", "aria-hidden", "encoding", "xmlns", "role", "tabindex"];

// Restrict inline `style` strictly to KaTeX math elements to prevent CSS injection/redressing.
if (typeof DOMPurify.addHook === "function") {
  DOMPurify.addHook("uponSanitizeAttribute", (node, data) => {
    if (data.attrName === "style") {
      const tag = node.tagName.toLowerCase();
      const isMathTag = tag.startsWith("m");
      const isKatexSpan = tag === "span" && (
        (typeof node.className === "string" && (node.className.includes("katex") || node.className.includes("vlist") || node.className.includes("strut")))
        || (typeof node.closest === "function" && Boolean(node.closest(".katex")))
      );
      if (!isMathTag && !isKatexSpan) {
        data.keepAttr = false;
      }
    }
  });
}

marked.use(markedKatex({ throwOnError: false }));
marked.use({
  extensions: [
    {
      name: "sectionRef",
      level: "inline",
      start(src: string) {
        return src.indexOf("§");
      },
      tokenizer(src: string) {
        const match = /^(§{1,2})\s*([a-zA-Z0-9_]+(?:[-.][a-zA-Z0-9_]+)*)/.exec(src);
        if (match) {
          return {
            type: "sectionRef",
            raw: match[0],
            sym: match[1],
            section: match[2],
          };
        }
      },
      renderer(token) {
        const sym = typeof token["sym"] === "string" ? token["sym"] : "§";
        const sec = typeof token["section"] === "string" ? token["section"] : "";
        return `<span class="md-section"><span class="md-section-sym">${escape(sym)}</span>${escape(sec)}</span>`;
      },
    },
    {
      name: "rawPath",
      level: "inline",
      start(src: string) {
        const match = /(?:file:\/\/|(?:\/(?:tmp|home|var|usr|opt|etc)|~\/)[a-zA-Z0-9_./@~-]+)/.exec(src);
        return match ? match.index : -1;
      },
      tokenizer(src: string) {
        const match = /^(?:file:\/\/\S+|(?:(?:\/(?:tmp|home|var|usr|opt|etc)|~\/)[a-zA-Z0-9_./@~-]+(?:\.[a-zA-Z0-9]+)?))/.exec(src);
        if (match) {
          let raw = match[0];
          while (/[.,;:!?)\]'"]$/.test(raw)) {
            raw = raw.slice(0, -1);
          }
          if (raw.length > 2) {
            return {
              type: "rawPath",
              raw,
              href: raw,
              text: raw,
            };
          }
        }
      },
      renderer(token) {
        const href = typeof token["href"] === "string" ? token["href"] : "";
        const text = typeof token["text"] === "string" ? token["text"] : href;
        const attribute = escape(href);
        return `<a class="md-link" data-href="${attribute}" title="${attribute}" role="link" tabindex="0">${escape(text)}</a>`;
      },
    },
  ],
});
marked.use({
  gfm: true,
  breaks: true,
  renderer: {
    link({ href, text }) {
      return reference(href, text);
    },
    image({ href, text }) {
      return reference(href, text || "image");
    },
    codespan({ text }: { text: string }) {
      const trimmed = text.trim();
      if (isPathLike(trimmed)) {
        const attribute = escape(trimmed);
        return `<code class="md-code-link" data-href="${attribute}" title="${attribute}" role="link" tabindex="0">${escape(text)}</code>`;
      }
      return `<code>${escape(text)}</code>`;
    },
  },
});

export function isPathLike(text: string): boolean {
  if (!text) return false;
  const trimmed = text.trim();
  if (/^file:\/\/\S+/i.test(trimmed)) return true;
  if (/^skills?:\/\/\S+/i.test(trimmed)) return true;
  if (/^~[/\\]\S+/.test(trimmed)) return true;
  if (/^[a-zA-Z]:[/\\]\S+/.test(trimmed)) return true;
  if (/^\.\.?[/\\]\S+/.test(trimmed)) return true;
  if (/^\/(?:tmp|home|var|usr|etc|opt|dev)\/\S+/.test(trimmed)) return true;
  if (/^[a-zA-Z0-9_.-]+(?:\/[a-zA-Z0-9_.-]+)+\.[a-zA-Z0-9]+(?::\d+(?:-\d+)?)?$/.test(trimmed)) return true;
  return false;
}

function reference(href: string, text: string): string {
  const url = href.trim();
  if (isSafeUrl(url)) {
    const attribute = escape(url);
    return `<a class="md-link" data-href="${attribute}" title="${attribute}" role="link" tabindex="0">${text}</a>`;
  }

  return `<span class="md-link">${text} <span class="md-url">${escape(url)}</span></span>`;
}

const DANGEROUS_SCHEME = /^(javascript|vbscript|data):/i;

function isSafeUrl(url: string): boolean {
  if (!url) return false;
  const trimmed = url.trim();
  if (DANGEROUS_SCHEME.test(trimmed)) return false;
  return true;
}

function escape(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/;

/**
 * Render one message's markdown to sanitized HTML.
 *
 * Pure, so the sanitizer is exercised against real strings rather than only
 * against whatever a live model happened to say (`markdown.test.ts`).
 */
export function renderMarkdown(text: string): string {
  let markdownText = text;
  let frontmatterHtml = "";

  const match = text.match(FRONTMATTER_RE);
  if (match) {
    const rawYaml = match[1].trim();
    markdownText = text.slice(match[0].length);
    frontmatterHtml = `<details class="md-frontmatter"><summary>Metadata</summary><pre><code>${escape(rawYaml)}</code></pre></details>\n\n`;
  }

  const html = marked.parse(markdownText, { async: false });
  const combined = frontmatterHtml ? frontmatterHtml + html : html;
  return DOMPurify.sanitize(combined, { ALLOWED_TAGS, ALLOWED_ATTR: ALLOWED_ATTR });
}
