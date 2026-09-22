<script setup lang="ts">
/**
 * What a thread row's context menu actually does (`docs/12` §2.3), and the decisions that
 * need a dialog.
 *
 * Kept out of the shell on purpose: the shell is layout, and every action here is a session
 * flow with its own failure modes — a fork that mints a new session id, a delete that
 * removes files, a handoff that writes none. Each one reports what it did rather than
 * leaving the user to guess, because two of them change nothing visible on their own:
 * a handoff commits a compaction entry (no file, no new session) and an export writes a
 * file elsewhere on disk.
 */
import { computed, ref, watch } from "vue";
import {
  branchTargets,
  branchThread,
  closeThread,
  deleteSession,
  exportHtml,
  handoffThread,
  openPath,
  pinSession,
  type BranchTarget,
  type ThreadSnapshot,
} from "../bridge";
import { menuFor, type MenuAction, type ThreadContext } from "../lib/thread-menu";
import BranchPicker from "./BranchPicker.vue";
import Modal from "./Modal.vue";
import ThreadMenu from "./ThreadMenu.vue";

const props = defineProps<{
  /** The open menu: which thread, and where the press landed. */
  menu: { id: string; at: { x: number; y: number }; action?: "delete" } | null;
  /** Everything the menu's rules need to know about that thread. */
  context: ThreadContext;
  /** The thread whose sessions can run the menu's engine commands (for `/pin`). */
  dispatcher: string | null;
  /** The thread's name, for the menu's label and the dialogs. */
  label: string;
}>();

const emit = defineEmits<{
  close: [];
  /** The store or the live set changed: re-read both. */
  refresh: [];
  /** Switch to another thread — a fork replaces the id we were talking to. */
  select: [id: string];
  /**
   * A fork answered with a session this window has never seen. It goes to the shell, which
   * re-reads the catalogue first and only then resumes it — selecting it from here would
   * race the refresh and land on "no longer in the store".
   */
  forked: [thread: ThreadSnapshot];
  /** Start an inline rename; the sidebar owns the input. */
  rename: [id: string];
  /** The session no longer exists. */
  deleted: [id: string];
  /** A neutral note worth showing: what an action did, when nothing else moved. */
  notice: [message: string];
  failed: [message: string];
}>();

const busy = ref(false);
/**
 * What the menu was opened *on*.
 *
 * Captured rather than read from the props, because acting on an item dismisses the menu —
 * `emit("close")` — and every dialog that follows (fork, handoff, delete) would then find
 * `props.menu` null and silently do nothing. The subject outlives the menu it came from.
 */
const subject = ref<{ id: string; label: string; context: ThreadContext } | null>(null);
const targets = ref<BranchTarget[] | null>(null);
const handoff = ref<{ instructions: string } | null>(null);
const confirming = ref(false);

watch(
  () => props.menu,
  (menu) => {
    if (menu) {
      subject.value = {
        id: menu.id,
        label: props.label,
        // The menu is dismissed before its confirmation opens. Keep the facts about the
        // selected row with its id so the dialog cannot turn a saved, live session into an
        // "unsaved" one when `props.menu` becomes null.
        context: {
          ...props.context,
          session: props.context.session === null ? null : { ...props.context.session },
        },
      };
      if (menu.action === "delete") {
        confirming.value = true;
        emit("close");
      }
    }
  },
  { immediate: true },
);

const items = computed(() => menuFor(props.context));

async function act(action: MenuAction): Promise<void> {
  const id = subject.value?.id;
  if (!id) return;
  emit("close");

  try {
    switch (action) {
      case "rename":
        emit("rename", id);
        return;
      case "pin": {
        if (!props.dispatcher) {
          emit("failed", "pinning needs a live thread to dispatch the engine's own /pin");
          return;
        }
        busy.value = true;
        await pinSession(props.dispatcher, id);
        emit("refresh");
        return;
      }
      case "export": {
        busy.value = true;
        // The host picks an absolute path inside the app's own directory: left to itself,
        // the engine writes `omp-session-<id>.html` relative to the *sidecar's* working
        // directory, which is the user's project.
        const path = await exportHtml(id);
        emit("notice", `exported to ${path}`);
        await openPath(path);
        return;
      }
      case "fork": {
        busy.value = true;
        targets.value = await branchTargets(id);
        return;
      }
      case "handoff":
        handoff.value = { instructions: "" };
        return;
      case "reveal": {
        const path = props.context.session?.cwd;
        if (path) await openPath(path);
        return;
      }
      case "copy-cwd": {
        const path = props.context.session?.cwd;
        if (path) await navigator.clipboard.writeText(path);
        return;
      }
      case "stop":
        busy.value = true;
        await closeThread(id);
        emit("refresh");
        return;
      case "delete":
        confirming.value = true;
        return;
    }
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = false;
  }
}

