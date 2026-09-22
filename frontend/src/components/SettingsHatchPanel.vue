<script setup lang="ts">
/**
 * The raw config escape hatch (`docs/12` §12, `docs/13` §"Raw config escape hatch").
 *
 * The curated screen draws 314 rows; the other 191 keys have no control anywhere, and this file
 * is where a user reaches them, where a key a newer engine added survives, and where a bug
 * report reads what is actually set. Three rules hold the panel together:
 *
 * - **The file belongs to the engine.** `docs/13`'s protection 3 asks for a locked,
 *   merge-not-clobber write, and the honest answer is that this app never writes the file:
 *   every change goes through `omp config set`, which holds the engine's own cross-process lock
 *   and re-emits the document. Measured at v18.2.6, that writer drops every comment, so the
 *   editor says so instead of letting one disappear later as a surprise.
 * - **A refusal stops the whole plan.** `hatch::Plan::is_applicable` is the host's rule, and it
 *   is the right one: a partly applied edit leaves the file disagreeing with the text on screen.
 *   So the refusals are shown where the changes would be, and Apply goes away.
 * - **The host's copy is the truth.** The left pane holds the file as the host read it, Apply
 *   re-reads it afterwards, and the report on screen is the host's own — including the words for
 *   what a write costs, which are `apply::recorded_message`'s and never a promise of our own.
 */
import { computed, inject, isRef, onMounted, onUnmounted, ref, watch } from "vue";

import {
  settingsBackup,
  settingsHatch,
  settingsHatchApply,
  settingsHatchPlan,
  type SettingsApplyReport,
  type SettingsBackup,
  type SettingsHatch,
  type SettingsHatchChange,
  type SettingsHatchPlan,
} from "../bridge";
import Modal from "./Modal.vue";
import Icon from "./ui/Icon.vue";

/**
 * The word the host wants typed before it will write a key it does not surface
 * (`validate::confirm_hatch`), and the comparison it makes — case-insensitive.
 */
const CONFIRMATION = "CONFIRM";

/** The debounce on the plan: a round trip per keystroke would be a round trip per keystroke. */
const PLAN_DEBOUNCE_MS = 300;

/**
 * A card, a field, and the two buttons — the classes `DialogPanel` uses, so a primary action in
 * this screen is the same object as a primary action in the approval dialog.
 */
const CARD = "rounded-[10px] border border-line bg-surface p-3.5";
const LABEL = "text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint";
const FIELD = "rounded-[6px] bg-raised px-2 py-1 font-mono text-[12px] text-fg outline-none";
const PRIMARY =
  "rounded-[6px] bg-accent px-2.5 py-1 text-[12px] font-medium text-canvas disabled:opacity-40";
const SECONDARY = "rounded-[6px] px-2.5 py-1 text-[12px] text-dim hover:bg-raised hover:text-fg";
const QUIET = "rounded-[6px] px-2 py-0.5 text-[11.5px] text-dim hover:bg-raised hover:text-fg";

/**
 * The shell owns the thread the window is looking at, and provides it rather than passing it:
 * this panel is mounted for its section alone and takes no props. `components/settings/context.ts`
 * is the shell's side of that key, and says the same thing — a plain string, so a self-contained
 * panel has no module to import. A ref is accepted as well as a bare string, because the thread
 * changes while the screen is open.
 */
const injected = inject<unknown>("settings-thread", null);
const thread = computed<string | null>(() => {
  const held: unknown = isRef(injected) ? injected.value : injected;
  return typeof held === "string" ? held : null;
});

const scope = ref<"global" | "project">("global");
const hatch = ref<SettingsHatch | null>(null);
const loading = ref(false);
/** A failed read of the file itself, which leaves nothing to show. */
const failure = ref<string | null>(null);
/** Whether the thread in view has a project config file, so the switch is worth offering. */
const projectFile = ref(false);

/**
 * The editor's text, and the copy the plan is diffed against.
 *
 * `null` means there is no editor, for either of the two reasons `noEditorBecause` names: the
 * host sends no text for a file it cannot parse — a textarea seeded with nothing would invite
 * overwriting a file nobody here can read — and a project file is a read-only pane, because the
 * host's plan and apply resolve to the global one.
 */
const draft = ref<string | null>(null);
const baseline = ref<string | null>(null);
/** The backup in the editor, when the text came from one rather than from the file. */
const seeded = ref<SettingsBackup | null>(null);

