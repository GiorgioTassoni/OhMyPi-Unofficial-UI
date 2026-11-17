<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import Icon from "./ui/Icon.vue";
import ColorText from "./ui/ColorText.vue";
import type { AskAnswer, AskQuestion, UiRequestSnapshot } from "../bridge";
import {
  answerOf,
  emptyStates,
  selectOption,
  selectionFor,
  selectionReady,
  setCustomInput,
  type AskSelectionState,
} from "../lib/askDialog";

const props = defineProps<{
  dialog: UiRequestSnapshot;
  busy: boolean;
}>();

const emit = defineEmits<{
  (event: "submit", answers: AskAnswer[]): void;
  (event: "collapse"): void;
}>();

const PRIMARY =
  "inline-flex items-center gap-2 rounded-[8px] bg-accent px-4 py-2 text-[12.5px] font-medium text-canvas shadow-xs transition-all hover:bg-accent/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-40";
const PAGER =
  "flex h-6 w-6 items-center justify-center rounded-[4px] text-dim transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-40";

const states = ref<Record<string, AskSelectionState>>(emptyStates(props.dialog.questions));
const phase = ref(0);
const scroller = ref<HTMLDivElement | null>(null);
const legend = ref<HTMLLegendElement | null>(null);

const total = computed(() => props.dialog.questions.length);
const current = computed<AskQuestion | undefined>(
  () => props.dialog.questions[phase.value],
);
const last = computed(() => phase.value >= total.value - 1);
const ready = computed(() => {
  const question = current.value;
  return question !== undefined && selectionReady(question, stateOf(question));
});

function stateOf(question: AskQuestion): AskSelectionState {
  return selectionFor(states.value, question);
}

function apply(question: AskQuestion, next: AskSelectionState): void {
  states.value = { ...states.value, [question.id]: next };
}

function groupName(question: AskQuestion): string {
  return `ask-${props.dialog.id}-${question.id}`;
}

function customInputId(question: AskQuestion): string {
  return `${groupName(question)}-custom`;
}

async function show(index: number): Promise<void> {
  phase.value = index;
  await nextTick();
  scroller.value?.scrollTo({ top: 0 });
  legend.value?.focus();
}

function previous(): void {
  if (phase.value > 0) {
    void show(phase.value - 1);
  }
}

function next(): void {
  if (!last.value && ready.value) {
    void show(phase.value + 1);
  }
}

function submit(): void {
  if (props.busy || current.value === undefined) {
    return;
  }
  if (!last.value) {
    next();
    return;
  }
  const blocked = props.dialog.questions.findIndex(
    (question) => !selectionReady(question, stateOf(question)),
  );
  if (blocked >= 0) {
    if (blocked !== phase.value) {
      void show(blocked);
    }
    return;
  }
  emit(
    "submit",
    props.dialog.questions.map((question) => answerOf(question, stateOf(question))),
  );
}

function onOptionChange(label: string): void {
  const question = current.value;
  if (question === undefined) {
    return;
  }
  apply(question, selectOption(question, stateOf(question), label));
}

function onCustomInput(event: Event): void {
  const question = current.value;
  if (question === undefined) {
    return;
  }
  const value = (event.target as HTMLInputElement).value;
  apply(question, setCustomInput(question, stateOf(question), value));
}
</script>

