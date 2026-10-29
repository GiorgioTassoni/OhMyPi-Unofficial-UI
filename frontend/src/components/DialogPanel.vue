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
import Icon from "./ui/Icon.vue";
import {
  allowCommand as grantCommand,
  allowTool,
  nextCommandGrant,
  respondUiRequest,
  type UiAnswer,
  type UiRequestSnapshot,
} from "../bridge";
import {
  APPROVE,
  approvalOf,
  formatRemaining,
  remainingMs,
} from "../lib/approval";
import {
  cleanOptionLabel,
  isDoneOption,
  isMultiSelectDialog,
  isOptionSelected,
  loadSelectedOptions,
  markDialogInteraction,
  questionKey,
  saveSelectedOptions,
  titleParts,
} from "../lib/dialogSelect";

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
/** The host names the next unapproved executable; the frontend never guesses. */
const candidatePrograms = ref<Record<string, string | null>>({});
let candidateRead = 0;

/** Client-side tracking of selected options in multi-choice questions across roundtrips. */
const selectedOptionsByQuestion = ref<Record<string, Set<string>>>(
  Object.fromEntries(
    Object.entries(loadSelectedOptions()).map(([k, arr]) => [k, new Set(arr)]),
  ),
);

async function refreshCandidates(): Promise<void> {
  const read = ++candidateRead;
  candidatePrograms.value = {};
  const entries = await Promise.all(props.dialogs.map(async (dialog) => {
    const approval = approvalOf(dialog);
    const command = approval?.fields.find((field) => field.label === "Command")?.value;
    if (!approval || !isCommandApproval(approval.tool) || !command) {
      return [dialog.id, null] as const;
    }
    try {
      return [dialog.id, await nextCommandGrant(props.thread, dialog.title)] as const;
    } catch {
      // A stale/closed session cannot safely offer a grant. Allow once still works.
      return [dialog.id, null] as const;
    }
  }));
  if (read === candidateRead) {
    candidatePrograms.value = Object.fromEntries(entries);
  }
}

watch([() => props.thread, () => props.dialogs], () => {
  void refreshCandidates();
}, { immediate: true, deep: true });

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
      // Sync any option already flagged by the engine or with checkmark prefix
      if (dialog.kind === "select") {
        const key = questionKey(props.thread, dialog.title);
        dialog.options.forEach((opt, idx) => {
          if (
            dialog.optionDetails?.[idx]?.selected === true ||
            /^(\[[xX✓✔☑]\]|[✓✔☑])\s+/i.test(opt)
          ) {
            selectedOptionsByQuestion.value[key] ??= new Set<string>();
            selectedOptionsByQuestion.value[key]!.add(cleanOptionLabel(opt));
          }
        });
      }
    }

    // Drop stale question keys belonging to this thread
    const threadPrefix = `${props.thread}:`;
    const liveKeys = new Set(dialogs.map((d) => questionKey(props.thread, d.title)));
    let changed = false;
    for (const k of Object.keys(selectedOptionsByQuestion.value)) {
      if (k.startsWith(threadPrefix) && !liveKeys.has(k)) {
        delete selectedOptionsByQuestion.value[k];
        changed = true;
      }
    }
    if (changed) {
      saveSelectedOptions(selectedOptionsByQuestion.value);
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

function optionDescription(dialog: UiRequestSnapshot, index: number): string | null {
  const desc = dialog.optionDetails?.[index]?.description?.trim();
  return desc && desc.length > 0 ? desc : null;
}

interface DialogOptionView {
  label: string;
  cleanLabel: string;
  description: string | null;
  index: number;
  isSelected: boolean;
}

interface DialogSelectViewModel {
  selectable: DialogOptionView[];
  doneOption: { label: string; index: number } | null;
  isMulti: boolean;
}

function selectViewModel(dialog: UiRequestSnapshot): DialogSelectViewModel {
  const isMulti = isMultiSelectDialog(dialog);
  const qKey = questionKey(props.thread, dialog.title);
  const selectedSet = selectedOptionsByQuestion.value[qKey];

  let doneOption: { label: string; index: number } | null = null;
  const selectable: DialogOptionView[] = [];

  for (let i = 0; i < dialog.options.length; i++) {
    const opt = dialog.options[i];
    if (isDoneOption(opt)) {
      doneOption = { label: opt, index: i };
    } else {
      selectable.push({
        label: opt,
        cleanLabel: cleanOptionLabel(opt),
        description: optionDescription(dialog, i),
        index: i,
        isSelected: isOptionSelected(dialog, opt, i, selectedSet),
      });
    }
  }

  return { selectable, doneOption, isMulti };
}

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
      title: titleParts(dialog.title),
      approval,
      command,
      program: candidatePrograms.value[dialog.id] ?? null,
      deadline: left === null ? null : formatRemaining(left),
      busy: answering.value[dialog.id] === true,
      select: dialog.kind === "select" ? selectViewModel(dialog) : null,
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
  markDialogInteraction(props.thread);
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
): Promise<void> {
  writing.value = dialog.id;
  try {
    await grantCommand(props.thread, dialog.title);
  } catch (cause) {
    emit("failed", describe(cause));
    return;
  } finally {
    writing.value = null;
  }
  void refreshCandidates();
  await answer(dialog, { value: APPROVE });
}