const plan = ref<SettingsHatchPlan | null>(null);
const planError = ref<string | null>(null);
const planning = ref(false);

const applying = ref(false);
const confirmation = ref("");
/** The host's own refusal, when it would not apply the plan at all. */
const confirmRefusal = ref<string | null>(null);
const applyError = ref<string | null>(null);
const report = ref<SettingsApplyReport | null>(null);
/** The plan that was applied, for the restart class of each change the report counts. */
const appliedPlan = ref<SettingsHatchPlan | null>(null);

const backingUp = ref(false);
const backupNotice = ref<string | null>(null);
const backupError = ref<string | null>(null);

/** A control that would replace the editor's text, held until the user says to lose it. */
const discard = ref<{ what: string; run: () => void } | null>(null);

let timer: ReturnType<typeof setTimeout> | null = null;
let readSeq = 0;
let planSeq = 0;
let applySeq = 0;

const path = computed(() => hatch.value?.path ?? "");
const fileText = computed(() => hatch.value?.text ?? "");
const fileError = computed(() => hatch.value?.error ?? null);
const fileExists = computed(() => hatch.value?.exists ?? true);
const keys = computed(() => hatch.value?.keys ?? []);
const fileRefusals = computed(() => hatch.value?.refusals ?? []);

/**
 * The declarations are what decides this panel's reach, not the screen's wishes:
 * `settings_hatch_plan` and `settings_hatch_apply` take no scope, and the host resolves both
 * to the global file — as does `settings_backup`. So a project file, and the overlays the
 * sources banner shows, are read here and written in their own editor.
 */
const globalScope = computed(() => scope.value === "global");

/** Why there is no editor: a file the host cannot read, or a scope its apply cannot reach. */
const noEditorBecause = computed<null | "error" | "project">(() => {
  if (fileError.value !== null) return "error";
  return scope.value === "global" ? null : "project";
});
const canEdit = computed(() => noEditorBecause.value === null);

/**
 * The host sends backups newest first; the order is the panel's promise rather than the host's,
 * and a copy the host could not timestamp goes last rather than at the epoch.
 */
const backups = computed<SettingsBackup[]>(() =>
  [...(hatch.value?.backups ?? [])].sort((left, right) => (right.at ?? -1) - (left.at ?? -1)),
);

/**
 * Whether the switch is offered at all: a project file to switch to, or the way back from one
 * that turned out not to exist.
 */
const scopeChoice = computed(() => thread.value !== null && (projectFile.value || scope.value === "project"));

const canApply = computed(
  () =>
    draft.value !== null &&
    planError.value === null &&
    plan.value !== null &&
    plan.value.changes.length > 0 &&
    plan.value.refusals.length === 0 &&
    !applying.value &&
    (confirmRefusal.value === null || confirmation.value.trim().toUpperCase() === CONFIRMATION),
);

/** What the apply did, said only as far as the report supports it. */
const reportSummary = computed<string>(() => {
  const answer = report.value;
  if (answer === null) return "";
  const landed = answer.changes.filter((change) => change.ok).length;
  if (answer.changes.length === 0) return "Nothing was applied: the host refused the plan.";
  const counted = `${landed} of ${answer.changes.length} changes landed.`;
  return answer.refused.length > 0 ? `${counted} ${answer.refused.length} keys were refused before anything ran.` : counted;
});

const planRestarts = computed<string[]>(() => (plan.value === null ? [] : classesOf(plan.value.changes)));

/** The restart classes of the changes the report says landed, in the report's own order. */
const landedRestarts = computed<string[]>(() => {
  const answer = report.value;
  const asked = appliedPlan.value;
  if (answer === null || asked === null) return [];
  const restarts = new Map(asked.changes.map((change) => [change.key, change.restart]));
  const landed: { restart: string }[] = [];
  for (const change of answer.changes) {
    const restart = change.ok ? restarts.get(change.key) : undefined;
    if (restart !== undefined) landed.push({ restart });
  }
  return classesOf(landed);
});

/** The restart classes of a set of changes, once each, in the order the host listed them. */
function classesOf(changes: { restart: string }[]): string[] {
  return [...new Set(changes.map((change) => change.restart))];
}

/**
 * What a write costs, in `apply::recorded_message`'s own words — a plan may not promise more
 * than the restart class allows, which is what makes the class worth printing at all.
 */
