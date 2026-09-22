<script setup lang="ts">
/**
 * The confirmation a dangerous write needs (`docs/13` §Q7, `docs/12` §12).
 *
 * The host refuses such a write unless `confirmation` is the key's own name, so this dialog is
 * the *only* thing that can produce one — and it cannot be a plain "are you sure", because the
 * whole point of typing the name is that it cannot be clicked through. A cancelled dialog
 * writes nothing: the caller performs the write, and this component only reports what was
 * typed, which is also why it holds no reference to the write itself.
 *
 * No shipped catalog row is `confirm`-level today (the five keys the mapping document marks
 * dangerous are `hidden`, so the curated screen has no row for them). The path stays because it
 * is the guard the document asks for, and because a future catalog may surface one on purpose.
 */
import { computed, ref } from "vue";

import Modal from "../Modal.vue";

const props = defineProps<{
  /** The setting's own name — what has to be typed. */
  keyName: string;
  /** The row's title, so the dialog says which row it is about. */
  label: string;
  /** Why the key is dangerous, in the mapping document's words. */
  why: string;
}>();

const emit = defineEmits<{
  (event: "cancel"): void;
  (event: "confirm", typed: string): void;
}>();

const typed = ref("");

/** The host trims before comparing, so the button agrees with what it will do. */
const matches = computed(() => typed.value.trim() === props.keyName);
</script>

<template>
  <!-- Never busy: the caller closes this dialog *before* it writes, so there is no state in
       which a write is in flight behind an open confirmation. -->
  <Modal :title="`Confirm ${props.label}`" @close="emit('cancel')">
    <div class="flex flex-col gap-3">
      <p class="text-[12.5px] leading-relaxed text-dim">{{ props.why }}</p>

      <p class="text-[11.5px] leading-relaxed text-faint">
        Type the setting's name to confirm. The host refuses this write without it, so this
        dialog cannot be clicked through.
      </p>

      <input
        :ref="(input) => (input as HTMLInputElement | null)?.focus()"
        v-model="typed"
        type="text"
        spellcheck="false"
        autocomplete="off"
        :placeholder="props.keyName"
        :aria-label="`type ${props.keyName} to confirm`"
        class="rounded-[6px] bg-raised px-3 py-2 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint focus:ring-1 focus:ring-accent"
        data-input="settings-confirm"
        @keydown.enter.prevent="matches && emit('confirm', typed)"
      />

      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          class="rounded-[6px] px-2.5 py-1 text-[12px] text-dim hover:bg-raised hover:text-fg"
          data-action="settings-confirm-cancel"
          @click="emit('cancel')"
        >
          Cancel
        </button>
        <button
          type="button"
          :disabled="!matches"
          class="rounded-[6px] border border-err/60 px-2.5 py-1 text-[12px] text-err hover:bg-err/10 disabled:opacity-40"
          data-action="settings-confirm-write"
          @click="emit('confirm', typed)"
        >
          Write it
        </button>
      </div>
    </div>
  </Modal>
</template>
