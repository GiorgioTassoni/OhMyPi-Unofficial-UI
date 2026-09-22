<script setup lang="ts">
/**
 * An assistant message, as markdown.
 *
 * The rendering and its sanitizing live in `lib/markdown.ts`; this component only
 * arranges the result. `v-html` is safe here for the reason that file documents —
 * the string has already been through a strict allowlist — and it is the reason
 * that file is asserted against an injection rather than eyeballed.
 *
 * Markdown's own elements carry no classes, so the tag styles are scoped here
 * instead of pulling in a typography plugin for one view.
 */
import { computed } from "vue";
import { openExternal } from "../bridge";
import { renderMarkdown } from "../lib/markdown";

const props = defineProps<{ text: string }>();
const emit = defineEmits<{ (event: "failed", message: string): void }>();

const html = computed(() => renderMarkdown(props.text));

/**
 * Hand a link to the host instead of following it here.
 *
 * The anchor has no `href` (see `lib/markdown.ts`), so there is nothing for the
 * webview to navigate to even if this handler never ran — and the host, not this
 * component, decides whether the URL may be opened. A refusal is reported upward
 * rather than swallowed: a click that does nothing is the failure mode this whole
 * arrangement exists to avoid.
 */
async function onClick(event: MouseEvent): Promise<void> {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }

  const url = target.closest("[data-href]")?.getAttribute("data-href");
  if (!url) {
    return;
  }

  // Do not navigate if user was selecting/highlighting text across the link
  const selection = window.getSelection();
  if (selection && selection.toString().length > 0) {
    return;
  }

  event.preventDefault();
  try {
    await openExternal(url);
  } catch (cause) {
    emit("failed", String(cause));
  }
}
</script>

<template>
  <div class="md select-text" v-html="html" @click="onClick" />
</template>

<style scoped>
.md :deep(> * + *) {
  margin-top: 0.5rem;
}

/* Headings carry the hierarchy the transcript was missing: prose is 13.5px, so
   each level has to step away from it to read as a heading at all. */
.md :deep(h1),
.md :deep(h2),
.md :deep(h3),
.md :deep(h4),
.md :deep(h5),
.md :deep(h6) {
  font-weight: 600;
  line-height: 1.3;
}

.md :deep(h1) {
  font-size: 20px;
}
.md :deep(h2) {
  font-size: 16px;
}
.md :deep(h3) {
  font-size: 15px;
}
.md :deep(h4) {
  font-size: 13.5px;
}
.md :deep(h5),
.md :deep(h6) {
  font-size: 13px;
}

.md :deep(ul),
.md :deep(ol) {
  padding-left: 1.25rem;
}
.md :deep(ul) {
  list-style: disc;
}
.md :deep(ol) {
  list-style: decimal;
}
.md :deep(li) {
  margin-top: 0.15rem;
}

.md :deep(blockquote) {
  border-left: 2px solid var(--color-line);
  padding-left: 0.6rem;
  color: var(--color-dim);
}
.md :deep(blockquote > * + *) {
  margin-top: 0.5rem;
}

.md :deep(details) {
  border: 1px solid var(--color-line);
  border-radius: 8px;
  background-color: color-mix(in srgb, var(--color-raised) 45%, transparent);
  padding: 0.45rem 0.6rem;
}
.md :deep(summary) {
  cursor: pointer;
  color: var(--color-fg);
  font-weight: 500;
}
.md :deep(details[open] > summary) {
  margin-bottom: 0.5rem;
}
.md :deep(details > *:not(summary) + *) {
  margin-top: 0.5rem;
}

.md :deep(code) {
  font-family: var(--font-mono);
  font-size: 0.85em;
  background-color: var(--color-raised);
  color: var(--color-fg);
  border-radius: 0.25rem;
  padding: 0.05rem 0.3rem;
}

/* A fenced block is the one card inside prose: darker than the page it sits on,
   with a hairline — the same shape every other card in the window has. */
.md :deep(pre) {
  background-color: var(--color-surface);
  border: 1px solid var(--color-line);
  border-radius: 8px;
  padding: 0.6rem 0.75rem;
  overflow-x: auto;
}

.md :deep(pre code) {
  background-color: transparent;
  padding: 0;
  font-size: 12px;
  line-height: 1.5;
}

.md :deep(hr) {
  border-color: var(--color-line);
}

.md :deep(table) {
  border-collapse: collapse;
  font-size: 12.5px;
}
.md :deep(th),
.md :deep(td) {
  border: 1px solid var(--color-line);
  padding: 0.25rem 0.5rem;
  text-align: left;
}

/* A link is not an anchor (see `lib/markdown.ts`): the text reads as a link and
   the URL is shown beside it, so nothing in a message is a navigable control. */
.md :deep(.md-link) {
  color: var(--color-accent);
}
.md :deep(.md-url) {
  color: var(--color-faint);
  font-family: var(--font-mono);
  font-size: 0.78em;
  word-break: break-all;
}
</style>