function restartNote(restart: string): string {
  const promises: Record<string, string | undefined> = {
    live: "Live sessions changed now; new ones start with it.",
    sidecar: "Each session reads settings when it starts — restart to use it now.",
    app: "This one is baked into app state at launch, so it takes effect after a relaunch of the app.",
  };
  return promises[restart] ?? "This build does not know what it takes for this key to be in effect.";
}

/** One change as `before → after`, in the shapes the file can hold. */
function changeText(change: SettingsHatchChange): string {
  const from = shown(change.before);
  const to = change.action === "reset" ? "(the engine's default)" : shown(change.after);
  return `${from} → ${to}`;
}

function shown(value: unknown): string {
  if (value === null || value === undefined) return "(unset)";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

/** A backup's moment in the reader's own clock: the host reports seconds, not text. */
function takenAt(at: number | null): string {
  if (at === null) return "no timestamp";
  return new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** What the host said, whichever channel it used to say it. */
function describe(cause: unknown): string {
  if (cause instanceof Error) return cause.message;
  if (typeof cause === "string") return cause;
  return JSON.stringify(cause);
}

/** Which file is in front of the user, and why one might not be readable. */
async function load(): Promise<void> {
  const mine = ++readSeq;
  loading.value = true;
  failure.value = null;
  try {
    const answer = await settingsHatch(thread.value, scope.value);
    if (mine !== readSeq) return;
    hatch.value = answer;
    seedFromFile();
    await probeProject(mine);
  } catch (cause) {
    if (mine !== readSeq) return;
    hatch.value = null;
    failure.value = describe(cause);
  } finally {
    if (mine === readSeq) loading.value = false;
  }
}

/**
 * Re-read the file without touching the editor: after a write the left pane has to follow the
 * engine, while the text the user typed stays where it is.
 */
async function refresh(): Promise<void> {
  try {
    hatch.value = await settingsHatch(thread.value, scope.value);
  } catch (cause) {
    failure.value = describe(cause);
  }
}

/** Does the thread in view have a project file? The switch exists only where there is one. */
async function probeProject(mine: number): Promise<void> {
  if (thread.value === null) {
    projectFile.value = false;
    return;
  }
  if (scope.value === "project") {
    projectFile.value = hatch.value?.exists ?? false;
    return;
  }
  try {
    const answer = await settingsHatch(thread.value, "project");
    if (mine === readSeq) projectFile.value = answer.exists;
  } catch {
    if (mine === readSeq) projectFile.value = false;
  }
}

/** Put the file, as the host read it, in front of the editor — where an edit can land at all. */
function seedFromFile(): void {
  const file = hatch.value;
  seeded.value = null;
  draft.value = file !== null && canEdit.value ? file.text : null;
  baseline.value = draft.value;
  plan.value = { changes: [], refusals: [] };
  planError.value = null;
  confirmRefusal.value = null;
  confirmation.value = "";
  applyError.value = null;
  report.value = null;
  appliedPlan.value = null;
}

/**
 * Plan whatever the editor holds, debounced.
 *
 * The plan is the host's answer about one text and nothing else, so the panel never short-cuts it
 * — except for the one case where there is no question to ask, which is text identical to the copy
 * the plan is diffed against. A plan that has not come back yet is not shown either: the answer on
 * screen always belongs to the text above it, which is also why Apply is off while one is due.
 */
function schedulePlan(): void {
  if (timer !== null) clearTimeout(timer);
  if (draft.value === null || draft.value === baseline.value) {
    planSeq += 1;
    plan.value = { changes: [], refusals: [] };
    planError.value = null;
    planning.value = false;
    return;
  }
  plan.value = null;
  planError.value = null;
  planning.value = true;
  timer = setTimeout(() => void askForPlan(), PLAN_DEBOUNCE_MS);
}

async function askForPlan(): Promise<void> {
  const text = draft.value;
  if (text === null || text === baseline.value) return;
  const mine = ++planSeq;
  planning.value = true;
  try {
    const answer = await settingsHatchPlan(text);
    if (mine !== planSeq) return;
    plan.value = answer;
    planError.value = null;
  } catch (cause) {
    if (mine !== planSeq) return;
    plan.value = null;
    planError.value = describe(cause);
  } finally {
    if (mine === planSeq) planning.value = false;
  }
}

async function apply(): Promise<void> {
  const text = draft.value;
  if (!canApply.value || text === null) return;
  const mine = ++applySeq;
  applying.value = true;
  applyError.value = null;
  try {
    const answer = await settingsHatchApply(
      text,
      confirmRefusal.value === null ? null : confirmation.value.trim(),
    );
    if (mine !== applySeq) return;
    report.value = answer;
    appliedPlan.value = plan.value;
    confirmRefusal.value = null;
    confirmation.value = "";
    // The plan is diffed against what was just asked for, so the same edit is not offered twice.
    // The file itself is re-read: the engine rewrote it, and the left pane says what it wrote.
    baseline.value = text;
    await refresh();
  } catch (cause) {
    if (mine !== applySeq) return;
    const message = describe(cause);
    // The host refuses the whole plan *before* it writes anything, and its refusal is the only
    // place that says a plan needs the word typed: a plan carries no danger level, and deriving
    // one here would be the second copy of the catalogue the host deliberately owns. So the first
    // attempt is the question, and asking it writes nothing. A failure *after* the word is typed
    // is a different animal, and is shown as the error it is.
    if (confirmRefusal.value === null) confirmRefusal.value = message;
    else applyError.value = message;
  } finally {
    if (mine === applySeq) applying.value = false;
  }
}

async function takeBackup(): Promise<void> {
  backingUp.value = true;
  backupError.value = null;
  try {
    backupNotice.value = await settingsBackup();
    await refresh();
  } catch (cause) {
    backupError.value = describe(cause);
  } finally {
    backingUp.value = false;
  }
}

/**
 * Run a control that replaces whatever the editor holds — a scope change, a re-read, a backup —
 * but not before saying that the edits it would replace have not been written yet.
 */
function guardEdits(what: string, run: () => void): void {
  if (draft.value !== null && draft.value !== baseline.value) {
    // Named `run`, not `then`: an object with a `then` function is a *thenable*, and `await`
    // on one calls it with a resolver it does not expect and waits forever.
    discard.value = { what, run };
    return;
  }
  run();
}

function discardNow(): void {
  const pending = discard.value;
  discard.value = null;
  pending?.run();
}

function switchScope(next: "global" | "project"): void {
  if (next === scope.value) return;
  guardEdits(`Switching to the ${next} file`, () => {
    scope.value = next;
  });
}

/** Load a backup into the editor, so its plan can be read before anything is written. */
function revert(backup: SettingsBackup): void {
  // A copy of a project file is not a thing this panel can put back: the host's apply writes the
  // global file only, so loading one would offer a plan that could never be applied to it.
  if (!globalScope.value) return;
  guardEdits(`Loading the backup taken ${takenAt(backup.at)}`, () => {
    draft.value = backup.text;
    seeded.value = backup;
  });
}

function backToFile(): void {
  guardEdits("Reading the file again", () => {
    seedFromFile();
  });
}

function reread(): void {
  guardEdits("Reading the file again", () => void load());
}

watch([draft, baseline], () => schedulePlan());
watch(scope, () => void load());
watch(thread, () => {
  if (thread.value === null && scope.value === "project") {
    // A project file belongs to a thread; with none open the honest scope is the global one,
    // and the scope watcher does the reading.
    scope.value = "global";
    return;
  }
  void load();
});

onMounted(() => void load());
onUnmounted(() => {
  if (timer !== null) clearTimeout(timer);
});
</script>

<template>
  <section class="flex min-h-0 flex-1 flex-col gap-3 overflow-auto px-5 py-4" data-hatch-panel>
    <div v-if="hatch === null" :class="CARD" data-hatch-read>
      <p v-if="loading" class="text-[12.5px] text-faint">reading the config file…</p>
      <template v-else>
        <p class="flex gap-2 text-[12.5px] leading-relaxed text-err">
          <Icon name="alert" class="mt-px h-3.5 w-3.5 shrink-0" />
          {{ failure ?? "the config file could not be read" }}
        </p>
        <button type="button" :class="[SECONDARY, 'mt-2']" data-hatch-action="retry" @click="load">
          try again
        </button>
      </template>
    </div>

    <template v-else>
      <!--
        Which file is in front of the user, and the two things that are the file's rather than
        the editor's: its scope, and the moment to take a copy of it.
      -->
      <div :class="CARD">
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Icon name="file" class="h-4 w-4 shrink-0 text-faint" />
          <span class="selectable min-w-0 flex-1 truncate font-mono text-[12.5px] text-fg" :title="path">
            {{ path }}
          </span>

          <div v-if="scopeChoice" class="flex items-center gap-1" role="group" aria-label="which config file">
            <button
              v-for="pick in (['global', 'project'] as const)"
              :key="pick"
              type="button"
              class="rounded-[6px] px-2.5 py-1 text-[12px]"
              :class="scope === pick ? 'bg-raised text-fg' : 'text-dim hover:text-fg'"
              :aria-pressed="scope === pick"
              :data-hatch-scope="pick"
              :title="
                pick === 'global'
                  ? 'the file the engine writes for every project'
                  : 'this thread\u2019s workspace file'
              "
              @click="switchScope(pick)"
            >
              {{ pick === "global" ? "Global" : "Project" }}
            </button>
          </div>

          <button
            type="button"
            :class="QUIET"
            data-hatch-action="reread"
            title="read the file again, discarding what the editor holds"
            @click="reread"
          >
            <Icon name="refresh" class="mr-1 inline h-3.5 w-3.5 align-[-2px]" />
            re-read
          </button>
        </div>

        <p v-if="!fileExists" class="mt-2 flex gap-2 text-[11.5px] leading-relaxed text-warn">
          <Icon name="alert" class="mt-px h-3.5 w-3.5 shrink-0" />
          <span v-if="canEdit">
            Nothing is at this path yet, so there is nothing to edit here. Writing below is still
            worth it: each change goes to the engine, which creates the file.
          </span>
          <span v-else>Nothing is at this path yet.</span>
        </p>
      </div>

      <!--
        A file that cannot be parsed has no editor, and says so where the editor would have been:
        the host deliberately sends no text for it, and an empty box would offer to overwrite a
        file nobody here can read.
      -->
      <div v-if="fileError !== null" :class="CARD" data-hatch-error>
        <p class="flex items-center gap-2 text-[12.5px] text-warn">
          <Icon name="alert" class="h-3.5 w-3.5 shrink-0" />
          This file exists and cannot be read.
        </p>
        <pre
          class="selectable mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-[6px] bg-canvas p-2.5 font-mono text-[11.5px] leading-relaxed text-dim"
        >{{ fileError }}</pre>
        <p class="mt-2 text-[11.5px] leading-relaxed text-faint">
          The engine's own words, and no editor: the host sends no text for a file it cannot parse.
          <template v-if="globalScope">
            A backup below can be loaded into the editor, which shows the plan it would produce
            before anything is written.
          </template>
          <template v-else>Copies of the file are listed below.</template>
        </p>
      </div>

      <div class="grid gap-3 lg:grid-cols-2">
        <!-- Left: what the file says, as the host read it — the side of the diff nothing edits. -->
        <div :class="[CARD, 'flex min-w-0 flex-col']">
          <h2 :class="LABEL">this file sets</h2>
          <p v-if="keys.length === 0" class="py-1.5 text-[11.5px] text-faint">No settings.</p>
          <ul v-else class="selectable mt-1 max-h-52 overflow-auto">
            <li
              v-for="key in keys"
              :key="key"
              class="truncate font-mono text-[11.5px] leading-relaxed text-dim"
              :title="key"
            >
              {{ key }}
            </li>
          </ul>

          <h2 :class="[LABEL, 'mt-3']">the file, as the host read it</h2>
          <pre
            v-if="fileError === null"
            class="selectable mt-1 min-h-24 flex-1 overflow-auto whitespace-pre rounded-[6px] bg-canvas p-2.5 font-mono text-[11.5px] leading-relaxed text-dim"
          >{{ fileText }}</pre>
          <p v-else class="mt-1 text-[11.5px] text-faint">—</p>
        </div>

        <!-- Right: the same file, by hand — where the host's apply can reach it. -->
        <div :class="[CARD, 'flex min-w-0 flex-col']">
          <div class="flex items-center gap-2">
            <h2 :class="LABEL">{{ canEdit ? "the file, by hand" : "the file" }}</h2>
            <button
              v-if="seeded !== null"
              type="button"
              :class="[QUIET, 'ml-auto']"
              data-hatch-action="back-to-file"
              @click="backToFile"
            >
              back to the file
            </button>
          </div>

          <!--
            Refusals are the file's own, not the edit's: parts of it this app will not act on, and
            they belong above the text so they are read before it is edited.
          -->
          <div
            v-if="fileRefusals.length > 0"
            class="mt-2 rounded-[6px] border border-line bg-canvas p-2.5"
            data-hatch-refusals
          >
            <p class="flex items-center gap-2 text-[11.5px] text-warn">
              <Icon name="alert" class="h-3.5 w-3.5 shrink-0" />
              {{ fileRefusals.length === 1 ? "One part of this file" : `${fileRefusals.length} parts of this file` }}
              the app will not act on.
            </p>
            <ul class="selectable mt-1.5 flex flex-col gap-1">
              <li v-for="(refusal, at) in fileRefusals" :key="at" class="text-[11.5px] leading-relaxed">
                <span v-if="refusal.key !== ''" class="font-mono text-dim">{{ refusal.key }}</span>
                <span :class="refusal.key === '' ? 'text-dim' : 'text-faint'"> — {{ refusal.reason }}</span>
              </li>
            </ul>
          </div>

          <p v-if="seeded !== null" class="mt-2 text-[11.5px] leading-relaxed text-accent-bright">
            Editing the backup taken {{ takenAt(seeded.at) }}. Nothing is written until you apply,
            and the plan below is against the file as it is now.
          </p>

          <textarea
            v-if="draft !== null"
            :value="draft"
            class="mt-2 min-h-96 w-full flex-1 resize-y rounded-[6px] bg-raised p-2.5 font-mono text-[12px] leading-relaxed text-fg outline-none"
            wrap="off"
            spellcheck="false"
            aria-label="the config file, by hand"
            data-hatch-editor
            @input="draft = ($event.target as HTMLTextAreaElement).value"
          ></textarea>
          <pre
            v-else-if="noEditorBecause === 'project'"
            class="selectable mt-2 min-h-24 flex-1 overflow-auto whitespace-pre rounded-[6px] bg-canvas p-2.5 font-mono text-[11.5px] leading-relaxed text-dim"
            data-hatch-readonly
          >{{ fileText }}</pre>
          <p v-else class="mt-2 text-[11.5px] leading-relaxed text-faint">
            No editor for this file:
            {{ globalScope ? "a backup below can be loaded into one." : "there is no readable text to hand over." }}
          </p>

          <p v-if="draft !== null" class="mt-2 text-[11.5px] leading-relaxed text-faint">
            YAML, and the engine's file rather than the app's. The engine rewrites the document in
            its own key order and keeps no comments, so a note written here is a setting that will
            be gone. Nothing in this panel writes the file: each change goes through the engine's
            own <span class="font-mono">omp config set</span>.
          </p>
          <p v-else-if="noEditorBecause === 'project'" class="mt-2 text-[11.5px] leading-relaxed text-faint">
            Reading only, and not by choice: the host's plan and apply commands carry no scope and
            resolve to the global file, as does its backup. Hand-edit a project file in your own
            editor — the engine reads it when the next session starts — or switch to Global to set
            the same key for every project.
          </p>
        </div>
      </div>

      <div v-if="draft !== null" :class="CARD" data-hatch-plan>
        <div class="flex items-center gap-2">
          <h2 :class="LABEL">what this would change</h2>
          <span v-if="planning" class="ml-auto text-[10.5px] text-faint">checking…</span>
          <span
            v-else-if="plan !== null && plan.changes.length > 0"
            class="ml-auto font-mono text-[10.5px] text-faint"
          >{{ plan.changes.length }}</span>
        </div>

        <p v-if="planError !== null" class="mt-2 flex gap-2 text-[12px] leading-relaxed text-err">
          <Icon name="alert" class="mt-px h-3.5 w-3.5 shrink-0" />
          {{ planError }}
        </p>

        <template v-else-if="plan !== null">
          <p v-if="plan.changes.length === 0 && plan.refusals.length === 0" class="mt-2 text-[11.5px] text-faint">
            Nothing to apply — the editor holds the file as it is.
          </p>

          <ul v-if="plan.changes.length > 0" class="selectable mt-2 flex flex-col gap-1.5">
            <li
              v-for="change in plan.changes"
              :key="change.key"
              class="flex items-baseline gap-2"
              data-hatch-change
            >
              <span class="shrink-0 font-mono text-[11.5px] text-fg">{{ change.key }}</span>
              <span class="shrink-0 font-mono text-[10.5px] uppercase tracking-[0.09em] text-faint">
                {{ change.action === "reset" ? "reset" : "set" }}
              </span>
              <span class="min-w-0 flex-1 truncate font-mono text-[11.5px] text-dim" :title="changeText(change)">
                {{ changeText(change) }}
              </span>
              <span class="shrink-0 font-mono text-[10.5px] uppercase tracking-[0.09em] text-faint">
                {{ change.restart }}
              </span>
            </li>
          </ul>

          <!-- A refused plan is not a partial one: the host would not write any of it. -->
          <div
            v-if="plan.refusals.length > 0"
            class="mt-2 rounded-[6px] border border-line bg-canvas p-2.5"
            data-hatch-plan-refusals
          >
            <p class="text-[12px] text-warn">This plan applies nothing.</p>
            <p class="mt-0.5 text-[11.5px] leading-relaxed text-faint">
              A half-applied edit would leave the file disagreeing with the text above, so one
              refusal takes the whole plan with it.
            </p>
            <ul class="selectable mt-1.5 flex flex-col gap-1">
              <li v-for="(refusal, at) in plan.refusals" :key="at" class="text-[11.5px] leading-relaxed">
                <span v-if="refusal.key !== ''" class="font-mono text-dim">{{ refusal.key }}</span>
                <span :class="refusal.key === '' ? 'text-dim' : 'text-faint'"> — {{ refusal.reason }}</span>
              </li>
            </ul>
          </div>

          <div v-if="plan.changes.length > 0 && plan.refusals.length === 0" class="mt-2 flex flex-col gap-1">
            <p v-for="which in planRestarts" :key="which" class="text-[11.5px] leading-relaxed text-faint">
              <span class="font-mono uppercase tracking-[0.09em]">{{ which }}</span> — {{ restartNote(which) }}
            </p>
          </div>
        </template>

        <div class="mt-3 flex flex-wrap items-center gap-2 border-t border-line pt-3">
          <button
            type="button"
            :class="PRIMARY"
            data-hatch-action="apply"
            :disabled="!canApply"
            @click="apply"
          >
            {{ applying ? "applying…" : confirmRefusal === null ? "apply" : "apply with CONFIRM" }}
          </button>
          <span class="text-[11.5px] text-faint">
            one <span class="font-mono">omp config set</span> per change, in the order shown
          </span>
        </div>

        <!--
          The host refuses the whole plan before writing anything when it wants the word typed, so
          this block is how the panel learns a plan needs it — and the sentence under it is the
          host's, naming the keys that forced it.
        -->
        <div
          v-if="confirmRefusal !== null"
          class="mt-2 rounded-[6px] border border-line bg-canvas p-2.5"
          data-hatch-confirm
        >
          <p class="text-[12px] text-warn">The host would not apply this, and nothing was written.</p>
          <pre
            class="selectable mt-1 whitespace-pre-wrap font-mono text-[11.5px] leading-relaxed text-dim"
          >{{ confirmRefusal }}</pre>
          <label class="mt-2 flex flex-wrap items-center gap-2 text-[11.5px] text-faint">
            <span>
              Type <span class="font-mono text-fg">{{ CONFIRMATION }}</span> to apply it anyway
            </span>
            <input
              v-model="confirmation"
              type="text"
              :class="[FIELD, 'selectable w-32']"
              :placeholder="CONFIRMATION"
              spellcheck="false"
              aria-label="type CONFIRM to apply this plan"
              data-hatch-confirmation
            />
          </label>
        </div>

        <p v-if="applyError !== null" class="mt-2 flex gap-2 text-[12px] leading-relaxed text-err" data-hatch-apply-error>
          <Icon name="alert" class="mt-px h-3.5 w-3.5 shrink-0" />
          {{ applyError }}
        </p>
      </div>

      <div v-if="report !== null" :class="CARD" data-hatch-report>
        <h2 :class="LABEL">what the apply did</h2>
        <p class="mt-2 text-[12.5px] text-fg">{{ reportSummary }}</p>
        <ul class="selectable mt-1.5 flex flex-col gap-1">
          <li v-for="(result, at) in report.changes" :key="at" class="flex items-baseline gap-2 text-[11.5px]">
            <Icon
              :name="result.ok ? 'check' : 'close'"
              class="h-3.5 w-3.5 shrink-0 self-center"
              :class="result.ok ? 'text-ok' : 'text-err'"
            />
            <span class="shrink-0 font-mono text-fg">{{ result.key }}</span>
            <span class="shrink-0 font-mono text-[10.5px] uppercase tracking-[0.09em] text-faint">
              {{ result.action }}
            </span>
            <span
              class="min-w-0 flex-1 truncate"
              :class="result.ok ? 'text-dim' : 'text-err'"
              :title="result.error ?? ''"
            >
              {{ result.ok ? "written by the engine" : (result.error ?? "the engine refused it") }}
            </span>
          </li>
          <li
            v-for="(refusal, at) in report.refused"
            :key="`refused-${at}`"
            class="flex items-baseline gap-2 text-[11.5px]"
            data-hatch-report-refusal
          >
            <Icon name="alert" class="h-3.5 w-3.5 shrink-0 self-center text-warn" />
            <span class="shrink-0 font-mono text-dim">{{ refusal.key }}</span>
            <span class="min-w-0 flex-1 text-faint">not applied — {{ refusal.reason }}</span>
          </li>
        </ul>
        <div v-if="landedRestarts.length > 0" class="mt-2 flex flex-col gap-1">
          <p v-for="which in landedRestarts" :key="which" class="text-[11.5px] leading-relaxed text-faint">
            <span class="font-mono uppercase tracking-[0.09em]">{{ which }}</span> — {{ restartNote(which) }}
          </p>
        </div>
      </div>

      <div :class="CARD" data-hatch-backups>
        <div class="flex items-center gap-2">
          <h2 :class="LABEL">backups</h2>
          <button
            v-if="globalScope"
            type="button"
            :class="[SECONDARY, 'ml-auto']"
            data-hatch-action="backup"
            :disabled="backingUp"
            @click="takeBackup"
          >
            {{ backingUp ? "copying…" : "back up now" }}
          </button>
        </div>
        <p class="mt-1 text-[11.5px] leading-relaxed text-faint">
          <template v-if="globalScope">
            The host copies this file aside before a write, so a hand-edit can be undone. A copy is
            a whole file: loading one into the editor shows the plan it would produce, and nothing
            is written until that plan is applied.
          </template>
          <template v-else>
            Copies of this file, newest first. They are listed rather than loaded: the host's apply
            and its backup both address the global file, which is the scope the switch above can
            write.
          </template>
        </p>
        <p v-if="backupNotice !== null" class="selectable mt-1.5 font-mono text-[11.5px] text-ok" data-hatch-backup-notice>
          {{ backupNotice }}
        </p>
        <p v-if="backupError !== null" class="mt-1.5 flex gap-2 text-[11.5px] text-err" data-hatch-backup-error>
          <Icon name="alert" class="mt-px h-3.5 w-3.5 shrink-0" />
          {{ backupError }}
        </p>
        <p v-if="backups.length === 0" class="mt-2 text-[11.5px] text-faint">No copy of this file yet.</p>
        <ul v-else class="mt-2 flex flex-col">
          <li
            v-for="(backup, at) in backups"
            :key="at"
            class="flex items-center gap-2 rounded-[6px] px-1.5 py-1 hover:bg-raised"
            data-hatch-backup
          >
            <Icon name="clock" class="h-3.5 w-3.5 shrink-0 text-faint" />
            <span
              class="shrink-0 text-[12px] text-dim"
              :title="backup.at === null ? 'the host did not timestamp this copy' : new Date(backup.at * 1000).toISOString()"
            >{{ takenAt(backup.at) }}</span>
            <span class="selectable min-w-0 flex-1 truncate font-mono text-[11.5px] text-faint" :title="backup.path">
              {{ backup.path }}
            </span>
            <button
              v-if="globalScope"
              type="button"
              :class="QUIET"
              data-hatch-action="revert"
              :title="`load this copy into the editor`"
              @click="revert(backup)"
            >
              revert
            </button>
          </li>
        </ul>
      </div>
    </template>

    <Modal v-if="discard !== null" title="discard the edits in the editor?" @close="discard = null">
      <p class="mb-3 text-[12.5px] leading-relaxed text-fg">
        {{ discard.what }} would replace the text and the plan with it. Nothing has been written
        yet, so the file itself is untouched.
      </p>
      <div class="flex gap-1.5">
        <button type="button" :class="PRIMARY" @click="discardNow">discard the edits</button>
        <button type="button" :class="SECONDARY" @click="discard = null">keep editing</button>
      </div>
    </Modal>
  </section>
</template>