async function fork(entryId: string): Promise<void> {
  const id = subject.value?.id;
  if (!id) return;
  busy.value = true;
  try {
    // The engine branches *in place*: the sidecar stays, the session file is new, and the
    // id we were talking to stops existing. So the answer is the new thread, and the shell
    // switches to it rather than trying to keep the old one alive.
    const forked = await branchThread(id, entryId);
    targets.value = null;
    emit("notice", `forked ${subject.value?.label ?? "the thread"} — the new thread is a child of it in the sidebar`);
    // Handed up rather than selected here: the fork is a session the window has never seen,
    // so the catalogue has to be re-read *before* something tries to open it.
    emit("forked", forked);
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = false;
  }
}

async function runHandoff(): Promise<void> {
  const id = subject.value?.id;
  if (!id || !handoff.value) return;
  busy.value = true;
  try {
    const text = handoff.value.instructions.trim();
    await handoffThread(id, text === "" ? null : text);
    handoff.value = null;
    // Deliberately worded: measured at v18.2.6, the RPC handoff never writes a file — it
    // commits a compaction entry into this same session, which is why the notice says so
    // and the transcript is re-read rather than a new thread offered.
    emit("notice", "handoff committed into this session as a compaction entry (no file is written)");
    emit("refresh");
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = false;
  }
}

async function remove(): Promise<void> {
  const id = subject.value?.id;
  const context = subject.value?.context;
  if (!id || !context) return;
  busy.value = true;
  try {
    if (context.live) {
      await closeThread(id);
    }
    if (context.session !== null) {
      await deleteSession(id);
    }
    confirming.value = false;
    emit("notice", `deleted ${subject.value?.label ?? "the conversation"}`);
    emit("deleted", id);
    emit("refresh");
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = false;
  }
}

function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}
</script>

<template>
  <ThreadMenu
    v-if="props.menu && props.menu.action === undefined"
    :items="items"
    :at="props.menu.at"
    :label="`actions for ${props.label}`"
    @pick="act"
    @close="emit('close')"
  />

  <BranchPicker
    v-if="targets !== null"
    :targets="targets"
    :busy="busy"
    @pick="fork"
    @close="targets = null"
  />

  <Modal v-if="handoff !== null" title="hand off this session" :busy="busy" @close="handoff = null">
    <p class="mb-2.5 text-[12.5px] text-dim">
      The engine writes a handoff document into this session as a compaction entry. It does
      not create a file or a new session — use Export HTML for that.
    </p>
    <textarea
      v-model="handoff.instructions"
      rows="3"
      spellcheck="false"
      class="w-full resize-none rounded-[6px] bg-raised px-2.5 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint focus:ring-1 focus:ring-accent"
      placeholder="optional instructions for the handoff (leave empty for the default)"
    />
    <div class="mt-3 flex items-center gap-2">
      <button
        :disabled="busy"
        class="rounded-[6px] bg-accent px-2.5 py-1 text-[12px] font-medium text-canvas disabled:opacity-40"
        @click="runHandoff"
      >
        {{ busy ? "handing off…" : "hand off" }}
      </button>
      <span class="text-[11px] text-faint">refused by the engine mid-turn</span>
    </div>
  </Modal>

  <Modal
    v-if="confirming"
    title="Delete conversation?"
    :busy="busy"
    @close="confirming = false"
  >
    <div class="rounded-[8px] border border-line/70 bg-raised/50 px-3 py-2.5">
      <p class="truncate text-[12.5px] font-medium text-fg" :title="subject?.label">
        {{ subject?.label }}
      </p>
      <p v-if="subject?.context.live" class="mt-1 text-[11.5px] text-warn">
        The active conversation will be stopped first.
      </p>
    </div>
    <p class="mt-3 text-[12px] leading-relaxed text-dim">
      <template v-if="subject?.context.session !== null">
        This permanently removes the conversation history and its generated artifacts.
      </template>
      <template v-else>
        This conversation has not been saved yet. It will be closed and removed.
      </template>
      This action cannot be undone.
    </p>
    <div class="mt-4 flex items-center justify-end gap-2">
      <button
        :disabled="busy"
        class="rounded-[6px] px-3 py-1.5 text-[12px] text-dim hover:bg-raised hover:text-fg disabled:opacity-40"
        @click="confirming = false"
      >
        Cancel
      </button>
      <button
        :disabled="busy"
        class="rounded-[6px] border border-err/30 bg-err/15 px-3 py-1.5 text-[12px] font-medium text-err hover:bg-err/25 disabled:opacity-40"
        @click="remove"
      >
        {{ busy ? "Deleting…" : "Delete conversation" }}
      </button>
    </div>
  </Modal>
</template>
