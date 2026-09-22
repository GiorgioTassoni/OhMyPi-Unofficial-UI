<script setup lang="ts">
/**
 * A `list` control: the array's items, each editable in place, with add and remove
 * (`docs/12` §12).
 *
 * Every edit writes the **whole array**, because that is the shape the key takes — the host
 * validates an array for an array key, and a half-array would be a value the engine never
 * asked for. Order is the user's own: an item is added at the end and removed where it
 * stands, and nothing here sorts, dedupes or trims behind their back.
 *
 * An item that is not a string keeps its JSON and must parse back (`lib/settings.ts`
 * `itemValue`), so an array of numbers survives a visit to this editor instead of becoming
 * an array of their own spellings.
 */
import { computed } from "vue";

import { itemValue, listItems, textOf, type ListItem } from "../../lib/settings";
import Icon from "../ui/Icon.vue";
import SettingsField from "./SettingsField.vue";

const props = defineProps<{
  value: unknown;
  disabled?: boolean;
  /** The row's label, for the add button's title. */
  label: string;
}>();

const emit = defineEmits<{
  (event: "write", value: unknown): void;
  (event: "invalid", why: string): void;
}>();

const items = computed(() => listItems(props.value));

/** The array as it stands, as a mutable copy — every commit starts here. */
function current(): unknown[] {
  return Array.isArray(props.value) ? [...props.value] : [];
}

function replace(item: ListItem, text: string): void {
  const parsed = itemValue(text, item.literal);
  if (!parsed.ok) {
    emit("invalid", parsed.why);
    return;
  }
  const next = current();
  next[item.index] = parsed.value;
  emit("write", next);
}

function remove(index: number): void {
  emit(
    "write",
    current().filter((_, at) => at !== index),
  );
}

function add(text: string): void {
  const trimmed = text.trim();
  if (trimmed === "") return;
  emit("write", [...current(), trimmed]);
}
</script>

<template>
  <div class="flex w-full flex-col gap-1.5">
    <div v-for="item in items" :key="item.index" class="flex items-center gap-1.5" data-list-item="">
      <span class="w-4 shrink-0 text-right font-mono text-[10.5px] text-faint">{{ item.index + 1 }}</span>
      <SettingsField
        :value="textOf(item.text)"
        :mono="true"
        :disabled="props.disabled"
        width="w-full"
        @commit="(text: string) => replace(item, text)"
      />
      <button
        type="button"
        :disabled="props.disabled"
        class="shrink-0 rounded-[6px] p-1.5 text-faint transition-colors hover:bg-raised hover:text-err disabled:opacity-40"
        :title="`remove item ${item.index + 1}`"
        data-action="list-remove"
        @click="remove(item.index)"
      >
        <Icon name="trash" class="h-3.5 w-3.5" />
      </button>
    </div>

    <p v-if="items.length === 0" class="text-[11.5px] text-faint">The list is empty.</p>

    <!--
      The new item is a field of its own rather than a button that appends a blank row: an
      empty string in a list of paths is a value the engine would act on, so nothing is
      written until something has been typed.
    -->
    <div class="flex items-center gap-1.5">
      <span class="w-4 shrink-0 text-right text-[10.5px] text-faint">
        <Icon name="plus" class="inline h-3 w-3" />
      </span>
      <SettingsField
        :value="''"
        :mono="true"
        :disabled="props.disabled"
        :clear-after-commit="true"
        width="w-full"
        placeholder="add an item"
        @commit="add"
      />
    </div>
  </div>
</template>
