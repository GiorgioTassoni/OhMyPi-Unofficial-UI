<script setup lang="ts">
/**
 * The Files tab (`docs/12` §8.2): what this session changed, the workspace it changed it in, and
 * the results the engine spilled out of a card.
 *
 * Changed and Artifacts are derived from the transcript rather than asked for: the rows *are*
 * the record of what a session did, and a list read from anywhere else could disagree with the
 * cards beside it. The tree is the one thing the window cannot derive, so that is the one thing
 * the host walks.
 */
import { computed, ref } from "vue";
import { readArtifact, type ArtifactSnapshot, type RowSnapshot } from "../bridge";
import { artifacts, changedFiles, formatBytes, relativeTo, type TurnReviewRequest } from "../lib/panel";
import { parseObject } from "../lib/toolView";
import DiffView from "./DiffView.vue";
import FileTree from "./FileTree.vue";
import Icon from "./ui/Icon.vue";
import TurnFiles from "./TurnFiles.vue";

const props = defineProps<{
  thread: string | null;
  rows: RowSnapshot[];
  /** The thread's working directory: the tree's root, and what paths are shown against. */
  cwd: string | null;
  live: boolean;
  review: TurnReviewRequest | null;
}>();

const emit = defineEmits<{
  jump: [row: number];
  failed: [message: string];
  closeReview: [];
}>();

type View = "changed" | "tree" | "artifacts";

const view = ref<View>("changed");

/**
 * The Changed list, with everything a row draws decided in one pass.
 *
 * `lib/panel.ts` owns *which* files a session changed; what is added here is the pair of
 * facts a row shows beside the path — the split between the folder and the file's own name,
 * and the diff counted — because the display should not recompute either per render.
 */
const changed = computed(() =>
  changedFiles(props.rows).map((file) => {
    const path = relativeTo(file.path, props.cwd);
    const cut = path.lastIndexOf("/");
    return {
      ...file,
      /** The path as a row reads it: the folder is context, the name is the subject. */
      folder: cut === -1 ? "" : path.slice(0, cut + 1),
      name: cut === -1 ? path : path.slice(cut + 1),
      /** The engine's own diff, counted, or null when it reported no diff for this file. */
      stats: diffStats(file.diff),
    };
  }),
);

/** The spilled results, each carrying what the card that spilled it said about the file. */
const spilled = computed(() =>
  artifacts(props.rows).map((entry) => ({ ...entry, ...spill(entry.row) })),
);

/** Added and removed lines in a unified diff, headers aside. */
function diffStats(diff: string | null): { added: number; removed: number } | null {
  if (diff === null) return null;

  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++") || line.startsWith("---")) continue;
    if (line.startsWith("+")) added += 1;
    else if (line.startsWith("-")) removed += 1;
  }

  return added + removed === 0 ? null : { added, removed };
}

/**
 * What the engine said about the result an artifact holds.
 *
 * The artifact record lives in the spilling card's `details.meta.truncation` and nowhere else
 * on this side: `ArtifactRef` carries the id, and reading the file is a round trip a row has
 * not made. `totalBytes` is the *untruncated* result — the same quantity `readArtifact`
 * reports as the file's size — so a row can show a real number rather than a guess.
 */
function spill(row: number): { bytes: number | null; truncated: boolean } {
  const details = parseObject(props.rows[row]?.tool?.details ?? "");
  const meta = details === null ? null : details.meta;
  const truncation =
    meta !== null && typeof meta === "object"
      ? (meta as Record<string, unknown>).truncation
      : null;

  if (truncation === null || typeof truncation !== "object") {
    return { bytes: null, truncated: false };
  }

  const fields = truncation as Record<string, unknown>;
  const size = [fields.totalBytes, fields.bytes].find((value) => typeof value === "number");

  return {
    bytes: typeof size === "number" ? size : null,
    truncated: typeof fields.truncatedBy === "string" || typeof fields.direction === "string",
  };
}

