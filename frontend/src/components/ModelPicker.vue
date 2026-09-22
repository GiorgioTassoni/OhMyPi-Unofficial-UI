<script setup lang="ts">
/**
 * The model picker (`docs/12` §7.1).
 *
 * Two measured properties of `get_available_models` shape this file: the response
 * is ~1.5 MB and, on a cold process, does not answer for tens of seconds. So the
 * picker renders whatever catalogue the host already has — immediately, even when
 * that is nothing — and shows a fetch in flight as a subtle state rather than as
 * an empty list, because "no models" and "not fetched yet" are different screens
 * to a user and only one of them is worth waiting on.
 *
 * It selects nothing itself: a pick, a reorder and a close all leave as events, and
 * the parent does the `set_model` and the star write, so a failure can be shown
 * inline against the popover that caused it instead of reverting quietly.
 *
 * Every rule about *what* is visible — grouping, ordering, search, favourites — is
 * in `lib/models.ts`, pure and asserted. This file arranges what those functions
 * decide, and owns the three pieces of state a popover has to hold: the query, the
 * keyboard highlight, and the row a drag is over.
 */
import { computed, onMounted, ref, watch } from "vue";
import {
  type ModelOption,
  modelKey,
  moveFavourite,
  providerGlyph,
  toggleFavourite,
  visibleGroups,
} from "../lib/models";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  /** The cached catalogue, as the host holds it. */
  models: ModelOption[];
  /** Favourite keys (`provider/id`), in the user's own order. */
  favourites: string[];
  /** The session's model as a key, or `null` when it reports none. */
  current: string | null;
  /** A fetch is in flight. */
  refreshing: boolean;
}>();

const emit = defineEmits<{
  (event: "select", payload: { provider: string; modelId: string }): void;
  (event: "favourites", keys: string[]): void;
  (event: "close"): void;
}>();

interface Row {
  model: ModelOption;
  key: string;
  /** Position in the flat list the arrow keys walk. */
  index: number;
}

interface Section {
  header: string;
  /** Favourites are the rows that drag and `⌥↑↓` apply to. */
  favourite: boolean;
  rows: Row[];
}

const search = ref<HTMLInputElement | null>(null);
const query = ref("");
/** The keyboard cursor, as an index into `flat`. */
const highlighted = ref(0);
/** The favourite being dragged, and the favourite row a drop would land on. */
const dragging = ref<string | null>(null);
const dropTarget = ref<string | null>(null);

/**
 * The rendered sections, each row carrying its place in the flat order.
 *
 * One pass builds both: the sections the template renders and the flat list the
 * arrow keys walk. Deriving the flat list from the sections instead would let the
 * two disagree, which is the bug where `Enter` selects the row above the highlight.
 */
const sections = computed<Section[]>(() => {
  const visible = visibleGroups(props.models, query.value, props.favourites);
  const sections: Section[] = [];
  let index = 0;

  if (visible.favourites.length > 0) {
    sections.push({
      header: "Favourites",
      favourite: true,
      rows: visible.favourites.map((model) => ({
        model,
        key: modelKey(model),
        index: index++,
      })),
    });
  }

  for (const group of visible.groups) {
    sections.push({
      header: `${providerGlyph(group.provider)} ${group.provider}`,
      favourite: false,
      rows: group.models.map((model) => ({ model, key: modelKey(model), index: index++ })),
    });
  }

  return sections;
});

const flat = computed(() => sections.value.flatMap((section) => section.rows));

/**
 * The highlighted index, kept inside the list.
 *
 * The catalogue can be replaced under an open popover (a fetch landing), so a
 * stored index is only ever a wish: clamping here costs one comparison and means
 * `Enter` cannot select a row that is no longer there.
 */
const activeIndex = computed(() => Math.min(highlighted.value, flat.value.length - 1));

const emptyLabel = computed(() => {
  const needle = query.value.trim();
  if (needle !== "") {
    return `No model matches “${needle}”.`;
  }
  // The catalogue arrives cached, so an empty list while a fetch runs is a state
  // to name, not a result to report.
  return props.refreshing ? "Fetching the catalogue…" : "No models available.";
});

