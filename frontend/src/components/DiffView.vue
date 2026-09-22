<script setup lang="ts">
/**
 * A unified diff, as `docs/12` §3.2 asks for in the `edit` card.
 *
 * The text is the engine's own `details.diff`, so this file only classifies lines
 * — it never compares anything. §3.2's "collapsing from a one-line summary to full
 * output" is the card's own header/body split; the body here is bounded and
 * scrolls, so a thousand-line diff cannot take over the transcript.
 */
import { computed } from "vue";

const props = defineProps<{ diff: string }>();

type Line = { text: string; kind: string };

const lines = computed<Line[]>(() => {
  const raw = props.diff.replace(/\n$/, "").split("\n");
  return raw.map((text) => ({ text, kind: classify(text) }));
});

const counts = computed(() => {
  let added = 0;
  let removed = 0;
  for (const line of lines.value) {
    if (line.kind === "added") added += 1;
    if (line.kind === "removed") removed += 1;
  }

  return { added, removed };
});

function classify(line: string): string {
  // The file headers are checked before the +/- prefixes: `+++` is a file name,
  // not an added line, and counting it as one would inflate every diff by one.
  if (line.startsWith("+++") || line.startsWith("---")) return "file";
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("diff ") || line.startsWith("index ")) return "file";
  if (line.startsWith("\\")) return "meta";
  if (line.startsWith("+")) return "added";
  if (line.startsWith("-")) return "removed";
  return "context";
}

function tint(kind: string): string {
  switch (kind) {
    case "added":
      return "text-ok bg-ok/5";
    case "removed":
      return "text-err bg-err/5";
    // A hunk header is a position, not a highlight: a change of place is not a
    // state the accent is reserved for.
    case "hunk":
      return "text-faint";
    case "file":
      return "text-[12px] text-fg";
    case "meta":
      return "text-faint italic";
    default:
      return "text-dim";
  }
}
</script>

<template>
  <div class="border-t border-line/40 bg-canvas/30 select-text">
    <div class="flex items-center gap-2.5 border-b border-line/30 px-3 py-1.5 font-mono text-[11px] text-faint select-none">
      <span class="font-medium text-ok">+{{ counts.added }}</span>
      <span class="font-medium text-err">-{{ counts.removed }}</span>
      <span class="text-faint/80">{{ lines.length }} lines</span>
    </div>

    <pre
      class="max-h-80 overflow-auto px-3 py-2 font-mono text-[11.5px] leading-[1.5] text-dim select-text"
    ><div v-for="(line, index) in lines" :key="index" :class="tint(line.kind)">{{ line.text }}</div></pre>
  </div>
</template>
