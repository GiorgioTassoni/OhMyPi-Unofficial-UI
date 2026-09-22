<script setup lang="ts">
/**
 * A menu opened at a point (`docs/12` §2.3), teleported and clamped the way `Popover` is:
 * the sidebar lives in a scrolling column, so an absolutely-positioned child would be
 * clipped exactly when the thread is near the bottom of the list.
 *
 * Disabled items are shown with their reason rather than hidden — a menu that silently
 * omits "Pin" teaches the user nothing, while one that says "open a thread first — the
 * engine writes its own pin set" explains the engine's own constraint.
 */
import { onMounted, onUnmounted, ref } from "vue";
import type { MenuAction, MenuItem } from "../lib/thread-menu";
import type { IconName } from "../lib/icons";
import Icon from "./ui/Icon.vue";

/**
 * The glyph beside each item.
 *
 * Labels alone make a reader scan a column of prose; the mark is what makes "Fork" and "Hand
 * off" distinguishable without reading. Every colour comes from the row's own text: an icon
 * that stayed `text-faint` while its label went red would split the row in two.
 */
const GLYPHS: Record<MenuAction, IconName> = {
  rename: "pencil",
  pin: "star",
  export: "download",
  fork: "branch",
  handoff: "external",
  reveal: "folder-open",
  "copy-cwd": "copy",
  stop: "stop",
  delete: "trash",
};

const props = defineProps<{
  items: MenuItem[];
  /** Where the press happened, in viewport coordinates. */
  at: { x: number; y: number };
  /** What the menu is about, for its label and for assistive technology. */
  label: string;
}>();

const emit = defineEmits<{
  pick: [action: MenuAction];
  close: [];
}>();

const panel = ref<HTMLElement | null>(null);
const placement = ref({ left: "0px", top: "0px" });

function place(): void {
  const width = panel.value?.offsetWidth ?? 220;
  const height = panel.value?.offsetHeight ?? 260;
  placement.value = {
    // Inside the viewport on both sides: a menu opened near an edge would otherwise run off
    // it, and the items past the edge would be unreachable.
    left: `${Math.min(Math.max(8, props.at.x), Math.max(8, window.innerWidth - width - 8))}px`,
    top: `${Math.min(Math.max(8, props.at.y), Math.max(8, window.innerHeight - height - 8))}px`,
  };
}

function onPointerDown(event: PointerEvent): void {
  const target = event.target as Node | null;
  if (target !== null && panel.value?.contains(target) === true) return;
  emit("close");
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

onMounted(() => {
  // Twice: the first pass positions a panel that has not been laid out yet, the second
  // clamps using its real size.
  place();
  requestAnimationFrame(place);
  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeydown, true);
});

onUnmounted(() => {
  document.removeEventListener("pointerdown", onPointerDown, true);
  document.removeEventListener("keydown", onKeydown, true);
});
</script>

<template>
  <Teleport to="body">
    <div
      ref="panel"
      role="menu"
      :aria-label="props.label"
      class="fixed z-40 min-w-56 overflow-hidden rounded-[10px] border border-line bg-surface p-1.5 shadow-xl shadow-black/50"
      :style="placement"
    >
      <template v-for="(item, index) in props.items" :key="item.action">
        <hr
          v-if="item.destructive && index > 0"
          class="my-1 border-t border-line"
        />
        <button
          role="menuitem"
          class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px] transition-colors disabled:cursor-not-allowed"
          :class="
            item.disabled
              ? 'text-faint/50 hover:bg-transparent'
              : item.destructive
                ? 'text-dim hover:bg-raised hover:text-err'
                : 'text-dim hover:bg-raised hover:text-fg'
          "
          :disabled="item.disabled"
          :title="item.reason"
          @click="emit('pick', item.action)"
        >
          <Icon :name="GLYPHS[item.action]" class="h-3.5 w-3.5 shrink-0" />
          <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
        </button>
      </template>
    </div>
  </Teleport>
</template>
