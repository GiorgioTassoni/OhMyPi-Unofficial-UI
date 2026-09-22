<script setup lang="ts">
/**
 * Cross-thread search (`docs/12` §7.4).
 *
 * The query goes to the host's index, which ranks one record at a time; the *model* here
 * (`lib/search.ts`) turns that into threads with snippets, because a reader looking for "which
 * conversation was that?" wants threads, not records.
 *
 * Debounced, with the previous query's results left on screen while the next one is in
 * flight: an overlay that empties itself on every keystroke flickers, and one that blocks on
 * every keystroke is unusable on a store with thousands of records.
 */
import { onMounted, onUnmounted, ref, watch } from "vue";
import { indexStatus, onIndexProgress, search, type IndexStatus, type SearchHit, type SessionSummary } from "../bridge";
import { groupHits, kindLabel, snippet } from "../lib/search";
import Icon from "./ui/Icon.vue";
import Modal from "./Modal.vue";

const props = defineProps<{
  /** The catalogue, for the thread names a result shows. */
  sessions: SessionSummary[];
}>();

const emit = defineEmits<{
  pick: [thread: string, hit: SearchHit];
  close: [];
}>();

const query = ref("");
const hits = ref<SearchHit[]>([]);
const status = ref<IndexStatus | null>(null);
const busy = ref(false);
const failure = ref<string | null>(null);

let timer: ReturnType<typeof setTimeout> | null = null;
let unlisten: (() => void) | null = null;

onMounted(async () => {
  const input = document.querySelector<HTMLInputElement>("[aria-label='search every thread'] input");
  input?.focus();

  unlisten = await onIndexProgress((next) => {
    status.value = next;
  }).catch(() => null);
  status.value = await indexStatus().catch(() => null);
});

onUnmounted(() => {
  if (timer) clearTimeout(timer);
  unlisten?.();
});

watch(query, (next) => {
  if (timer) clearTimeout(timer);
  const needle = next.trim();
  if (needle === "") {
    hits.value = [];
    return;
  }
  timer = setTimeout(() => void run(needle), 120);
});

async function run(needle: string): Promise<void> {
  busy.value = true;
  try {
    const found = await search(needle);
    // A slower earlier query must not overwrite a faster later one.
    if (query.value.trim() !== needle) return;
    hits.value = found;
    failure.value = null;
  } catch (cause) {
    failure.value = typeof cause === "string" ? cause : String(cause);
  } finally {
    if (query.value.trim() === needle) busy.value = false;
  }
}

const groups = () => groupHits(hits.value, props.sessions);
</script>

<template>
  <Modal title="search every thread" @close="emit('close')">
    <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-2">
      <Icon name="search" class="h-3.5 w-3.5 shrink-0 text-faint" />
      <input
        v-model="query"
        spellcheck="false"
        placeholder="titles, messages, reasoning, tool calls and results"
        aria-label="search every thread"
        class="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
      />
    </div>

    <p v-if="failure" class="mt-2 text-[12px] text-err">{{ failure }}</p>

    <div v-else-if="query.trim() === ''" class="mt-3 text-[12.5px] text-faint">
      <!-- The reason wins over the count: "no sessions indexed yet" is the same words for an
           empty store and for an index that could not be read, and only one of those is true. -->
      <p v-if="status?.error" class="text-warn">{{ status.error }}</p>
      <p v-else-if="status && status.indexed === 0">
        no sessions indexed yet{{ status.running ? " — indexing now" : "" }}
      </p>
      <p v-else-if="status">
        <span class="text-dim">{{ status.indexed }} of {{ status.total }} sessions indexed.</span>
        Type to search across all of them.
      </p>
      <p v-else>the index is not available</p>
    </div>

    <p v-else-if="hits.length === 0 && !busy" class="mt-3 text-[12.5px] text-faint">
      nothing matches “{{ query }}”
    </p>

    <ul class="mt-3 space-y-3">
      <li v-for="group in groups()" :key="group.thread">
        <!-- The thread a hit belongs to leads its group; the hits under it are the matches. -->
        <button
          class="w-full rounded-[6px] px-2 py-1 text-left hover:bg-raised"
          @click="group.hits[0] && emit('pick', group.thread, group.hits[0])"
        >
          <span class="block truncate text-[12.5px] text-fg">{{ group.title }}</span>
        </button>
        <ul class="mt-0.5 flex flex-col">
          <li v-for="(hit, index) in group.hits" :key="`${hit.ordinal}-${index}`">
            <button
              class="flex w-full items-baseline gap-2 rounded-[6px] px-2 py-2 text-left hover:bg-raised"
              :title="`message ${hit.ordinal}`"
              @click="emit('pick', group.thread, hit)"
            >
              <span v-if="kindLabel(hit.kind)" class="shrink-0 text-[10.5px] text-faint">
                {{ kindLabel(hit.kind) }}
              </span>
              <span class="min-w-0 flex-1 text-[12px] text-dim">
                <template v-for="(part, partIndex) in [snippet(hit.text, query)]" :key="partIndex">
                  <!-- The matched run is the one word at full strength; the rest is context. -->
                  <template v-if="part">
                    <span>{{ part.before }}</span>
                    <span class="text-fg">{{ part.match }}</span>
                    <span>{{ part.after }}</span>
                  </template>
                  <template v-else>{{ hit.text.slice(0, 160) }}</template>
                </template>
              </span>
            </button>
          </li>
        </ul>
        <p v-if="group.more" class="px-2 pt-0.5 text-[10.5px] text-faint">
          more matches in this thread
        </p>
      </li>
    </ul>

    <p class="mt-3 text-[10.5px] text-faint">
      <span v-if="busy">searching…</span>
      <span v-else-if="status?.running">indexing {{ status.indexed }}/{{ status.total }} — results may be incomplete</span>
      <span v-else-if="status?.error" class="text-warn">{{ status.error }}</span>
      <span v-else>enter opens the thread · esc closes</span>
    </p>
  </Modal>
</template>
