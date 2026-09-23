<script setup lang="ts">
/**
 * The dialogs the agent is waiting on (`docs/12` §10).
 *
 * Every dialog here is *blocking*: the run is parked inside a tool call until the
 * host answers, so an unrendered kind is not a degraded screen — it is a stopped
 * agent. That is why all four wire kinds have a path, why an unknown kind still
 * renders its text and a way out, and why the engine's own deadline is shown rather
 * than left to expire in silence.
 *
 * An approval is the one kind the engine formats for us: a two-option `select` whose
 * `title` the engine laid out. It is rendered as that layout — labelled fields, and
 * a body for the content — rather than re-derived from the tool arguments, because
 * the engine's `formatApprovalDetails` is the decision about what a user needs to
 * see (`lib/approval.ts` reads it).
 *
 * Tool-level "Always allow" persists in OMP; executable grants stay on this conversation.
 */
import { computed, onUnmounted, ref, watch } from "vue";
import {
  allowCommand as grantCommand,
  allowTool,
  respondUiRequest,
  type UiAnswer,
  type UiRequestSnapshot,
} from "../bridge";
import {
  APPROVE,
  approvalOf,
  commandProgram,
  formatRemaining,
  remainingMs,
} from "../lib/approval";

const props = defineProps<{
  /** The thread the dialogs belong to: an answer goes to its session. */
  thread: string;
  /** The pending set, as the host announced it. */
  dialogs: UiRequestSnapshot[];
}>();

const emit = defineEmits<{ (event: "failed", message: string): void }>();

/** Text typed into `input`/`editor` dialogs, keyed by dialog id. */
const replies = ref<Record<string, string>>({});
/** When this window first saw each dialog — the anchor its deadline counts from. */
const seenAt = ref<Record<string, number>>({});
/** The clock the countdown reads. Only runs while something has a deadline. */
const now = ref(Date.now());
/** Answers in flight, so a second click cannot answer the same dialog twice. */
const answering = ref<Record<string, boolean>>({});
/** The dialog whose "always allow" write is in flight. */
const writing = ref<string | null>(null);

/**
 * The two buttons a dialog has.
 *
 * A generic dialog answers an engine question using the engine's own options.
 */
const PRIMARY =
  "rounded-[6px] bg-accent px-2.5 py-1 text-[12px] font-medium text-canvas disabled:opacity-40";
const SECONDARY =
  "rounded-[6px] px-2.5 py-1 text-[12px] text-dim hover:bg-raised hover:text-fg disabled:opacity-40";
const APPROVAL_BUTTON =
  "min-h-[38px] rounded-[7px] px-3 py-1.5 text-[12px] font-medium transition-colors hover:bg-line-strong/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-40";

watch(
  () => props.dialogs,
  (dialogs) => {
    const live = new Set(dialogs.map((dialog) => dialog.id));
    // Text typed into a dialog that is gone is dropped, so a later dialog reusing
    // the id cannot inherit a stale answer.
    replies.value = Object.fromEntries(
      Object.entries(replies.value).filter(([id]) => live.has(id)),
    );
    seenAt.value = Object.fromEntries(
      Object.entries(seenAt.value).filter(([id]) => live.has(id)),
    );
    answering.value = Object.fromEntries(
      Object.entries(answering.value).filter(([id]) => live.has(id)),
    );

    for (const dialog of dialogs) {
      seenAt.value[dialog.id] ??= Date.now();
      // An `editor` starts from the engine's prefill; an `input` starts empty with
      // the engine's placeholder as a hint.
      if (dialog.prefill !== null && replies.value[dialog.id] === undefined) {
        replies.value[dialog.id] = dialog.prefill;
      }
    }
  },
  { immediate: true, deep: true },
);

/**
 * Tick only while a deadline is on screen.
 *
 * The engine answers a dialog itself when its `timeout` expires, so the countdown is
 * the difference between "it stopped" and "it told me it would" — but a permanent
 * one-second timer, for dialogs that mostly carry no deadline at all, is not worth
 * the wakeups.
 */
const ticking = computed(() => props.dialogs.some((dialog) => dialog.timeoutMs !== null));
let ticker: ReturnType<typeof setInterval> | null = null;

