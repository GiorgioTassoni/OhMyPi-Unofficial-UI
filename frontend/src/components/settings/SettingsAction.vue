<script setup lang="ts">
/**
 * An action row's control: a button that *does* something rather than storing something
 * (`docs/12` §12).
 *
 * The host names the action (`open-mode-menu`, `open-config-file`) and the screen owns the
 * verb, because the verb is a UI decision: the app's own config file is reached by the app's
 * own raw-config section rather than by handing the path to the OS, and the approval mode is
 * changed where its confirmation lives — the mode popover, which writes the key *and* restarts
 * the sidecar after any running turn finishes.
 *
 * An action this build has never heard of says so, in place of a button. A control that did
 * nothing when pressed would be indistinguishable from one that worked.
 */
import { inject, ref } from "vue";

import type { SettingsActionRow } from "../../bridge";
import Icon from "../ui/Icon.vue";
import ModePopover from "../ModePopover.vue";
import Popover from "../Popover.vue";
import { SETTINGS_CONTEXT } from "./context";

const props = defineProps<{ row: SettingsActionRow }>();

const emit = defineEmits<{
  /** The screen dispatches on the id: it owns the navigation and the notices. */
  (event: "dispatch", action: string): void;
  /** Something the app owns changed, so the screen re-reads itself and the session. */
  (event: "changed"): void;
}>();

const context = inject(SETTINGS_CONTEXT);
if (context === undefined) {
  throw new Error("SettingsAction is only usable inside the settings screen");
}

/** The verbs this build knows, by the host's action id. */
const VERBS: Partial<Record<string, string>> = {
  "open-mode-menu": "Change mode",
  "open-config-file": "Open the raw config",
};

const open = ref(false);
const failure = ref<string | null>(null);

function describe(cause: unknown): string {
  return typeof cause === "string" ? cause : cause instanceof Error ? cause.message : String(cause);
}
</script>

<template>
  <div class="flex flex-col items-end gap-1">
    <Popover
      v-if="props.row.action === 'open-mode-menu'"
      :open="open"
      label="approval mode"
      :width="320"
      @close="open = false"
    >
      <template #trigger>
        <button
          type="button"
          :disabled="context.thread.value === null"
          class="flex items-center gap-2 rounded-[6px] border px-2.5 py-1 text-[11.5px]"
          :class="
            props.row.tone === 'danger'
              ? 'border-err/50 text-err hover:bg-err/10'
              : 'border-line-strong text-fg hover:bg-raised'
          "
          :title="context.thread.value === null ? 'open a thread first — the mode is per session' : ''"
          :data-action="props.row.action"
          @click="open = !open"
        >
          {{ VERBS[props.row.action] }}
          <Icon name="chevron-down" class="h-3 w-3" />
        </button>
      </template>

      <ModePopover
        :thread="context.thread.value ?? ''"
        :open="open"
        :mode="context.mode.value"
        @close="open = false"
        @changed="emit('changed')"
        @failed="failure = describe($event)"
      />
    </Popover>

    <button
      v-else-if="VERBS[props.row.action] !== undefined"
      type="button"
      class="rounded-[6px] border px-2.5 py-1 text-[11.5px]"
      :class="
        props.row.tone === 'danger'
          ? 'border-err/50 text-err hover:bg-err/10'
          : 'border-line-strong text-fg hover:bg-raised'
      "
      :data-action="props.row.action"
      @click="emit('dispatch', props.row.action)"
    >
      {{ VERBS[props.row.action] }}
    </button>

    <span v-else class="max-w-[220px] text-right text-[11.5px] text-faint">
      no control in this build for `{{ props.row.action }}`
    </span>

    <p v-if="failure !== null" class="max-w-[220px] text-right text-[10.5px] text-err">
      {{ failure }}
    </p>
  </div>
</template>
