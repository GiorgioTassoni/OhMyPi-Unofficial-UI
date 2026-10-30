/**
 * The sanitizer, asserted against the strings that would hurt.
 *
 * This is the only frontend test, and it exists for one reason: `renderMarkdown`
 * turns model output into markup inside a webview that can call host commands.
 * The failure it guards against — a `<script>` or a handler surviving into the DOM
 * — is silent in a visual check, because a screenshot of a leaked payload looks
 * identical to a screenshot of inert text.
 */

import { describe, expect, test } from "bun:test";
// The DOM DOMPurify needs is registered by the preload in `bunfig.toml`.
import { renderMarkdown } from "./markdown";

/** What a browser would build from the sanitized HTML. */
function parse(html: string): HTMLElement {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host;
}

describe("renderMarkdown", () => {
  test("drops a script element, source and all", () => {
    const html = renderMarkdown("before\n\n<script>window.__pwned = 1</script>\n\nafter");

    expect(html).not.toContain("<script");
    expect(html).not.toContain("__pwned"); // the text goes too: no stray source left
    expect(html).toContain("before");
    expect(html).toContain("after");
  });

  test("keeps the text of raw HTML but drops its event handlers", () => {
    // Raw HTML is sanitized rather than discarded: dropping it would lose what
    // the model wrote inside a tag the allowlist already makes safe.
    const html = renderMarkdown('<p onclick="alert(1)" onmouseover="alert(2)">text</p>');

    expect(html).toBe("<p>text</p>");
  });

  test("drops an injected image with an error handler", () => {
    const html = renderMarkdown('<img src="x" onerror="alert(1)">');

    expect(html).not.toContain("<img");
    expect(html).not.toContain("onerror");
  });

  test("keeps markdown syntax working", () => {
    const html = renderMarkdown("# Title\n\n- one\n- two\n\n`code` and **bold**");

    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<li>one</li>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<strong>bold</strong>");
  });

  test("safely isolates YAML frontmatter without turning it into a setext heading", () => {
    const skillDoc = `---
name: impeccable
description: "Design and polish"
metadata:
  version: 4.7.0
---

## Setup
Run context`;
    const html = renderMarkdown(skillDoc);
    expect(html).not.toContain("<h2>name: impeccable");
    expect(html).toContain("<details");
    expect(html).toContain("Metadata");
    expect(html).toContain("<h2>Setup</h2>");
    expect(html).toContain("<p>Run context</p>");
  });

  test("keeps safe collapsible details without trusting its attributes", () => {
    const host = parse(
      renderMarkdown(
        '<details open onclick="alert(1)">\n<summary>Why</summary>\n\nBecause.\n</details>',
      ),
    );
    const details = host.querySelector("details");

    expect(details).not.toBeNull();
    expect(details?.hasAttribute("open")).toBe(true);
    expect(details?.getAttribute("onclick")).toBeNull();
    expect(details?.querySelector("summary")?.textContent).toBe("Why");
    expect(details?.textContent).toContain("Because.");
  });

  test("keeps separate paragraphs inside a blockquote", () => {
    const quote = parse(renderMarkdown("> first\n>\n> second")).querySelector("blockquote");

    expect(quote?.querySelectorAll("p")).toHaveLength(2);
    expect(quote?.textContent).toContain("first");
    expect(quote?.textContent).toContain("second");
  });

  test("renders a fenced block as code, not as a live element", () => {
    const html = renderMarkdown("```html\n<script>alert(1)</script>\n```");

    expect(html).toContain("<pre>");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });

  test("makes a link the host would open clickable, but never navigable", () => {
    const anchor = parse(renderMarkdown("[docs](https://omp.sh/docs)")).querySelector("a");

    expect(anchor?.getAttribute("data-href")).toBe("https://omp.sh/docs");
    // No `href`: an anchor that can navigate replaces the app with a web page
    // while the host bridge stays attached, which no sanitizer setting prevents.
    expect(anchor?.hasAttribute("href")).toBe(false);
    expect(anchor?.getAttribute("onclick")).toBeNull();
  });

  test("shows dangerous link schemes as text instead", () => {
    for (const url of [
      "javascript:alert(1)",
      "vbscript:msgbox(1)",
      "data:text/html;base64,PHNjcmlwdD4=",
    ]) {
      const host = parse(renderMarkdown(`[click](${url})`));

      expect(host.querySelector("a")).toBeNull();
      expect(host.textContent).toContain(url);
    }
  });

  test("renders file paths and file URIs as clickable links", () => {
    for (const url of [
      "/tmp/setty-terrain-confronto-tablet.png",
      "file:///home/user/photo.jpg",
      "./src/components/ThreadView.vue",
      "skill://impeccable",
    ]) {
      const host = parse(renderMarkdown(`[open file](${url})`));
      const anchor = host.querySelector("a");

      expect(anchor).not.toBeNull();
      expect(anchor?.getAttribute("data-href")).toBe(url);
      expect(anchor?.hasAttribute("href")).toBe(false);
      expect(anchor?.textContent).toBe("open file");
    }
  });

  test("makes inline code containing file paths clickable", () => {
    const host = parse(renderMarkdown("Check `/tmp/output.png` and `src/lib/markdown.ts`"));
    const codeLinks = host.querySelectorAll("code.md-code-link");

    expect(codeLinks.length).toBe(2);
    expect(codeLinks[0]?.getAttribute("data-href")).toBe("/tmp/output.png");
    expect(codeLinks[1]?.getAttribute("data-href")).toBe("src/lib/markdown.ts");
  });

  test("autolinks standalone file paths in text", () => {
    const host = parse(renderMarkdown("Saved to /tmp/screenshot.png. Check it!"));
    const anchor = host.querySelector("a");

    expect(anchor).not.toBeNull();
    expect(anchor?.getAttribute("data-href")).toBe("/tmp/screenshot.png");
    expect(anchor?.textContent).toBe("/tmp/screenshot.png");
  });

  test("strips an href a model wrote as raw HTML", () => {
    // `href` is not in the attribute allowlist, so the model's own anchor cannot
    // become navigable even though it wrote valid HTML.
    const host = parse(renderMarkdown('<a href="https://evil.example">click</a>'));

    expect(host.querySelector("a")?.hasAttribute("href")).toBe(false);
    expect(host.textContent).toContain("click");
  });

  test("a URL cannot escape its attribute and open a second one", () => {
    // Without escaping, a crafted URL would close `data-href` and open an allowed
    // attribute — and the click handler would then hand the host the wrong URL.
    const crafted = 'https://ok"data-href="https://evil.example';
    const anchors = parse(renderMarkdown(`[x](${crafted})`)).querySelectorAll("a");

    expect(anchors.length).toBe(1);
    expect(anchors[0]?.getAttribute("data-href")).toBe(crafted);
    expect(anchors[0]?.getAttribute("title")).toBe(crafted);
  });

  test("keeps a data: image out of the DOM even though the CSP would load it", () => {
    const html = renderMarkdown("![chart](data:image/png;base64,iVBORw0KGgo=)");

    expect(parse(html).querySelector("img")).toBeNull();
  });

  test("renders inline math like $O(n)$ with KaTeX", () => {
    const html = renderMarkdown("costo $O(n)$ per ogni tasto");
    expect(html).toContain('class="katex"');
    expect(html).toContain("O");
    expect(html).toContain("n");
    expect(html).not.toContain("$O(n)$");
  });

  test("renders block math with KaTeX", () => {
    const html = renderMarkdown("$$\\sum_{i=1}^n i$$");
    expect(html).toContain('class="katex-display"');
  });

  test("strips inline styles from arbitrary elements while preserving math styles", () => {
    const html = renderMarkdown('<p style="position:fixed;top:0;left:0;color:red;">malicious text</p>');
    expect(html).not.toContain("position:fixed");
    expect(html).not.toContain("color:red");
    expect(html).toContain("<p>malicious text</p>");
  });

  test("renders section references as styled section chips", () => {
    const html = renderMarkdown("See §4 and §Code styling, §§12-14, or §AI-Created 2.");
    expect(html).toContain('<span class="md-section"><span class="md-section-sym">§</span>4</span>');
    expect(html).toContain('<span class="md-section"><span class="md-section-sym">§</span>Code</span>');
    expect(html).toContain('<span class="md-section"><span class="md-section-sym">§§</span>12-14</span>');
    expect(html).toContain('<span class="md-section"><span class="md-section-sym">§</span>AI-Created</span>');
  });
});
