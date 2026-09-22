<script setup lang="ts">
/**
 * A `record` control: the map's entries, each editable, with add and remove (`docs/12` §12).
 *
 * A record's *keys* are its data — `task.agentAdvisor` is keyed by agent name, `modelRoles`
 * by role — so they are editable too, and a rename is a new map rather than a moved entry.
 * Like the list editor, every edit writes the whole record: that is the shape the key takes.
 *
 * `modelRoles` is deliberately narrower in v1: OMP owns the fixed role names, so they are
 * labels rather than editable record keys and entries cannot be added or removed. Only each
 * role's model is editable, through the model picker. Picking one keeps the `:high`-style
 * effort suffix the selector already carried, so the picker changes the model and nothing else.
 */
import { computed, inject, ref } from "vue";
import { itemValue, recordEntries, selectorFor, selectorModel, textOf } from "../../lib/settings";
import ModelPicker from "../ModelPicker.vue";
import Popover from "../Popover.vue";
import Icon from "../ui/Icon.vue";
import { SETTINGS_CONTEXT } from "./context";
import SettingsField from "./SettingsField.vue";

const props = defineProps<{
  /** The catalog key, which decides whether the value field gets the model picker. */
  keyName: string;
  value: unknown;
  disabled?: boolean;
  label: string;
}>();

const emit = defineEmits<{
  (event: "write", value: unknown): void;
  (event: "invalid", why: string): void;
}>();

const context = inject(SETTINGS_CONTEXT);
if (context === undefined) {
  throw new Error("SettingsRecord is only usable inside the settings screen");
}

/** The entry whose model is being picked, by key. One picker at a time, like every popover. */
const picking = ref<string | null>(null);
/** The entry being added: a key and a value, held until both are worth writing. */
const adding = ref(false);
const draftKey = ref("");
const draftValue = ref("");

const entries = computed(() => recordEntries(props.value));
const roles = computed(() => props.keyName === "modelRoles");

/** The record as it stands, as a mutable copy — every commit starts here. */
function current(): Record<string, unknown> {
  const value = props.value;
  if (value === null || typeof value !== "object" || Array.isArray(value)) return {};
  return { ...(value as Record<string, unknown>) };
}

function rename(from: string, to: string): void {
  if (roles.value) return;
  const name = to.trim();
  if (name === "" || name === from) return;
  const next = current();
  const held = next[from];
  delete next[from];
  next[name] = held;
  emit("write", next);
}

function setValue(key: string, text: string): void {
  const was = current()[key];
  const parsed = itemValue(text, typeof was !== "string");
  if (!parsed.ok) {
    emit("invalid", parsed.why);
    return;
  }
  emit("write", { ...current(), [key]: parsed.value });
}

function remove(key: string): void {
  if (roles.value) return;
  const next = current();
  delete next[key];
  emit("write", next);
}

function pick(key: string, picked: { provider: string; modelId: string }): void {
  picking.value = null;
  emit("write", { ...current(), [key]: selectorFor(picked.provider, picked.modelId, textOf(current()[key])) });
}

function commitAddition(): void {
  if (roles.value) return;
  const key = draftKey.value.trim();
  const value = draftValue.value.trim();
  if (key === "" || value === "") return;
  adding.value = false;
  draftKey.value = "";
  draftValue.value = "";
  emit("write", { ...current(), [key]: value });
}
</script>

