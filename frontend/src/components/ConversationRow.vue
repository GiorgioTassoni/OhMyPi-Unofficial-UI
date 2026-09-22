<script setup lang="ts">
/**
 * One row of the conversation (`docs/12` §3.1).
 *
 * The row kinds are the ones the transcript produces — `user`, `assistant`,
 * `tool`, and `notice:<level>` — and each has its own shape in §3.1's table: a
 * user turn is right-aligned with its attachments, an assistant turn is markdown
 * with its reasoning collapsed and tinted, a tool call is a card, and maintenance
 * activity is a quiet line under the turn it belongs to.
 */
import { computed, onBeforeUnmount, ref } from "vue";
import type { AttachmentSnapshot, RowSnapshot } from "../bridge";
import Icon from "./ui/Icon.vue";
import MarkdownBody from "./MarkdownBody.vue";
import ToolCard from "./ToolCard.vue";

const props = defineProps<{ row: RowSnapshot }>();
/** A link the host refused to open, for the window's error banner. */
const emit = defineEmits<{ (event: "failed", message: string): void }>();

const isNotice = computed(() => props.row.role.startsWith("notice:"));
/** `notice:warning` reads as `warning`; the level is the line's tint. */
const level = computed(() => (isNotice.value ? props.row.role.slice(7) : ""));
const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | null = null;

/** Copy only this row's authored text — never its thinking, tool calls, or rendered HTML. */
async function copyMessage(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.row.text);
    copied.value = true;
    if (copiedTimer !== null) clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => {
      copied.value = false;
      copiedTimer = null;
    }, 1_500);
  } catch (cause) {
    emit("failed", `Could not copy message: ${String(cause)}`);
  }
}

onBeforeUnmount(() => {
  if (copiedTimer !== null) clearTimeout(copiedTimer);
});

/**
 * The line's own tint: quiet by default, and the palette's state colours only
 * when the level is one worth noticing. No fill and no border — a maintenance
 * line is a line, not a chip.
 */
function tint(): string {
  switch (level.value) {
    case "error":
      return "text-err";
    case "warning":
      return "text-warn";
    default:
      return "text-faint";
  }
}

/** The engine carries attachment bytes on the block, so no fetch is involved. */
function source(attachment: AttachmentSnapshot): string {
  return `data:${attachment.mimeType};base64,${attachment.data}`;
}
</script>

<template>
  <!-- A user turn: right-aligned, so a long thread reads as a dialogue rather
       than one column of prose. -->
  <div v-if="row.role === 'user'" class="flex flex-col items-end gap-1.5 my-1">
    <div v-if="row.attachments.length > 0" class="flex flex-wrap justify-end gap-1.5">
      <img
        v-for="(attachment, index) in row.attachments"
        :key="index"
        :src="source(attachment)"
        alt="attachment"
        class="max-h-36 rounded-[10px] ring-1 ring-line/60 shadow-sm"
      />
    </div>
    <div v-if="row.text" class="group/message flex max-w-[80%] flex-col items-end gap-1.5">
      <p
        class="whitespace-pre-wrap break-words rounded-[16px] bg-raised/90 border border-line/50 px-4 py-2.5 text-[13px] leading-relaxed text-fg shadow-sm select-text"
      >
        {{ row.text }}
      </p>
      <button
        type="button"
        class="pointer-events-none rounded-[5px] p-1 text-faint opacity-0 transition-[color,background-color,opacity] group-hover/message:pointer-events-auto group-hover/message:opacity-100 hover:bg-raised hover:text-fg focus-visible:pointer-events-auto focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        :aria-label="copied ? 'Message copied' : 'Copy message'"
        :title="copied ? 'Copied' : 'Copy message'"
        @click="copyMessage"
      >
        <Icon :name="copied ? 'check' : 'copy'" class="h-3.5 w-3.5" />
      </button>
    </div>
  </div>

  <!-- An assistant turn: reasoning first, collapsed, then the answer as markdown. -->
  <div v-else-if="row.role === 'assistant'" class="flex flex-col gap-1.5 my-1">
    <details v-if="row.thinking" class="group my-0.5">
      <summary
        class="flex items-center gap-1.5 cursor-pointer select-none text-[12px] font-medium text-faint transition-colors hover:text-dim list-none"
      >
        <Icon name="brain" class="h-3.5 w-3.5 text-faint" />
        <span>Thought</span>
        <Icon name="chevron-right" class="h-3 w-3 text-faint transition-transform group-open:rotate-90" />
      </summary>
      <div
        class="mt-1.5 whitespace-pre-wrap break-words rounded-[8px] border-l-2 border-line-strong/60 bg-surface/50 px-3 py-2 text-[12px] italic leading-relaxed text-dim select-text"
      >
        {{ row.thinking }}
      </div>
    </details>

    <div v-if="row.text" class="group/message flex flex-col gap-1.5">
      <div class="text-[13.5px] leading-relaxed text-fg select-text">
        <MarkdownBody :text="row.text" @failed="emit('failed', $event)" />
      </div>
      <button
        v-if="!row.streaming"
        type="button"
        class="pointer-events-none self-start rounded-[5px] p-1 text-faint opacity-0 transition-[color,background-color,opacity] group-hover/message:pointer-events-auto group-hover/message:opacity-100 hover:bg-raised hover:text-fg focus-visible:pointer-events-auto focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        :aria-label="copied ? 'Message copied' : 'Copy message'"
        :title="copied ? 'Copied' : 'Copy message'"
        @click="copyMessage"
      >
        <Icon :name="copied ? 'check' : 'copy'" class="h-3.5 w-3.5" />
      </button>
    </div>
    <!-- The animation is on the row the engine is still filling, which is what
         makes the coalesced patch stream readable as a live turn. -->
    <span
      v-if="row.streaming"
      class="inline-block h-3.5 w-1.5 animate-pulse self-start rounded-full bg-accent"
    />
  </div>

  <!-- A tool call. -->
  <ToolCard v-else-if="row.role === 'tool' && row.tool" :tool="row.tool" />

  <!-- Maintenance activity: compaction, retries, mode changes, reminders. -->
  <span
    v-else
    class="inline-block max-w-full break-words text-[11.5px] select-text"
    :class="tint()"
  >
    {{ level }} · {{ row.text }}
  </span>
</template>
