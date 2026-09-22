<script setup lang="ts">
/**
 * What the composer is holding (`docs/12` §5.1).
 *
 * Two kinds of chip, because there are two routes and the difference is visible to the
 * user: an image shows itself and travels as bytes with the message, while a path shows
 * where the file is and travels as that path for the agent to read. Neither is a
 * thumbnail of what the model will see — the engine re-encodes images per model and
 * drops them entirely for a text-only one, so this strip is the app's own account of
 * what was attached. `docs/12` §3.1's row inside the conversation is the engine's.
 *
 * A refusal rides here too, under the chips: an image that cannot fit the frame has to
 * be visible *before* a send, not discovered by one (`lib/attachments.ts`).
 *
 * The chips carry no border: they are raised fields on the composer's surface, the same
 * fill the composer's own prompt block uses, so the strip reads as part of the message
 * rather than as a row of boxes above it.
 */
import { describeBytes, type Attachment } from "../lib/attachments";
import Icon from "./ui/Icon.vue";

defineProps<{
  attachments: Attachment[];
  /** Why the last attach attempt failed, or why the message cannot be sent. */
  refusal: string | null;
}>();

const emit = defineEmits<{
  (event: "remove", id: string): void;
}>();
</script>

<template>
  <div v-if="attachments.length > 0 || refusal" class="mb-2 flex flex-col gap-1">
    <div v-if="attachments.length > 0" class="flex flex-wrap gap-1.5">
      <div
        v-for="attachment in attachments"
        :key="attachment.id"
        class="flex items-center gap-2 rounded-[6px] bg-raised p-1 pr-1.5"
      >
        <img
          v-if="attachment.kind === 'image'"
          :src="`data:${attachment.mime};base64,${attachment.data}`"
          :alt="attachment.name"
          class="h-9 w-9 rounded-[6px] object-cover ring-1 ring-line"
        />
        <!-- No thumbnail for a path: the app holds no filesystem permission, so the
             file is not read here. The engine reads it if the agent opens it. -->
        <span
          v-else
          class="grid h-9 w-9 place-items-center rounded-[6px] text-faint ring-1 ring-line"
        >
          <Icon name="file" class="h-4 w-4" />
        </span>

        <span class="flex flex-col leading-tight">
          <span
            :title="attachment.kind === 'path' ? attachment.path : attachment.name"
            class="max-w-[14rem] truncate font-mono text-[11.5px] text-fg"
          >
            {{ attachment.name }}
          </span>
          <span class="text-[10.5px] text-faint">
            <template v-if="attachment.kind === 'image'">
              {{ attachment.mime }} · {{ describeBytes(attachment.bytes) }}
            </template>
            <template v-else>path · the agent reads it</template>
          </span>
        </span>

        <button
          :title="`remove ${attachment.name}`"
          class="rounded-[6px] p-1 text-faint hover:bg-surface hover:text-err"
          @click="emit('remove', attachment.id)"
        >
          <Icon name="close" class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <p v-if="refusal" class="text-[11.5px] text-err">{{ refusal }}</p>
  </div>
</template>