<template>
  <div class="flex w-full flex-col gap-1.5">
    <div
      v-for="entry in entries"
      :key="entry.key"
      class="grid items-center gap-2"
      :class="roles
        ? 'grid-cols-[110px_minmax(0,1fr)]'
        : 'grid-cols-[minmax(140px,0.45fr)_minmax(0,1fr)_auto]'"
      data-record-entry=""
    >
      <span
        v-if="roles"
        class="truncate px-1 font-mono text-[12.5px] text-dim"
        :title="entry.key"
        data-record-key="fixed"
      >
        {{ entry.key }}
      </span>
      <SettingsField
        v-else
        :value="entry.key"
        :mono="true"
        :disabled="props.disabled"
        width="w-full"
        @commit="(text: string) => rename(entry.key, text)"
      />

      <!--
        The picker replaces the value field for a role, and shows a value the catalogue does
        not have as itself: a role pointing at a model this build has not fetched is a fact
        about the selector, not an invitation to rewrite it.
      -->
      <Popover
        v-if="roles"
        :open="picking === entry.key"
        :label="`${entry.key} model`"
        :width="360"
        @close="picking = null"
      >
        <template #trigger>
          <button
            type="button"
            :disabled="props.disabled"
            class="flex w-full min-w-0 items-center gap-2 rounded-[6px] border border-line/60 bg-raised px-2.5 py-1.5 text-left text-[12.5px] transition-colors hover:border-line-strong hover:bg-raised/80 disabled:opacity-40"
            data-control="role-model"
            @click="picking = entry.key"
          >
            <span
              class="min-w-0 flex-1 truncate font-mono text-fg"
              :title="textOf(entry.value)"
            >
              {{ textOf(entry.value) === "" ? "pick a model" : textOf(entry.value) }}
            </span>
            <Icon name="chevron-down" class="h-3.5 w-3.5 shrink-0 text-faint" />
          </button>
        </template>

        <ModelPicker
          :models="context.models.value"
          :favourites="context.favourites.value"
          :current="selectorModel(textOf(entry.value))"
          :refreshing="context.refreshing.value"
          @select="(picked) => pick(entry.key, picked)"
          @favourites="context.setFavourites"
          @close="picking = null"
        />
      </Popover>

      <SettingsField
        v-else
        :value="textOf(entry.value)"
        :mono="true"
        :disabled="props.disabled"
        width="w-full"
        @commit="(text: string) => setValue(entry.key, text)"
      />

      <button
        v-if="!roles"
        type="button"
        :disabled="props.disabled"
        class="shrink-0 rounded-[6px] p-1.5 text-faint transition-colors hover:bg-raised hover:text-err disabled:opacity-40"
        :title="`remove ${entry.key}`"
        data-action="record-remove"
        @click="remove(entry.key)"
      >
        <Icon name="trash" class="h-3.5 w-3.5" />
      </button>
    </div>

    <p v-if="entries.length === 0" class="text-[11.5px] text-faint">The record is empty.</p>

    <!--
      An entry is written only once it has both halves: a key with an empty value is a
      selector the engine would treat as unset, and the point of adding one is to set it.
    -->
    <div
      v-if="adding && !roles"
      class="grid grid-cols-[minmax(140px,0.45fr)_minmax(0,1fr)_auto] items-center gap-2"
    >
      <input
        :ref="(input) => (input as HTMLInputElement | null)?.focus()"
        v-model="draftKey"
        type="text"
        spellcheck="false"
        placeholder="key"
        aria-label="new key"
        class="w-full rounded-[6px] border border-line/60 bg-raised px-2.5 py-1.5 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint transition-colors focus:border-accent/80 focus:ring-1 focus:ring-accent/40"
        data-input="record-key"
        @keydown.enter.prevent="commitAddition"
      />
      <input
        v-model="draftValue"
        type="text"
        spellcheck="false"
        placeholder="value"
        aria-label="new value"
        class="w-full rounded-[6px] border border-line/60 bg-raised px-2.5 py-1.5 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint transition-colors focus:border-accent/80 focus:ring-1 focus:ring-accent/40"
        data-input="record-value"
        @keydown.enter.prevent="commitAddition"
      />
      <button
        type="button"
        :disabled="props.disabled"
        class="shrink-0 rounded-[6px] border border-line bg-surface px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-40"
        data-action="record-add"
        @click="commitAddition"
      >
        Add
      </button>
    </div>

    <button
      v-else-if="!roles"
      type="button"
      :disabled="props.disabled"
      class="flex items-center gap-1.5 self-start rounded-[6px] border border-line/40 bg-surface/40 px-2 py-1 text-[11.5px] text-faint transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-40"
      :title="`add an entry to ${props.label}`"
      data-action="record-new"
      @click="(adding = true), (draftKey = ''), (draftValue = '')"
    >
      <Icon name="plus" class="h-3.5 w-3.5" />
      Add entry
    </button>
  </div>
</template>
