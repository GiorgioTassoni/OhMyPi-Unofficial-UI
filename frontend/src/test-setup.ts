/**
 * The DOM the frontend's tests run against.
 *
 * Bun has no DOM and DOMPurify binds to a `window` when it is imported, so a
 * window has to exist before any module under test is loaded. That is what makes
 * this a preload (`bunfig.toml`) rather than a line in a test file: a test's own
 * imports are hoisted above its statements, so registering here is the only way
 * `import { renderMarkdown } from "./markdown"` can stay a static import.
 *
 * jsdom rather than the lighter happy-dom, decided by measurement: under
 * happy-dom, DOMPurify's *default* allowlist drops `<h1>`, `<p>` and `<pre>`
 * (`"<p>plain</p>"` sanitizes to `"plain"`). A sanitizer that misbehaves in the
 * test environment cannot be asserted against it — the markdown tests would have
 * been checking a pipeline that no browser runs.
 */

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>");

Object.assign(globalThis, {
  window: dom.window as unknown as Window,
  document: dom.window.document,
});
