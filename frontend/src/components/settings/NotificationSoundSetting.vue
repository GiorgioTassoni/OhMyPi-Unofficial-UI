<script setup lang="ts">
import { ref } from "vue";
import { playNotificationSound, type SoundKind } from "../../lib/notificationSound";
import Icon from "../ui/Icon.vue";
import Toggle from "../ui/Toggle.vue";

const props = defineProps<{ enabled: boolean }>();
const emit = defineEmits<{ change: [enabled: boolean] }>();
const previewError = ref<string | null>(null);

async function preview(kind: SoundKind): Promise<void> {
  previewError.value = null;
  try {
    await playNotificationSound(kind);
  } catch (error) {
    previewError.value = error instanceof Error ? error.message : String(error);
  }
}
</script>

<template>
  <section class="mt-8">
    <div class="flex items-center gap-3 border-b border-line/50 pb-2">
      <h2 class="text-[12px] font-semibold uppercase tracking-wider text-fg/80">App sounds</h2>
    </div>
    <div class="flex items-start gap-6 rounded-[8px] px-3 py-3 hover:bg-raised/40" data-setting="notification-sound">
      <div class="min-w-0 flex-1">
        <p class="text-[13.5px] font-medium text-fg">Notification sound</p>
        <p class="mt-0.5 text-[12px] leading-relaxed text-faint">
          Play a soft chime when a turn finishes or the agent needs your answer.
        </p>
        <div class="mt-2.5 flex flex-wrap items-center gap-2 text-[11px] text-faint">
          <span class="mr-1">App preference · applies immediately</span>
          <button
            type="button"
            class="inline-flex items-center gap-1 rounded-[5px] border border-line/60 bg-surface/50 px-2 py-0.5 text-dim transition-all hover:border-line-strong hover:bg-raised hover:text-fg active:scale-95 shadow-xs"
            @click="preview('finished')"
          >
            <Icon name="play" class="h-2.5 w-2.5 text-accent" />
            Preview finished
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1 rounded-[5px] border border-line/60 bg-surface/50 px-2 py-0.5 text-dim transition-all hover:border-line-strong hover:bg-raised hover:text-fg active:scale-95 shadow-xs"
            @click="preview('needs-you')"
          >
            <Icon name="play" class="h-2.5 w-2.5 text-accent" />
            Preview question
          </button>
        </div>
        <p v-if="previewError" class="mt-1.5 text-[11px] text-err" role="alert">{{ previewError }}</p>
      </div>
      <Toggle :checked="props.enabled" label="Notification sound" @change="emit('change', $event)" />
    </div>
  </section>
</template>
