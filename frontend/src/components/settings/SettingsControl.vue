<script setup lang="ts">
/**
 * The control in a setting row's right-hand column (`docs/12` §12).
 *
 * One switch over the host's `control` name rather than seven inline blocks, so a row's
 * anatomy is written once: the widget, and — under it — the refusal *this* control produced.
 * There are two kinds of refusal in a row and they belong in different places: "that is not a
 * number" is about the box the user is typing in, and the engine's own words after a write are
 * about the setting, which is why the first is drawn here and the second by the row.
 *
 * A control the host names and this build does not know draws the value read-only with that
 * said out loud: a stale frontend must not silently render nothing where a setting is.
 */
import { computed, ref } from "vue";

import type { SettingsSettingRow } from "../../bridge";
import { numberValue } from "../../lib/settings";
import Toggle from "../ui/Toggle.vue";
import SettingsField from "./SettingsField.vue";
import SettingsList from "./SettingsList.vue";
import SettingsRecord from "./SettingsRecord.vue";
import SettingsSecret from "./SettingsSecret.vue";
import SettingsSelect from "./SettingsSelect.vue";

const props = defineProps<{
  row: SettingsSettingRow;
  pending: boolean;
}>();

const emit = defineEmits<{
  (event: "write", value: unknown): void;
  (event: "reset"): void;
}>();

/** What this control refused, shown under it until the next write. */
const invalid = ref<string | null>(null);
const wide = computed(() => props.row.control === "list" || props.row.control === "record");

function write(value: unknown): void {
  invalid.value = null;
  emit("write", value);
}

function commitNumber(text: string): void {
  const parsed = numberValue(text);
  if (!parsed.ok) {
    invalid.value = parsed.why;
    return;
  }
  write(parsed.value);
}
</script>

<template>
  <div class="flex flex-col gap-1" :class="wide ? 'w-full items-stretch' : 'items-end'">
    <Toggle
      v-if="props.row.control === 'toggle'"
      :checked="props.row.value === true"
      :disabled="props.pending"
      :label="props.row.label"
      @change="write"
    />

    <SettingsSelect
      v-else-if="props.row.control === 'select'"
      :choices="props.row.choices"
      :value="props.row.value"
      :disabled="props.pending"
      :label="props.row.label"
      @write="write"
    />

    <SettingsField
      v-else-if="props.row.control === 'number'"
      :value="props.row.value"
      :mono="true"
      :disabled="props.pending"
      :error="invalid"
      width="w-[120px]"
      placeholder="not set"
      @commit="commitNumber"
    />

    <SettingsField
      v-else-if="props.row.control === 'text'"
      :value="props.row.value"
      :disabled="props.pending"
      width="w-[240px]"
      placeholder="not set"
      @commit="write"
    />

    <SettingsSecret
      v-else-if="props.row.control === 'secret'"
      :present="props.row.present"
      :redacted="props.row.redacted"
      :key-name="props.row.key"
      :disabled="props.pending"
      @write="write"
      @reset="emit('reset')"
    />

    <SettingsList
      v-else-if="props.row.control === 'list'"
      :value="props.row.value"
      :disabled="props.pending"
      :label="props.row.label"
      @write="write"
      @invalid="invalid = $event"
    />

    <SettingsRecord
      v-else-if="props.row.control === 'record'"
      :key-name="props.row.key"
      :value="props.row.value"
      :disabled="props.pending"
      :label="props.row.label"
      @write="write"
      @invalid="invalid = $event"
    />

    <span v-else class="max-w-[240px] text-right font-mono text-[11.5px] text-faint">
      this build has no `{{ props.row.control }}` control — use the raw config
    </span>

    <p
      v-if="invalid !== null"
      class="text-[10.5px] text-err"
      :class="wide ? 'w-full text-left' : 'max-w-[240px] text-right'"
    >
      {{ invalid }}
    </p>
  </div>
</template>
