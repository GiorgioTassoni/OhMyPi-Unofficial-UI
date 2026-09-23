<script setup lang="ts">
/**
 * A unified diff, as `docs/12` §3.2 asks for in the `edit` card.
 *
 * Tool cards show the engine's own `details.diff`; the end-of-turn review can supply
 * source-ordered pieces with an omitted-lines marker. This view never compares files.
 * §3.2's "collapsing from a one-line summary to full
 * output" is the card's own header/body split; the body here is bounded and
 * scrolls, so a thousand-line diff cannot take over the transcript.
 */
import { computed } from "vue";
import type { ReviewPiece } from "../lib/reviewDiff";

const props = defineProps<{ diff?: string; pieces?: ReviewPiece[]; compact?: boolean }>();

type Line = { kind: "diff"; text: string; tone: string } | { kind: "gap"; count: number };

const lines = computed<Line[]>(() => {
  const pieces: ReviewPiece[] = props.pieces ?? [{ kind: "diff", text: props.diff ?? "" }];
  return pieces.flatMap((piece): Line[] => {
    if (piece.kind === "gap") return [{ kind: "gap", count: piece.lines }];
    return piece.text.replace(/\n$/, "").split("\n").map((text) => ({
      kind: "diff", text, tone: classify(text),
    }));
  });
});

const lineCount = computed(() => lines.value.filter((line) => line.kind === "diff").length);

const counts = computed(() => {
  let added = 0;
  let removed = 0;
  for (const line of lines.value) {
    if (line.kind !== "diff") continue;
    if (line.tone === "added") added += 1;
    if (line.tone === "removed") removed += 1;
  }

  return { added, removed };
});

function classify(line: string): string {
  // The file headers are checked before the +/- prefixes: `+++` is a file name,
  // not an added line, and counting it as one would inflate every diff by one.
  if (line.startsWith("+++ ") || line.startsWith("--- ")) return "file";
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
  <div class="select-text" :class="compact ? 'bg-canvas/15' : 'border-t border-line/40 bg-canvas/30'">
    <div v-if="!compact" class="flex items-center gap-2.5 border-b border-line/30 px-3 py-1.5 font-mono text-[11px] text-faint select-none">
      <span class="font-medium text-ok">+{{ counts.added }}</span>
      <span class="font-medium text-err">-{{ counts.removed }}</span>
      <span class="text-faint/80">{{ lineCount }} lines</span>
    </div>

    <pre
      class="max-h-80 overflow-auto whitespace-normal px-3 py-2 font-mono text-[11.5px] leading-[1.5] text-dim select-text"
    ><div class="w-max min-w-full"><template v-for="(line, index) in lines" :key="index">
      <div v-if="line.kind === 'gap'" class="flex items-center gap-2 py-1.5 text-[11px] text-faint select-none">
        <span class="h-px min-w-3 flex-1 bg-line/70" />
        <span>{{ line.count }} unchanged line{{ line.count === 1 ? '' : 's' }}</span>
        <span class="h-px min-w-3 flex-1 bg-line/70" />
      </div>
      <div v-else class="whitespace-pre" :class="tint(line.tone)">{{ line.text }}</div>
    </template></div></pre>
  </div>
</template>