// A new search starts at the top: the old index pointed into a different list.
watch(query, () => {
  highlighted.value = 0;
});

onMounted(() => {
  search.value?.focus();
});

function isCurrent(model: ModelOption): boolean {
  return props.current !== null && modelKey(model) === props.current;
}

function isFavourite(key: string): boolean {
  return props.favourites.includes(key);
}

/**
 * The payload the parent sends as `set_model{provider, modelId}`.
 *
 * Named rather than inlined at both call sites (click and Enter) because it is a
 * contract with the host: the provider and the id, never the composed key.
 */
function select(row: Row): void {
  emit("select", { provider: row.model.provider, modelId: row.model.id });
}

/**
 * Move a favourite, and take the highlight with it.
 *
 * Following the row is what makes the key repeatable: leaving the highlight on the
 * index would aim the next `⌥↓` at whichever model slid into the gap.
 */
function reorder(key: string, delta: number): void {
  const from = props.favourites.indexOf(key);
  if (from < 0) {
    return;
  }

  const moved = moveFavourite(props.favourites, key, delta);
  const to = moved.indexOf(key);
  if (to === from) {
    // Past an end: `moveFavourite` clamped, so emitting would write back the list
    // the host already has.
    return;
  }

  emit("favourites", moved);
  // Favourites are the first rows of the flat list, so a stored position is also
  // the flat index.
  highlighted.value = to;
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
    return;
  }

  if (event.key === "Enter") {
    const row = flat.value[activeIndex.value];
    if (row === undefined) {
      return;
    }
    event.preventDefault();
    select(row);
    return;
  }

  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
    return;
  }

  event.preventDefault();
  const delta = event.key === "ArrowDown" ? 1 : -1;

  if (!event.altKey) {
    highlighted.value = Math.min(
      Math.max(activeIndex.value + delta, 0),
      flat.value.length - 1,
    );
    return;
  }

  // `⌥↑↓` reorders the highlighted row. It is handled here as well as on the row
  // because focus normally sits in the search field — the combobox pattern — where
  // the highlight *is* the keyboard cursor.
  const row = flat.value[activeIndex.value];
  if (row !== undefined) {
    reorder(row.key, delta);
  }
}

/**
 * `⌥↑↓` on a focused favourite row.
 *
 * Row-level so that a row reached by click or Tab can be reordered without moving
 * the highlight first; it stops there so the root handler cannot move the same row
 * a second time from the event's bubble.
 */
function onRowKeydown(event: KeyboardEvent, favourite: boolean, key: string): void {
  if (!favourite || !event.altKey) {
    return;
  }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
    return;
  }

  event.preventDefault();
  event.stopPropagation();
  reorder(key, event.key === "ArrowDown" ? 1 : -1);
}

function onDragStart(event: DragEvent, key: string): void {
  dragging.value = key;
  // Truthy rather than `!== null`: the payload is optional in practice (a
  // synthesised event, a platform that starts the drag without one), and losing
  // the reorder over it would be silly.
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
    // Some platforms refuse to start a drag with an empty payload.
    event.dataTransfer.setData("text/plain", key);
  }
}

function endDrag(): void {
  dragging.value = null;
  dropTarget.value = null;
}

function onDragOver(event: DragEvent, key: string): void {
  if (dragging.value === null) {
    return;
  }
  // Only a row that claims the drop can receive it.
  event.preventDefault();
  dropTarget.value = key;
}

/** `@drop.prevent` on the row already suppressed the platform's own handling. */
function onDrop(key: string): void {
  const from = dragging.value;
  endDrag();

  if (from === null || from === key) {
    return;
  }

  // Both indices are in the *stored* list, not the rendered one, so a stored key
  // with no catalogue row between them cannot shift the result by one.
  const delta = props.favourites.indexOf(key) - props.favourites.indexOf(from);
  reorder(from, delta);
}
</script>