watch(
  ticking,
  (live) => {
    if (live && ticker === null) {
      ticker = setInterval(() => {
        now.value = Date.now();
      }, 1000);
      return;
    }
    if (!live && ticker !== null) {
      clearInterval(ticker);
      ticker = null;
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  if (ticker !== null) {
    clearInterval(ticker);
    ticker = null;
  }
});

/**
 * One pass over the pending set per render.
 *
 * The parse and the countdown are derived from a dialog plus the clock, so doing
 * them here keeps the template from re-reading the same prompt once per line it
 * renders.
 */
const cards = computed(() =>
  props.dialogs.map((dialog) => {
    const left = remainingMs(dialog.timeoutMs, seenAt.value[dialog.id] ?? now.value, now.value);
    const approval = approvalOf(dialog);
    const command = approval?.fields.find((field) => field.label === "Command")?.value ?? null;
    return {
      dialog,
      approval,
      command,
      program: approval !== null && isCommandApproval(approval.tool) && command !== null
        ? commandProgram(command)
        : null,
      deadline: left === null ? null : formatRemaining(left),
      busy: answering.value[dialog.id] === true,
    };
  }),
);

function isCommandApproval(tool: string): boolean {
  return tool === "bash" || tool === "bash_interactive";
}

async function answer(dialog: UiRequestSnapshot, reply: UiAnswer): Promise<void> {
  if (answering.value[dialog.id] === true) {
    return;
  }
  answering.value = { ...answering.value, [dialog.id]: true };
  try {
    await respondUiRequest(props.thread, dialog.id, reply);
  } catch (cause) {
    // A rejection means the engine is no longer waiting — its deadline passed, or
    // the dialog was withdrawn — which the store has already announced. Report it;
    // retrying would answer whatever replaced it.
    emit("failed", describe(cause));
  } finally {
    const next = { ...answering.value };
    delete next[dialog.id];
    answering.value = next;
  }
}

/**
 * Answer this call and later matching requests in this session.
 *
 * The write goes first: if it fails, nothing has been approved and the dialog is
 * still there for the user to decide on its own.
 */
async function allow(dialog: UiRequestSnapshot, tool: string): Promise<void> {
  writing.value = dialog.id;
  try {
    await allowTool(props.thread, tool);
  } catch (cause) {
    emit("failed", describe(cause));
    return;
  } finally {
    writing.value = null;
  }
  await answer(dialog, { value: APPROVE });
}

/** Store a conversation-local executable grant before approving this call. */
async function allowCommandForConversation(
  dialog: UiRequestSnapshot,
  command: string,
): Promise<void> {
  writing.value = dialog.id;
  try {
    await grantCommand(props.thread, command);
  } catch (cause) {
    emit("failed", describe(cause));
    return;
  } finally {
    writing.value = null;
  }
  await answer(dialog, { value: APPROVE });
}

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
  <div v-if="cards.length > 0" class="flex flex-col gap-3 select-text" data-dialog-panel>
    <section
      v-for="(card, index) in cards"
      :key="card.dialog.id"
      class="rounded-[10px] border p-3.5"
      :class="card.approval && isCommandApproval(card.approval.tool)
        ? 'border-line-strong/70 bg-selected/60'
        : 'border-warn/40 bg-warn/5'"
    >
      <header class="mb-3 flex items-center gap-2">
        <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-warn" />
        <span v-if="card.command && card.approval && isCommandApproval(card.approval.tool)" class="text-[12.5px] font-medium text-fg">
          The agent wants to run
        </span>
        <template v-else>
          <span class="text-[11px] font-medium text-warn">Needs approval</span>
          <span v-if="card.approval" class="text-[12.5px] font-medium text-fg">
            {{ card.approval.tool }}
          </span>
          <span v-else class="text-[12px] text-dim">{{ card.dialog.kind }}</span>
        </template>
        <span v-if="cards.length > 1" class="ml-auto text-[11px] text-faint">
          {{ index + 1 }} of {{ cards.length }}
        </span>
        <span v-else-if="!card.approval" class="ml-auto font-mono text-[10.5px] text-faint">
          {{ card.dialog.id }}
        </span>
      </header>

      <!-- An approval, as the engine laid it out. -->
      <template v-if="card.approval">
        <p
          v-if="card.dialog.message"
          class="text-[12.5px] whitespace-pre-wrap text-fg"
        >
          {{ card.dialog.message }}
        </p>

        <!-- The engine's own argument field names, one readout per line. -->
        <dl class="flex flex-col gap-2">
          <div
            v-for="(field, position) in card.approval.fields"
            :key="position"
            :class="
              field.label === 'Command' && isCommandApproval(card.approval.tool)
                ? 'rounded-[7px] bg-canvas/70 px-3 py-2.5'
                : 'flex items-start justify-between gap-4 py-1'
            "
          >
            <template v-if="field.label === 'Command' && isCommandApproval(card.approval.tool)">
              <dt class="sr-only">Command</dt>
              <dd class="whitespace-pre-wrap break-words font-mono not-italic text-[12px] leading-relaxed text-fg">
                {{ field.value }}
              </dd>
            </template>
            <template v-else>
              <dt class="text-[12.5px] text-fg">{{ field.label }}</dt>
              <dd class="max-w-[82%] break-all font-mono text-[11.5px] text-dim">{{ field.value }}</dd>
            </template>
          </div>
        </dl>

        <div v-for="block in card.approval.blocks" :key="block.label" class="mt-2">
          <p class="mb-1 px-1 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            {{ block.label }}
          </p>
          <pre
            class="max-h-64 overflow-auto rounded-[6px] bg-raised p-2.5 font-mono text-[11.5px] whitespace-pre-wrap text-dim"
          >{{ block.body }}</pre>
        </div>

        <!-- Lines the engine sent that carry no label we know: shown, never dropped. -->
        <p
          v-for="(note, line) in card.approval.notes"
          :key="line"
          class="mt-1 text-[11.5px] whitespace-pre-wrap text-faint"
        >
          {{ note }}
        </p>

        <div class="mt-3 flex flex-wrap items-center gap-2">
          <button
            v-if="isCommandApproval(card.approval.tool) && card.program && card.command"
            :disabled="card.busy || writing === card.dialog.id"
            :class="[APPROVAL_BUTTON, 'inline-flex items-center text-fg']"
            @click="allowCommandForConversation(card.dialog, card.command)"
          >
            <span class="inline-flex items-baseline gap-2">
              <span>Always allow</span>
              <span class="font-mono font-medium not-italic text-warn/80">{{ card.program }}</span>
            </span>
          </button>
          <button
            v-if="!isCommandApproval(card.approval.tool)"
            :disabled="card.busy || writing === card.dialog.id"
            :class="[APPROVAL_BUTTON, 'text-fg']"
            @click="allow(card.dialog, card.approval.tool)"
          >
            Always allow {{ card.approval.tool }}
          </button>
          <button
            :disabled="card.busy || writing === card.dialog.id"
            :class="[APPROVAL_BUTTON, 'text-fg']"
            @click="answer(card.dialog, { value: APPROVE })"
          >
            Allow once
          </button>
          <button
            :disabled="card.busy || writing === card.dialog.id"
            :class="[APPROVAL_BUTTON, 'text-err/80']"
            @click="answer(card.dialog, { value: card.dialog.options[1] })"
          >
            Deny
          </button>
        </div>
        <p
          v-if="isCommandApproval(card.approval.tool) && card.program"
          class="mt-1.5 text-[11px] text-dim"
        >
          Always allow applies to this conversation.
        </p>
      </template>

      <!-- Any other dialog: the engine's text, and a control that answers it. -->
      <template v-else>
        <p class="text-[12.5px] whitespace-pre-wrap text-fg">
          {{ card.dialog.title }}
        </p>
        <p
          v-if="card.dialog.message"
          class="mt-2 text-[12.5px] leading-relaxed whitespace-pre-wrap text-dim"
        >
          {{ card.dialog.message }}
        </p>

        <div v-if="card.dialog.kind === 'select'" class="mt-3 flex flex-wrap gap-1.5">
          <button
            v-for="(option, position) in card.dialog.options"
            :key="option"
            :class="position === 0 ? PRIMARY : SECONDARY"
            @click="answer(card.dialog, { value: option })"
          >
            {{ option }}
          </button>
        </div>

        <div v-else-if="card.dialog.kind === 'confirm'" class="mt-3 flex gap-1.5">
          <button
            :class="PRIMARY"
            @click="answer(card.dialog, { confirmed: true })"
          >
            yes
          </button>
          <button
            :class="SECONDARY"
            @click="answer(card.dialog, { confirmed: false })"
          >
            no
          </button>
        </div>

        <!-- `editor` exists to collect more than one line, so it gets a textarea and a
             button; `input` gets one line, with Enter as the obvious way to send. An
             unknown kind falls here too, which is what keeps it answerable. -->
        <div v-else class="mt-3 flex flex-col gap-2">
          <textarea
            v-if="card.dialog.kind === 'editor'"
            v-model="replies[card.dialog.id]"
            rows="3"
            spellcheck="false"
            class="w-full resize-none rounded-[6px] bg-raised px-2.5 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint focus:ring-1 focus:ring-accent"
          />
          <input
            v-else
            v-model="replies[card.dialog.id]"
            :placeholder="card.dialog.placeholder ?? ''"
            spellcheck="false"
            class="w-full rounded-[6px] bg-raised px-2.5 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint focus:ring-1 focus:ring-accent"
            @keydown.enter="answer(card.dialog, { value: replies[card.dialog.id] ?? '' })"
          />
          <button
            :class="[PRIMARY, 'self-start']"
            @click="answer(card.dialog, { value: replies[card.dialog.id] ?? '' })"
          >
            send
          </button>
        </div>

        <button
          class="mt-2 rounded-[6px] px-2 py-1 text-[11.5px] text-faint hover:bg-raised hover:text-fg"
          @click="answer(card.dialog, { cancelled: true })"
        >
          dismiss
        </button>
      </template>

      <p v-if="card.deadline" class="mt-2 font-mono text-[11px] text-warn">
        {{ card.deadline }}
      </p>
    </section>

  </div>
</template>