/** Which changed file has its diff open, and which artifact is being read. */
const openDiff = ref<string | null>(null);
const artifact = ref<ArtifactSnapshot | null>(null);
const reading = ref(false);

async function read(id: string): Promise<void> {
  const thread = props.thread;
  if (thread === null) return;

  reading.value = true;
  artifact.value = null;
  try {
    artifact.value = await readArtifact(thread, id);
  } catch (cause) {
    emit("failed", cause instanceof Error ? cause.message : String(cause));
  } finally {
    reading.value = false;
  }
}

async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (cause) {
    emit("failed", cause instanceof Error ? cause.message : String(cause));
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <!--
      The three views are one size down from the panel's own tabs: the same strip, saying
      "inside Files" rather than "inside the window".
    -->
    <div class="flex shrink-0 items-center gap-1 px-2 py-2">
      <button
        v-for="tab in (['changed', 'tree', 'artifacts'] as View[])"
        :key="tab"
        class="rounded-[6px] px-2.5 py-1 text-[11.5px]"
        :class="view === tab ? 'bg-raised text-fg' : 'text-dim hover:text-fg'"
        :aria-pressed="view === tab"
        @click="view = tab"
      >
        {{ tab === 'changed' && props.review ? 'review' : tab }}
        <span v-if="tab === 'changed' && !props.review && changed.length" class="font-mono text-[10.5px] text-faint">{{ changed.length }}</span>
        <span v-else-if="tab === 'artifacts' && spilled.length" class="font-mono text-[10.5px] text-faint">{{ spilled.length }}</span>
      </button>
    </div>

    <!-- One turn's complete review, opened from its transcript card. -->
    <div v-if="view === 'changed' && props.review" class="min-h-0 flex-1 overflow-auto pb-2">
      <div class="flex items-center justify-between px-3 py-2">
        <span class="text-[11.5px] text-dim">Turn review</span>
        <button
          type="button"
          class="rounded-[5px] px-2 py-1 text-[11.5px] text-faint hover:bg-raised hover:text-fg"
          @click="emit('closeReview')"
        >
          Back to files
        </button>
      </div>
      <div class="px-2">
        <TurnFiles
          :summary="props.review.summary"
          :workspace="props.cwd"
          sidebar
          @jump="emit('jump', $event)"
        />
      </div>
    </div>

    <!-- Changed: this session's own edits, in the order it made them. -->
    <div v-else-if="view === 'changed'" class="flex min-h-0 flex-1 flex-col overflow-auto pb-2">
      <p
        v-if="changed.length === 0"
        class="mx-auto max-w-sm px-4 py-8 text-center text-[12.5px] leading-relaxed text-faint"
      >
        This session has not written or edited a file yet. A file a
        <span class="font-mono">bash</span> command wrote does not appear here — no tool reports
        a path for one.
      </p>

      <template v-for="file in changed" :key="file.path">
        <div class="group mx-1.5 flex items-center gap-2 rounded-[6px] px-1.5 py-1.5 hover:bg-raised">
          <span
            class="grid size-4 shrink-0 place-items-center"
            :class="file.kind === 'written' ? 'text-ok' : 'text-accent'"
            :title="file.kind === 'written' ? 'the file was written whole' : 'the file was edited'"
          >
            <Icon :name="file.kind === 'written' ? 'file' : 'pencil'" class="h-3.5 w-3.5" />
          </span>
          <button
            class="min-w-0 flex-1 truncate text-left font-mono text-[12px] text-fg"
            :title="file.path"
            @click="emit('jump', file.row)"
          >
            <span class="text-faint">{{ file.folder }}</span><span>{{ file.name }}</span>
          </button>
          <span v-if="file.touches > 1" class="shrink-0 font-mono text-[10.5px] text-faint">
            ×{{ file.touches }}
          </span>
          <span v-if="file.stats" class="shrink-0 font-mono text-[10.5px]">
            <span class="text-ok">+{{ file.stats.added }}</span> <span class="text-err">−{{ file.stats.removed }}</span>
          </span>
          <button
            v-if="file.diff"
            class="shrink-0 rounded-[5px] px-2 py-0.5 text-[11.5px]"
            :class="openDiff === file.path ? 'text-accent' : 'text-faint hover:bg-raised hover:text-fg'"
            @click="openDiff = openDiff === file.path ? null : file.path"
          >
            diff
          </button>
        </div>
        <div v-if="openDiff === file.path && file.diff" class="px-3 pb-2">
          <DiffView :diff="file.diff" />
        </div>
      </template>
    </div>

    <!-- Tree: the workspace, unfolded one level at a time by the host. -->
    <FileTree
      v-else-if="view === 'tree' && props.thread !== null"
      :thread="props.thread"
      :node="null"
      :depth="0"
      @failed="emit('failed', $event)"
    />

    <!-- Artifacts: results too large to sit in a card, read back from the engine's own files. -->
    <div v-else-if="view === 'artifacts'" class="flex min-h-0 flex-1 flex-col overflow-auto pb-2">
      <p
        v-if="spilled.length === 0"
        class="mx-auto max-w-sm px-4 py-8 text-center text-[12.5px] leading-relaxed text-faint"
      >
        Nothing has spilled out of a card yet. A tool result too large for the transcript is
        written to a file and its card keeps a pointer:
        <span class="font-mono">artifact://&lt;id&gt;</span>.
      </p>

      <template v-for="entry in spilled" :key="`${entry.id}-${entry.row}`">
        <div class="mx-1.5 flex items-center gap-2 rounded-[6px] px-1.5 py-1.5 hover:bg-raised">
          <button
            class="min-w-0 flex-1 truncate text-left font-mono text-[12px] text-fg"
            :title="`read artifact://${entry.id}`"
            @click="read(entry.id)"
          >
            artifact://{{ entry.id }}
          </button>
          <span v-if="entry.truncated" class="shrink-0 text-[11px] text-warn">truncated</span>
          <span v-if="entry.bytes !== null" class="shrink-0 font-mono text-[10.5px] text-faint">
            {{ formatBytes(entry.bytes) }}
          </span>
          <button
            class="shrink-0 rounded-[5px] px-2 py-0.5 text-[11.5px] text-faint hover:bg-raised hover:text-fg"
            @click="emit('jump', entry.row)"
          >
            {{ entry.tool }}
          </button>
        </div>
      </template>

      <p v-if="reading" class="px-3.5 py-2 text-[11.5px] text-faint">reading…</p>

      <div v-if="artifact" class="mx-2 mb-2 overflow-hidden rounded-[10px] border border-line bg-surface">
        <div class="flex items-center gap-2 px-3 py-1.5">
          <span class="min-w-0 flex-1 truncate font-mono text-[10.5px] text-faint" :title="artifact.path">
            {{ artifact.path }}
          </span>
          <span class="shrink-0 font-mono text-[10.5px] text-faint">{{ formatBytes(artifact.bytes) }}</span>
          <button
            class="shrink-0 rounded-[5px] px-2 py-0.5 text-[11.5px] text-dim hover:bg-raised hover:text-fg"
            @click="copy(artifact.text)"
          >
            copy
          </button>
          <button
            class="shrink-0 rounded-[5px] px-2 py-0.5 text-[11.5px] text-dim hover:bg-raised hover:text-fg"
            @click="artifact = null"
          >
            close
          </button>
        </div>
        <pre class="max-h-96 overflow-auto whitespace-pre-wrap px-3 pb-2.5 font-mono text-[11px] text-dim">{{ artifact.text }}</pre>
        <p v-if="artifact.truncated" class="border-t border-line px-3 py-1.5 text-[11px] text-warn">
          only the first {{ formatBytes(artifact.text.length) }} of {{ formatBytes(artifact.bytes) }} are shown
        </p>
      </div>
    </div>
  </div>
</template>
