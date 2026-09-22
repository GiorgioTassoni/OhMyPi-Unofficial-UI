<script setup lang="ts">
/**
 * The permission ladder (`docs/12` §7.2).
 *
 * Three rows and nothing else — no Plan mode row (D4). The engine has no runtime
 * setter for this, so the host writes `tools.approvalMode` and restarts the sidecar
 * on the same session file; the popover says so before it happens, because the one
 * thing a mode change must not do is silently drop a running turn.
 */
import { onMounted, onUnmounted, ref } from "vue";
import { setApprovalMode } from "../bridge";
import { APPROVAL_MODES, modeRow } from "../lib/chips";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string;
  open: boolean;
  /** The mode in force for this session, as the host launched it. */
  mode: string | null;
}>();

const emit = defineEmits<{
  (event: "close"): void;
  (event: "failed", message: string): void;
  /** The mode changed, which restarts the sidecar: the caller re-reads the session. */
  (event: "changed"): void;
}>();

/** The mode being switched to, so the rows can refuse a second click. */
const switching = ref<string | null>(null);

async function choose(mode: string): Promise<void> {
  if (switching.value !== null || mode === props.mode) {
    if (mode === props.mode) {
      emit("close");
    }
    return;
  }
  switching.value = mode;
  try {
    // The restart returns the *thread*, not the new session's status: the approval mode
    // brings a different process up on the same session file, so the caller re-reads the
    // status and the conversation rather than being handed a stale picture.
    await setApprovalMode(props.thread, mode);
    emit("changed");
    emit("close");
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    switching.value = null;
  }
}

function onKeydown(event: KeyboardEvent): void {
  // The reference's keys: `1`–`3` pick a row. Only while the panel is open, and only
  // when nothing is being typed into.
  if (!props.open || switching.value !== null) {
    return;
  }
  const target = event.target as HTMLElement | null;
  if (target !== null && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) {
    return;
  }
  const index = Number(event.key) - 1;
  const row = APPROVAL_MODES[index];
  if (row !== undefined) {
    event.preventDefault();
    void choose(row.mode);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

/** Tauri rejects with a plain string, and a thrown `Error` would stringify badly. */
function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}
</script>

<template>
  <div class="flex flex-col gap-0.5">
    <button
      v-for="(row, index) in APPROVAL_MODES"
      :key="row.mode"
      type="button"
      :disabled="switching !== null"
      class="flex w-full items-start gap-3 rounded-[6px] px-2.5 py-2 text-left hover:bg-raised disabled:opacity-40"
      @click="choose(row.mode)"
    >
      <span class="flex min-w-0 flex-col">
        <span class="flex items-center gap-2">
          <span class="text-[13px]" :class="row.warning ? 'text-warn' : 'text-fg'">
            {{ row.label }}
          </span>
          <span v-if="modeRow(mode)?.mode === row.mode" class="text-[11px] text-accent">
            current
          </span>
          <span v-if="switching === row.mode" class="text-[11px] text-faint">switching…</span>
        </span>
        <span class="mt-0.5 text-[11.5px] text-dim">{{ row.detail }}</span>
      </span>

      <!--
        The row you are on is marked twice, as the reference does it: a check where the
        digit would be, and the word `current` beside the name.
      -->
      <Icon
        v-if="modeRow(mode)?.mode === row.mode"
        name="check"
        class="mt-0.5 ml-auto h-4 w-4 shrink-0 text-accent"
      />
      <span v-else class="mt-0.5 ml-auto shrink-0 font-mono text-[10.5px] text-faint">
        {{ index + 1 }}
      </span>
    </button>

    <p class="px-2.5 pb-0.5 pt-1.5 text-[11px] text-faint">
      changing this restarts the session on the same file — the conversation is kept,
      a turn in flight is not
    </p>
  </div>
</template>
