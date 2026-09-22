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

  test("shows a link the host would refuse as text instead", () => {
    for (const url of [
      "javascript:alert(1)",
      "file:///etc/passwd",
      "vscode://file/etc/passwd",
    ]) {
      const host = parse(renderMarkdown(`[click](${url})`));

      expect(host.querySelector("a")).toBeNull();
      expect(host.textContent).toContain(url); // the destination stays visible
    }
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
});