<template>
  <form v-if="current" class="flex flex-col gap-3.5" novalidate @submit.prevent="submit">
    <header class="flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
        <span class="truncate text-[12.5px] font-medium text-fg">Questions</span>
      </div>
      <div class="flex shrink-0 items-center gap-1">
        <template v-if="total > 1">
          <button
            type="button"
            :disabled="busy || phase === 0"
            :class="PAGER"
            aria-label="Previous question"
            @click="previous"
          >
            <Icon name="chevron-left" size="h-4 w-4" />
          </button>
          <span aria-live="polite" class="font-mono text-[10.5px] tabular-nums text-dim">{{ phase + 1 }} of {{ total }}</span>
          <button
            type="button"
            :disabled="busy || last || !ready"
            :class="PAGER"
            aria-label="Next question"
            @click="next"
          >
            <Icon name="chevron-right" size="h-4 w-4" />
          </button>
        </template>
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded-[4px] text-faint transition-colors hover:bg-raised hover:text-dim focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
          title="Click to collapse"
          aria-expanded="true"
          @click="emit('collapse')"
        >
          <Icon name="chevron-down" size="h-3.5 w-3.5" />
        </button>
      </div>
    </header>

    <div ref="scroller" class="min-w-0 max-h-[50vh] overflow-y-auto overscroll-contain">
      <fieldset
        :key="current.id"
        :disabled="busy"
        class="min-w-0 border-0 p-0"
        :class="busy ? 'opacity-60' : ''"
      >
        <legend
          ref="legend"
          tabindex="-1"
          class="mb-1.5 flex flex-wrap items-center gap-2 text-[12.5px] font-medium text-fg outline-none"
        >
          <span class="whitespace-pre-wrap"><ColorText :text="current.question" /></span>
          <span
            v-if="current.header"
            class="rounded-[4px] bg-raised px-1.5 py-0.5 text-[10.5px] font-medium text-dim"
          >
            {{ current.header }}
          </span>
        </legend>

        <div class="flex flex-col gap-1.5">
          <label
            v-for="(option, index) in current.options"
            :key="`${index}-${option.label}`"
            class="flex cursor-pointer items-start gap-3 rounded-[8px] border border-line-strong/50 bg-surface/60 px-3.5 py-2.5 text-[12.5px] text-fg transition-colors hover:border-accent/70 hover:bg-raised"
          >
            <input
              class="peer sr-only"
              :type="current.multi ? 'checkbox' : 'radio'"
              :name="groupName(current)"
              :value="option.label"
              :aria-label="option.label"
              :checked="stateOf(current).selected.includes(option.label)"
              @change="onOptionChange(option.label)"
            />
            <span
              class="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center border transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-accent"
              :class="[
                current.multi ? 'rounded-[3px]' : 'rounded-full',
                stateOf(current).selected.includes(option.label)
                  ? 'border-accent bg-accent text-canvas'
                  : 'border-line-strong bg-surface text-transparent',
              ]"
            >
              <Icon v-if="current.multi" name="check" size="h-3 w-3" />
              <span v-else class="h-1.5 w-1.5 rounded-full bg-current" />
            </span>

            <span class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="flex flex-wrap items-center gap-2">
                <span class="leading-snug"><ColorText :text="option.label" /></span>
                <span
                  v-if="current.recommended === index"
                  class="rounded-[4px] bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium text-accent"
                >
                  Recommended
                </span>
              </span>
              <span
                v-if="option.description"
                class="whitespace-pre-wrap text-[11.5px] leading-relaxed text-dim"
              >
                {{ option.description }}
              </span>
              <pre
                v-if="option.preview"
                class="max-h-40 overflow-auto rounded-[6px] bg-canvas/70 p-2 font-mono text-[11px] whitespace-pre-wrap text-dim"
              >{{ option.preview }}</pre>
            </span>
          </label>

          <div class="flex flex-col gap-1">
            <label :for="customInputId(current)" class="text-[11.5px] text-dim">
              Other (type your own)
            </label>
            <input
              :id="customInputId(current)"
              type="text"
              spellcheck="false"
              :value="stateOf(current).custom"
              placeholder="Type your answer..."
              class="w-full rounded-[6px] border border-line-strong/60 bg-surface/80 px-3 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint/70 focus:border-accent/80 focus:ring-1 focus:ring-accent/40"
              @input="onCustomInput($event)"
            />
          </div>
        </div>
      </fieldset>
    </div>

    <div class="flex items-center justify-between gap-2">
      <slot name="secondary" />
      <button
        v-if="total > 1 && !last"
        type="button"
        :disabled="busy || !ready"
        :class="PRIMARY"
        @click="next"
      >
        <span>Next</span>
        <Icon name="chevron-right" size="h-3.5 w-3.5" />
      </button>
      <button v-else type="submit" :disabled="busy || !ready" :class="PRIMARY">
        <span>Submit</span>
        <Icon name="arrow-up" size="h-3.5 w-3.5" />
      </button>
    </div>
  </form>
</template>