function onSelectOption(dialog: UiRequestSnapshot, option: string, _index?: number): void {
  if (answering.value[dialog.id] === true) return;

  if (isDoneOption(option)) {
    void answer(dialog, { value: option });
    return;
  }

  if (isMultiSelectDialog(dialog)) {
    const key = questionKey(props.thread, dialog.title);
    selectedOptionsByQuestion.value[key] ??= new Set<string>();
    const set = selectedOptionsByQuestion.value[key]!;
    const clean = cleanOptionLabel(option);
    if (set.has(clean)) {
      set.delete(clean);
    } else {
      set.add(clean);
    }
    saveSelectedOptions(selectedOptionsByQuestion.value);
  }

  void answer(dialog, { value: option });
}

function onEditorKeydown(event: KeyboardEvent, dialog: UiRequestSnapshot): void {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    const val = replies.value[dialog.id] ?? "";
    if (val.trim().length > 0) {
      void answer(dialog, { value: val });
    }
  }
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
      :class="card.approval
        ? (isCommandApproval(card.approval.tool) ? 'border-line-strong/70 bg-selected/60' : 'border-warn/40 bg-warn/5')
        : 'border-line-strong/70 bg-raised/50 shadow-sm'"
    >
      <header class="mb-3 flex items-center gap-2">
        <span
          class="h-1.5 w-1.5 shrink-0 rounded-full"
          :class="card.approval ? 'bg-warn' : 'bg-accent'"
        />
        <span v-if="card.command && card.approval && isCommandApproval(card.approval.tool)" class="text-[12.5px] font-medium text-fg">
          The agent wants to run
        </span>
        <template v-else-if="card.approval">
          <span class="text-[11px] font-medium text-warn">Needs approval</span>
          <span class="text-[12.5px] font-medium text-fg">{{ card.approval.tool }}</span>
        </template>
        <template v-else>
          <span class="text-[12.5px] font-medium text-fg">
            {{ card.dialog.kind === 'confirm' ? 'Confirmation' : 'Question' }}
          </span>
          <span
            v-if="card.title.isCustomResponse"
            class="text-[11px] text-faint"
          >
            · Custom response
          </span>
        </template>
        <span
          v-if="card.title.stepBadge"
          class="ml-auto font-mono text-[11px] text-faint"
        >
          {{ card.title.stepBadge }}
        </span>
        <span v-else-if="cards.length > 1" class="ml-auto font-mono text-[11px] text-faint">
          {{ index + 1 }} of {{ cards.length }}
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
            @click="allowCommandForConversation(card.dialog)"
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
          Always allow applies to this conversation. This run is approved in full;
          later runs still ask about other commands in the line.
        </p>
      </template>

      <!-- Any other dialog: the engine's text, and a control that answers it. -->
      <template v-else>
        <div class="flex items-start gap-2">
          <span
            v-if="card.title.selectedBadge"
            class="inline-flex shrink-0 items-center rounded-[4px] bg-accent/15 px-1.5 py-0.5 text-[10.5px] font-medium text-accent"
          >
            {{ card.title.selectedBadge }}
          </span>
          <p class="text-[12.5px] font-medium whitespace-pre-wrap text-fg">
            {{ card.title.text }}
          </p>
        </div>
        <p
          v-if="card.dialog.message && !card.title.isCustomResponse"
          class="mt-2 text-[12.5px] leading-relaxed whitespace-pre-wrap text-dim"
        >
          {{ card.dialog.message }}
        </p>

        <div v-if="card.dialog.kind === 'select' && card.select" class="mt-3 flex flex-col gap-1.5">
          <!-- Selectable options: checkboxes for multi-select, radios for single-select -->
          <button
            v-for="opt in card.select.selectable"
            :key="`${opt.label}_${opt.index}`"
            :disabled="card.busy"
            class="group relative flex items-start gap-3 rounded-[8px] border px-3.5 py-2.5 text-left text-[12.5px] transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
            :class="[
              opt.isSelected
                ? 'border-accent/80 bg-accent/10 text-fg shadow-xs ring-1 ring-accent/30'
                : 'border-line-strong/50 bg-surface/60 text-fg hover:border-accent/70 hover:bg-raised hover:text-fg',
              card.busy ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer',
            ]"
            @click="onSelectOption(card.dialog, opt.label, opt.index)"
          >
            <!-- Multi-select checkbox -->
            <div
              v-if="card.select.isMulti"
              class="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors"
              :class="[
                opt.isSelected
                  ? 'border-accent bg-accent text-canvas shadow-xs'
                  : 'border-line-strong bg-surface text-transparent group-hover:border-accent/60',
              ]"
            >
              <svg class="h-3 w-3 fill-current stroke-current stroke-[0.5]" viewBox="0 0 20 20">
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
            </div>

            <!-- Single-select radio -->
            <div
              v-else
              class="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border border-line-strong bg-surface transition-colors group-hover:border-accent/60"
            >
              <div class="h-1.5 w-1.5 rounded-full bg-transparent group-hover:bg-accent" />
            </div>

            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <div class="flex items-center gap-2">
                <span class="leading-snug" :class="opt.isSelected ? 'font-medium text-fg' : 'text-fg'">
                  {{ opt.cleanLabel }}
                </span>
                <span
                  v-if="opt.isSelected"
                  class="rounded bg-accent/20 px-1.5 py-0.2 text-[10.5px] font-medium text-accent"
                >
                  selected
                </span>
              </div>
              <span
                v-if="opt.description"
                class="text-[11.5px] leading-relaxed text-dim transition-colors whitespace-pre-wrap group-hover:text-fg/80"
              >
                {{ opt.description }}
              </span>
            </div>
          </button>

          <!-- Done selecting primary action button (if present) -->
          <div
            v-if="card.select.doneOption"
            class="mt-2 flex items-center justify-end pt-1"
          >
            <button
              :disabled="card.busy"
              class="flex items-center gap-2 rounded-[8px] bg-accent px-4 py-2 text-[12.5px] font-medium text-canvas shadow-xs transition-all hover:bg-accent/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-60 disabled:cursor-not-allowed"
              @click="onSelectOption(card.dialog, card.select.doneOption.label, card.select.doneOption.index)"
            >
              <svg class="h-3.5 w-3.5 fill-current stroke-current stroke-[0.5]" viewBox="0 0 20 20">
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
              <span>Done selecting</span>
            </button>
          </div>
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
          <div
            v-if="card.dialog.kind === 'editor'"
            class="group relative flex flex-col rounded-[8px] border border-line-strong/60 bg-surface/80 p-2.5 transition-all focus-within:border-accent/80 focus-within:ring-1 focus-within:ring-accent/40"
          >
            <textarea
              v-model="replies[card.dialog.id]"
              rows="3"
              spellcheck="false"
              :placeholder="card.dialog.placeholder || (card.title.isCustomResponse ? 'Type your custom response...' : 'Type your response...')"
              class="w-full resize-none bg-transparent pr-3 text-[12.5px] leading-relaxed text-fg outline-none placeholder:text-faint/70 [scrollbar-gutter:stable]"
              @keydown="onEditorKeydown($event, card.dialog)"
            />
            <div class="mt-2 flex items-center justify-between border-t border-line-strong/30 pt-2">
              <span class="flex items-center gap-1.5 text-[11px] text-faint select-none">
                <kbd class="rounded border border-line-strong/60 bg-raised/70 px-1.5 py-0.5 text-[10px] font-mono text-dim">Enter</kbd>
                <span>send</span>
                <span class="opacity-40">·</span>
                <kbd class="rounded border border-line-strong/60 bg-raised/70 px-1.5 py-0.5 text-[10px] font-mono text-dim">Shift+Enter</kbd>
                <span>new line</span>
              </span>
              <button
                :disabled="card.busy || !replies[card.dialog.id]?.trim()"
                class="inline-flex items-center gap-1.5 rounded-[6px] bg-accent px-3 py-1.5 text-[12px] font-medium text-canvas shadow-xs transition-all hover:bg-accent/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-35 disabled:cursor-not-allowed"
                @click="answer(card.dialog, { value: replies[card.dialog.id] ?? '' })"
              >
                <span>Send</span>
                <Icon name="arrow-up" size="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
          <div
            v-else
            class="flex items-center gap-2"
          >
            <input
              v-model="replies[card.dialog.id]"
              :placeholder="card.dialog.placeholder ?? 'Type your answer...'"
              spellcheck="false"
              class="flex-1 rounded-[6px] border border-line-strong/60 bg-surface/80 px-3 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint/70 focus:border-accent/80 focus:ring-1 focus:ring-accent/40"
              @keydown.enter="answer(card.dialog, { value: replies[card.dialog.id] ?? '' })"
            />
            <button
              :disabled="card.busy || !replies[card.dialog.id]?.trim()"
              class="inline-flex items-center gap-1.5 rounded-[6px] bg-accent px-3 py-2 text-[12px] font-medium text-canvas shadow-xs transition-all hover:bg-accent/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-35 disabled:cursor-not-allowed"
              @click="answer(card.dialog, { value: replies[card.dialog.id] ?? '' })"
            >
              <span>Send</span>
              <Icon name="arrow-up" size="h-3.5 w-3.5" />
            </button>
          </div>
        </div>

        <div class="mt-2.5 flex items-center justify-between">
          <p v-if="card.deadline" class="font-mono text-[11px] text-warn">
            {{ card.deadline }}
          </p>
          <div v-else />
          <button
            class="rounded-[6px] px-2.5 py-1 text-[11.5px] text-faint transition-colors hover:bg-raised hover:text-fg"
            @click="answer(card.dialog, { cancelled: true })"
          >
            dismiss
          </button>
        </div>
      </template>

      <p v-if="card.approval && card.deadline" class="mt-2 font-mono text-[11px] text-warn">
        {{ card.deadline }}
      </p>
    </section>

  </div>
</template>