<template>
  <!-- The picker is the body of a popover, so it draws no card of its own: the search field
       is the only raised surface here, and the rows are rows. -->
  <div class="flex flex-col" @keydown="onKeydown">
    <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-2">
      <Icon name="search" class="h-3.5 w-3.5 shrink-0 text-faint" />
      <input
        ref="search"
        v-model="query"
        type="text"
        spellcheck="false"
        placeholder="Search models"
        aria-label="Search models"
        class="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
      />

      <!--
        The fetch, as a state: opening the picker never waits for the network, so
        this says "there may be more in a moment" without emptying a list that is
        still perfectly usable.
      -->
      <span v-if="refreshing" class="flex shrink-0 items-center gap-1.5 text-[11px] text-faint">
        <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
        refreshing
      </span>
    </div>

    <div role="listbox" aria-label="Models" class="mt-1.5 max-h-80 overflow-y-auto">
      <p v-if="flat.length === 0" class="px-2.5 py-4 text-[12.5px] text-faint">{{ emptyLabel }}</p>

      <section v-for="section in sections" :key="section.header" class="pt-2 first:pt-0.5">
        <div class="flex items-baseline gap-2 px-2.5 pb-1">
          <span class="text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            {{ section.header }}
          </span>
          <!-- Only the favourites drag, so only they carry the hint. -->
          <span v-if="section.favourite" class="ml-auto text-[10.5px] text-faint">
            drag or ⌥↑↓ to reorder
          </span>
        </div>

        <div
          v-for="entry in section.rows"
          :key="entry.key"
          role="option"
          :aria-selected="entry.index === activeIndex"
          :tabindex="section.favourite ? 0 : -1"
          class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-2"
          :class="[
            entry.index === activeIndex ? 'bg-selected' : 'hover:bg-raised',
            dropTarget === entry.key ? 'ring-1 ring-accent ring-inset' : '',
          ]"
          @click="select(entry)"
          @keydown="onRowKeydown($event, section.favourite, entry.key)"
          @dragover="onDragOver($event, entry.key)"
          @drop.prevent="onDrop(entry.key)"
        >
          <span
            v-if="section.favourite"
            class="shrink-0 cursor-grab text-faint hover:text-dim"
            title="Drag to reorder"
            draggable="true"
            @dragstart="onDragStart($event, entry.key)"
            @dragend="endDrag"
          >
            <Icon name="grip" class="h-3.5 w-3.5" />
          </span>

          <span class="w-4 shrink-0 font-mono text-[10.5px] text-faint">
            {{ providerGlyph(entry.model.provider) }}
          </span>

          <span
            class="min-w-0 flex-1 truncate text-[13px]"
            :class="isCurrent(entry.model) ? 'text-accent' : 'text-fg'"
          >
            {{ entry.model.name }}
          </span>

          <span class="flex shrink-0 items-center gap-2">
            <!--
              The model's own suggestion, not the session's level: the tag is a hint
              for choosing, while the Effort chip shows what the session actually runs.
            -->
            <span
              v-if="entry.model.defaultEffort"
              class="text-[11px] text-faint"
              :title="`${entry.model.name} suggests ${entry.model.defaultEffort}; the session's level is on the Effort chip`"
            >
              {{ entry.model.defaultEffort }}
            </span>

            <span v-if="isCurrent(entry.model)" title="current model">
              <Icon name="check" class="h-3.5 w-3.5 text-accent" />
            </span>

            <!-- A press here toggles the star and nothing else: the row's own click would
                 otherwise select the model the press was aiming past. -->
            <button
              class="shrink-0 rounded-[6px] p-0.5"
              :title="isFavourite(entry.key) ? 'Remove from favourites' : 'Add to favourites'"
              :aria-pressed="isFavourite(entry.key)"
              @click.stop="emit('favourites', toggleFavourite(favourites, entry.key))"
            >
              <Icon
                name="star"
                :solid="isFavourite(entry.key)"
                class="h-3.5 w-3.5"
                :class="isFavourite(entry.key) ? 'text-accent-bright' : 'text-faint'"
              />
            </button>
          </span>
        </div>
      </section>
    </div>

    <p class="px-2.5 pb-0.5 pt-2 text-[10.5px] text-faint">
      ↑↓ move · ⏎ select · ⌥↑↓ reorder · esc close
    </p>
  </div>
</template>
