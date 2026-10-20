<script setup lang="ts">
/**
 * The command palette's panel (`docs/12` §7.3).
 *
 * Presentation only: what is showing and which row is selected arrive as props, because
 * both are consequences of the draft (`lib/palette.ts`) rather than state this component
 * could own without keeping a second copy of it in step.
 *
 * Grouped by source so a user-authored command is visibly not a builtin — the reference's
 * grouping, and the reason `source` travels with every row.
 */
import { computed } from "vue";
import { itemDetail, itemKey, itemLabel, type PaletteItem, type PaletteView } from "../lib/palette";
import type { IconName } from "../lib/icons";
import Icon from "./ui/Icon.vue";

/**
 * The glyph for an app row, by the id the shell dispatches on.
 *
 * A row whose action is a thing with an icon in this window should look like that thing; one
 * this build has no glyph for gets the generic mark rather than an empty column.
 */
const APP_ICONS: Record<string, IconName | undefined> = {
  settings: "gear",
  "add-skill": "sparkle",
  "add-provider": "plug",
  help: "info",
  clear: "trash",
};

const props = defineProps<{
  view: PaletteView;
  /** The highlighted row's index into `view.items`. */
  selected: number;
}>();

const emit = defineEmits<{
  (event: "pick", index: number): void;
  (event: "hover", index: number): void;
}>();

/**
 * The groups, each row carrying its index in the flat keyboard order.
 *
 * A running counter rather than a search per row: `view.items` is the groups flattened in
 * this same order (`paletteFor` builds it that way), so the index is the position, and a
 * lookup would be the same number computed more slowly.
 */
const rows = computed(() => {
  let index = 0;

  return props.view.groups.map((group) => ({
    source: group.source,
    items: group.items.map((item) => ({ item, index: index++ })),
  }));
});

/**
 * The row's other names, as one faint label.
 *
 * The group heading already names the source, so what is worth putting beside the command is
 * how else it can be typed. Subcommands have no aliases of their own.
 */
function itemAliases(item: PaletteItem): string | null {
  return item.kind === "command" && item.command.aliases.length > 0
    ? item.command.aliases.join(", ")
    : null;
}

/** Whether the row opens another list — which the chevron on its right says. */
function hasSubcommands(item: PaletteItem): boolean {
  return item.kind === "command" && item.command.subcommands.length > 0;
}
</script>

<template>
  <!--
    The list hangs inside the composer's card rather than being a card of its own: it is
    recessed into the surface, with rows and headings doing all the dividing.
  -->
  <div
    class="mb-2 max-h-72 overflow-auto rounded-[8px] border border-line/50 bg-canvas/80 p-1.5 shadow-lg shadow-black/30"
    role="dialog"
    aria-label="commands"
  >
    <div v-for="group in rows" :key="group.source">
      <p class="flex items-center gap-1.5 px-2 pb-1 pt-2 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        <Icon v-if="group.source === 'skill'" name="zap" class="h-3 w-3 text-accent" />
        <span>{{ group.source === 'skill' ? 'Skills' : group.source }}</span>
      </p>

      <button
        v-for="row in group.items"
        :key="itemKey(row.item)"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left transition-colors"
        :class="row.index === selected ? 'bg-selected text-fg font-medium' : 'text-dim hover:bg-raised/70 hover:text-fg'"
        @click="emit('pick', row.index)"
        @mousemove="emit('hover', row.index)"
      >
        <Icon
          v-if="row.item.kind === 'app'"
          :name="APP_ICONS[row.item.app.id] ?? 'sliders'"
          class="h-3.5 w-3.5 shrink-0 text-faint"
        />
        <Icon
          v-else-if="row.item.kind === 'command' && row.item.command.source === 'skill'"
          name="zap"
          class="h-3.5 w-3.5 shrink-0 text-accent"
        />
        <span class="shrink-0 text-[13px] text-fg">{{ itemLabel(row.item) }}</span>
        <span v-if="itemAliases(row.item)" class="shrink-0 text-[11px] text-faint">
          {{ itemAliases(row.item) }}
        </span>
        <span v-if="itemDetail(row.item)" class="min-w-0 flex-1 truncate text-[11.5px] text-dim">
          {{ itemDetail(row.item) }}
        </span>
        <span v-else class="flex-1" />

        <!-- An app row is reachable without the palette, and saying how is the honest hint. -->
        <kbd
          v-if="row.item.kind === 'app' && row.item.app.key !== null"
          class="shrink-0 font-mono text-[10.5px] text-faint rounded bg-raised px-1.5 py-0.5 border border-line/50"
        >
          {{ row.item.app.key }}
        </kbd>

        <Icon
          v-if="hasSubcommands(row.item)"
          name="chevron-right"
          class="h-3.5 w-3.5 shrink-0 text-faint"
        />
      </button>
    </div>

    <p class="px-2 pb-0.5 pt-1.5 text-[10.5px] text-faint">
      <template v-if="view.level === 'subcommands'">
        {{ view.of }} · ↑↓ choose · enter runs · tab completes · esc closes
      </template>
      <template v-else>↑↓ choose · enter runs · tab completes · esc closes</template>
    </p>
  </div>
</template>
