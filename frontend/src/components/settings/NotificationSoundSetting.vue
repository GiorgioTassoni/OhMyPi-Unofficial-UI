<script setup lang="ts">
import { ref } from "vue";
import { playNotificationSound, type SoundKind } from "../../lib/notificationSound";
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
        <div class="mt-2 flex items-center gap-3 text-[11px] text-faint">
          <span>App preference · applies immediately</span>
          <button type="button" class="text-dim hover:text-fg" @click="preview('finished')">Preview finished</button>
          <button type="button" class="text-dim hover:text-fg" @click="preview('needs-you')">Preview question</button>
        </div>
        <p v-if="previewError" class="mt-1.5 text-[11px] text-err" role="alert">{{ previewError }}</p>
      </div>
      <Toggle :checked="props.enabled" label="Notification sound" @change="emit('change', $event)" />
    </div>
  </section>
</template>
