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
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { openExternal } from "../bridge";
import { renderMarkdown } from "../lib/markdown";

const props = defineProps<{ text: string }>();
const emit = defineEmits<{ (event: "failed", message: string): void }>();

const root = ref<HTMLElement | null>(null);
const html = computed(() => renderMarkdown(props.text));

const COPY_ICON = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg><span>Copy</span>`;
const CHECK_ICON = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--color-ok)" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg><span style="color:var(--color-ok)">Copied</span>`;

function decorateCodeBlocks(): void {
  if (!root.value) return;
  const pres = root.value.querySelectorAll("pre");
  for (const pre of pres) {
    if (pre.querySelector(".code-header")) continue;
    const code = pre.querySelector("code");
    const langMatch = code?.className.match(/language-([a-zA-Z0-9_-]+)/);
    const lang = langMatch ? langMatch[1] : "";

    const header = document.createElement("div");
    header.className = "code-header select-none";

    const langSpan = document.createElement("span");
    langSpan.className = "code-lang";
    langSpan.textContent = lang;
    header.appendChild(langSpan);

    const copyBtn = document.createElement("button");
    copyBtn.type = "button";
    copyBtn.className = "code-copy-btn";
    copyBtn.title = "Copy code";
    copyBtn.setAttribute("aria-label", "Copy code");
    copyBtn.innerHTML = COPY_ICON;
    header.appendChild(copyBtn);

    pre.insertBefore(header, pre.firstChild);
  }
}

watch(html, () => void nextTick(decorateCodeBlocks));
onMounted(() => void nextTick(decorateCodeBlocks));

/**
 * Hand a link to the host instead of following it here, or handle code block copy.
 */
async function onClick(event: MouseEvent): Promise<void> {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }

  const copyBtn = target.closest<HTMLButtonElement>(".code-copy-btn");
  if (copyBtn) {
    event.preventDefault();
    event.stopPropagation();
    const pre = copyBtn.closest("pre");
    const code = pre?.querySelector("code")?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(code);
      copyBtn.innerHTML = CHECK_ICON;
      setTimeout(() => {
        copyBtn.innerHTML = COPY_ICON;
      }, 1500);
    } catch (cause) {
      emit("failed", `Could not copy code: ${String(cause)}`);
    }
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
  <div ref="root" class="md select-text" v-html="html" @click="onClick" />
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
  letter-spacing: -0.015em;
  color: var(--color-fg);
}

.md :deep(h1) {
  font-size: 19px;
  margin-top: 1rem;
  margin-bottom: 0.4rem;
}
.md :deep(h2) {
  font-size: 16px;
  margin-top: 0.85rem;
  margin-bottom: 0.35rem;
}
.md :deep(h3) {
  font-size: 14.5px;
  margin-top: 0.75rem;
  margin-bottom: 0.25rem;
}
.md :deep(h4) {
  font-size: 13.5px;
  margin-top: 0.6rem;
  margin-bottom: 0.2rem;
}
.md :deep(h5),
.md :deep(h6) {
  font-size: 13px;
  margin-top: 0.5rem;
  margin-bottom: 0.2rem;
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
  margin-top: 0.2rem;
}

.md :deep(blockquote) {
  border-left: 1px solid var(--color-accent);
  padding-left: 0.75rem;
  color: var(--color-dim);
  font-style: italic;
  margin: 0.5rem 0;
}
.md :deep(blockquote > * + *) {
  margin-top: 0.4rem;
}

.md :deep(details) {
  border: 1px solid var(--color-line);
  border-radius: 8px;
  background-color: color-mix(in srgb, var(--color-raised) 35%, transparent);
  padding: 0.45rem 0.65rem;
  margin: 0.5rem 0;
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
  font-size: 0.86em;
  background-color: var(--color-raised);
  color: var(--color-fg);
  border-radius: 4px;
  padding: 0.1rem 0.35rem;
  border: 1px solid color-mix(in srgb, var(--color-line) 60%, transparent);
}

/* Fenced code block with header and copy button */
.md :deep(pre) {
  background-color: var(--color-surface);
  border: 1px solid var(--color-line);
  border-radius: 8px;
  padding: 0;
  overflow: hidden;
  margin: 0.65rem 0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
}

.md :deep(pre .code-header) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.35rem 0.75rem;
  background-color: color-mix(in srgb, var(--color-raised) 65%, transparent);
  border-bottom: 1px solid var(--color-line);
  font-size: 11px;
}

.md :deep(pre .code-lang) {
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--color-faint);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  font-weight: 500;
}

.md :deep(pre .code-copy-btn) {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.15rem 0.45rem;
  border-radius: 4px;
  font-size: 11px;
  font-family: var(--font-sans);
  color: var(--color-faint);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 120ms ease;
}

.md :deep(pre .code-copy-btn:hover) {
  color: var(--color-fg);
  background-color: var(--color-surface);
}

.md :deep(pre code) {
  display: block;
  background-color: transparent;
  padding: 0.65rem 0.85rem;
  font-size: 12px;
  line-height: 1.5;
  border-radius: 0;
  border: none;
  overflow-x: auto;
}

.md :deep(hr) {
  border-color: var(--color-line);
  margin: 1rem 0;
}

/* Polished tables */
.md :deep(table) {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  font-size: 12.5px;
  border-radius: 6px;
  border: 1px solid var(--color-line);
  overflow: hidden;
  margin: 0.75rem 0;
}
.md :deep(th) {
  background-color: var(--color-surface);
  font-weight: 600;
  color: var(--color-fg);
  border-bottom: 1px solid var(--color-line);
  padding: 0.45rem 0.65rem;
  text-align: left;
}
.md :deep(td) {
  border-bottom: 1px solid var(--color-line);
  padding: 0.4rem 0.65rem;
  text-align: left;
}
.md :deep(tr:last-child td) {
  border-bottom: none;
}
.md :deep(tr:nth-child(even) td) {
  background-color: color-mix(in srgb, var(--color-surface) 35%, transparent);
}

.md :deep(kbd) {
  display: inline-block;
  padding: 0.1rem 0.4rem;
  font-size: 11px;
  font-family: var(--font-mono);
  line-height: 1.25;
  color: var(--color-fg);
  background-color: var(--color-raised);
  border: 1px solid var(--color-line-strong);
  border-radius: 4px;
  box-shadow: 0 1px 0 color-mix(in srgb, var(--color-line-strong) 80%, black);
}

/* Links */
.md :deep(.md-link) {
  color: var(--color-accent);
  text-underline-offset: 3px;
  transition: color 120ms ease;
}
.md :deep(.md-link:hover) {
  color: var(--color-accent-bright);
}
.md :deep(.md-url) {
  color: var(--color-faint);
  font-family: var(--font-mono);
  font-size: 0.78em;
  word-break: break-all;
}
</style>
