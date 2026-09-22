<script setup lang="ts">
/**
 * The settings rail (`docs/12` §12, the reference's left column): a back control, the search
 * field, and the sections.
 *
 * The search field lives here rather than over the content because that is what it searches
 * *across*: a query returns rows from every section, grouped by the section that owns them, so
 * the results belong where the sections are. Selecting a hit navigates and highlights rather
 * than filtering the page, because the answer to "where is this setting" is a place, not a
 * list.
 *
 * The identity row at the bottom is the sidebar's, kept: the window's own identity should not
 * disappear because the user is looking at settings.
 */
import type { SettingsRow } from "../../bridge";
import { hitCount, rowId, sectionIcon, type NavGroup, type SearchGroup } from "../../lib/settings";
import Icon from "../ui/Icon.vue";

const props = defineProps<{
  groups: NavGroup[];
  /** The section on screen. */
  active: string;
  query: string;
  /** The hits, already grouped by the section that owns them. */
  hits: SearchGroup[];
  /** The OS user and app version, for the footer row. */
  identity: { user: string | null; version: string };
  /** How many sessions the catalogue holds. */
  sessions: number;
}>();

const emit = defineEmits<{
  back: [];
  /** A section row was pressed. */
  select: [sectionId: string];
  /** The search field changed. */
  query: [text: string];
  /** A hit was chosen: go to its section and put its row under the reader's eye. */
  hit: [sectionId: string, rowId: string];
}>();

/** The hit's secondary line: a setting's key, or nothing for the other two roles. */
function hitKey(row: SettingsRow): string | null {
  return row.role === "setting" ? row.key : null;
}
</script>

<template>
  <aside class="flex w-64 shrink-0 flex-col bg-rail">
    <div class="flex flex-col gap-0.5 p-2">
      <button
        class="group flex items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] text-dim hover:bg-raised hover:text-fg"
        title="back to the thread view (⌘,)"
        data-action="settings-back"
        @click="emit('back')"
      >
        <Icon name="arrow-left" class="h-4 w-4 text-faint group-hover:text-dim" />
        Settings
      </button>
    </div>

    <div class="px-2 pb-1">
      <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-1.5">
        <Icon name="search" class="h-3.5 w-3.5 shrink-0 text-faint" />
        <input
          :value="props.query"
          type="text"
          spellcheck="false"
          placeholder="Search settings"
          aria-label="search settings"
          class="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
          data-input="settings-search"
          @input="emit('query', ($event.target as HTMLInputElement).value)"
        />
        <button
          v-if="props.query !== ''"
          type="button"
          class="shrink-0 text-faint hover:text-fg"
          title="clear the search"
          data-action="settings-search-clear"
          @click="emit('query', '')"
        >
          <Icon name="close" class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <nav class="min-h-0 flex-1 overflow-auto px-2 pb-2">
      <!-- Hits: grouped by section, in the catalog's own order. -->
      <template v-if="props.query.trim() !== ''">
        <p class="px-1.5 pb-1 pt-2 text-[10.5px] text-faint" data-hits="">
          {{ hitCount(props.hits) }} {{ hitCount(props.hits) === 1 ? "setting" : "settings" }}
        </p>

        <div v-for="group in props.hits" :key="group.section.id" class="mb-1">
          <p class="px-1.5 pb-0.5 pt-1.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            {{ group.section.title }}
          </p>
          <button
            v-for="hit in group.rows"
            :key="rowId(hit.row)"
            class="flex w-full flex-col gap-0.5 rounded-[6px] px-2.5 py-1.5 text-left hover:bg-raised"
            data-hit=""
            :data-row-key="rowId(hit.row)"
            @click="emit('hit', group.section.id, rowId(hit.row))"
          >
            <span class="truncate text-[12.5px] text-fg">{{ hit.row.label }}</span>
            <span v-if="hitKey(hit.row) !== null" class="truncate font-mono text-[10px] text-faint">
              {{ hitKey(hit.row) }}
            </span>
          </button>
        </div>

        <p v-if="props.hits.length === 0" class="px-1.5 py-2 text-[12px] text-faint">
          No setting matches “{{ props.query.trim() }}”.
        </p>
      </template>

      <!-- The nav, grouped into the catalog's three columns. -->
      <template v-else>
        <div v-for="group in props.groups" :key="group.id" class="mb-1" :data-nav-group="group.id">
          <p class="px-1.5 pb-0.5 pt-2 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            {{ group.label }}
          </p>
          <button
            v-for="section in group.sections"
            :key="section.id"
            class="group flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px]"
            :class="section.id === props.active ? 'bg-selected text-fg' : 'text-dim hover:bg-raised hover:text-fg'"
            :data-section="section.id"
            @click="emit('select', section.id)"
          >
            <Icon
              :name="sectionIcon(section.icon)"
              class="h-4 w-4 shrink-0"
              :class="section.id === props.active ? 'text-accent' : 'text-faint group-hover:text-dim'"
            />
            <span class="min-w-0 flex-1 truncate">{{ section.title }}</span>
          </button>
        </div>
      </template>
    </nav>

    <!-- The identity row, the same one the sidebar's footer carries (`docs/12` §2.2). -->
    <div class="mt-auto flex items-center gap-2.5 px-3 py-2.5">
      <span
        class="grid h-7 w-7 shrink-0 place-items-center rounded-full bg-raised text-[13px] text-accent"
        aria-hidden="true"
      >
        π
      </span>
      <div class="min-w-0 flex-1">
        <p class="truncate text-[12.5px] text-fg">{{ props.identity.user ?? "local" }}</p>
        <p class="truncate text-[11px] text-faint">
          {{ props.sessions }} {{ props.sessions === 1 ? "session" : "sessions" }} · v{{
            props.identity.version
          }}
        </p>
      </div>
    </div>
  </aside>
</template>
