<script setup lang="ts">
/**
 * The settings screen (`docs/12` §12, `docs/13` for the mapping it renders).
 *
 * Full-page: the settings rail on the left, the section on the right, and the window's own
 * chrome above them. It is not a panel over a thread, because a setting is not about the thread
 * on screen — the project config file is *found* through that thread's workspace, which is why
 * the screen asks about it and why it re-reads when the thread changes.
 *
 * The catalog is the host's: sections, groups, enum options and the words for why a row is
 * dangerous. The frontend applies one small importance policy to their presentation order;
 * unlisted and newly added OMP entries retain their host order instead of disappearing.
 *
 * Three things this file owns, and they are all *one* decision each:
 *
 * 1. **the write path** — a control asks for a value, the row's danger level decides whether a
 *    typed confirmation comes first, and the engine's own sentence is what the row then shows;
 * 2. **the restart** — a `sidecar` write is not live, so the row offers the restart and this
 *    screen performs it once, for every session, and reports what happened to each;
 * 3. **the search's landing** — a hit or a "needs" link moves the rail *and* puts the row in
 *    front of the reader, which is the difference between a working search and a list.
 */
import { computed, nextTick, onMounted, provide, ref, watch } from "vue";

import {
  resetSetting,
  setSetting,
  settingsRestartSessions,
  settingsScreen,
  type ModelOption,
  type SettingsScreen as SettingsScreenModel,
  type SettingsRow,
} from "../bridge";
import {
  locate as locating,
  navGroups,
  openingSection,
  prioritizeSettings,
  searchRows,
  SETTINGS_INFO_ID,
  sourceLines,
} from "../lib/settings";
import SettingsHatchPanel from "./SettingsHatchPanel.vue";
import { SETTINGS_CONTEXT, SETTINGS_THREAD, type SettingsContext } from "./settings/context";
import SettingsConfirm from "./settings/SettingsConfirm.vue";
import SettingsRail from "./settings/SettingsRail.vue";
import SettingsSectionPage from "./settings/SettingsSectionPage.vue";
import SettingsSources from "./settings/SettingsSources.vue";

const props = defineProps<{
  /** The session whose workspace holds the project config file; `null` with nothing open. */
  thread: string | null;
  /** The OS user and app version, for the rail's footer. */
  identity: { user: string | null; version: string };
  /** How many sessions the catalogue holds. */
  sessions: number;
  /** The cached model catalogue, for the record editor's picker. */
  models: ModelOption[];
  /** Favourite model keys, in the user's order. */
  favourites: string[];
  refreshing: boolean;
  /** The active session's approval mode, for the General section's mode row. */
  mode: string | null;
}>();

const emit = defineEmits<{
  /** Back to the thread view. */
  back: [];
  /** Something the app owns changed — the approval mode restarts a sidecar. */
  changed: [];
  /** The favourites order changed; the shell stores it. */
  favourites: [keys: string[]];
  /** A failure with nowhere on the row to show it. */
  failed: [message: string];
}>();

/** What the controls below need from the shell. Provided, not threaded through four levels. */
const context: SettingsContext = {
  thread: computed(() => props.thread),
  mode: computed(() => props.mode),
  models: computed(() => props.models),
  favourites: computed(() => props.favourites),
  refreshing: computed(() => props.refreshing),
  setFavourites: (keys) => emit("favourites", keys),
};
provide(SETTINGS_CONTEXT, context);
// The raw-config panel takes no props and still has to know which workspace's file to read.
provide(SETTINGS_THREAD, context.thread);

const screen = ref<SettingsScreenModel | null>(null);
const loading = ref(true);
const failure = ref<string | null>(null);
const active = ref("");
const query = ref("");
/** The row a search hit or a "needs" link landed on; cleared by the next plain navigation. */
const highlight = ref<string | null>(null);
const pending = ref(new Set<string>());
/** The engine's own sentence per row, after a write. Never shortened. */
const messages = ref<Record<string, string | undefined>>({});
const errors = ref<Record<string, string | undefined>>({});
/** The write a confirmation is holding, until it is typed or abandoned. */
const held = ref<{ key: string; label: string; why: string; value: unknown; clear: boolean } | null>(null);
const restarting = ref(false);
const note = ref<string | null>(null);

const content = ref<HTMLElement | null>(null);

const groups = computed(() => (screen.value === null ? [] : navGroups(screen.value)));
const hits = computed(() => (screen.value === null ? [] : searchRows(screen.value, query.value)));
const lines = computed(() => (screen.value === null ? [] : sourceLines(screen.value)));
const section = computed(() => screen.value?.sections.find((entry) => entry.id === active.value) ?? null);
/** The escape hatch's own section: host-composed title and blurb, the panel's own body. */
const hatch = computed(() => section.value?.id === "raw-config");
/** App-owned context about where settings came from and catalog compatibility. */
const info = computed(() => active.value === SETTINGS_INFO_ID);

/** Where a gated row's `needs` key lives, resolved against the catalog the screen holds. */
function needsOf(key: string): { sectionId: string; title: string } | null {
  const found = screen.value === null ? null : locating(screen.value, key);
  return found === null ? null : { sectionId: found.section.id, title: found.section.title };
}

/** Read the whole screen from the host. One round trip, and the only source of its shape. */
async function read(): Promise<void> {
  try {
    const next = await settingsScreen(props.thread);
    screen.value = prioritizeSettings(next);
    failure.value = null;
    // A section the new catalog does not have (or the first read) opens on General.
    if (active.value !== SETTINGS_INFO_ID && !next.sections.some((entry) => entry.id === active.value)) {
      active.value = openingSection(next);
    }
  } catch (cause) {
    failure.value = describe(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void read());
// The project config file is the *thread's* workspace's, so the screen re-reads when the shell
// changes the thread — and shows no project file when there is none, which is the honest answer.
watch(
  () => props.thread,
  () => void read(),
);

/**
 * A row asked for a value.
 *
 * The danger gate is here rather than in the row because it is the *write* that is gated: a
 * level-`confirm` key does not go to the engine without its own name typed, so the write waits
 * behind the dialog and a cancelled dialog writes nothing at all.
 */
function request(row: SettingsRow, value: unknown, clear: boolean): void {
  // Only a setting row can be written; the other two roles have no key for the engine to take.
  if (row.role !== "setting") return;

  if (row.danger?.level === "confirm") {
    held.value = { key: row.key, label: row.label, why: row.danger.why, value, clear };
    return;
  }
  void commit(row.key, value, clear, null);
}

/** Hand one value (or a reset) to the engine and show exactly what it said. */
async function commit(key: string, value: unknown, clear: boolean, confirmation: string | null): Promise<void> {
  pending.value = new Set([...pending.value, key]);
  errors.value = { ...errors.value, [key]: undefined };
  messages.value = { ...messages.value, [key]: undefined };

  try {
    const outcome = clear
      ? await resetSetting(key, confirmation)
      : await setSetting(key, value, confirmation);
    messages.value = { ...messages.value, [key]: outcome.message };
    // Re-read rather than patching the row: the value moved, and so did where it is written.
    await read();
  } catch (cause) {
    errors.value = { ...errors.value, [key]: describe(cause) };
  } finally {
    const next = new Set(pending.value);
    next.delete(key);
    pending.value = next;
  }
}

/** The confirmation was typed: the held write goes through with the typed name. */
function confirmHeld(typed: string): void {
  const write = held.value;
  held.value = null;
  if (write === null) return;
  void commit(write.key, write.value, write.clear, typed);
}

/**
 * Restart every live session, so what is already written is in effect.
 *
 * One action for the whole screen rather than one per row, because that is what the host
 * command does — and its answer names every session it skipped, which is the part a user needs
 * (a session with no sidecar cannot be restarted, and pretending otherwise would be the lie the
 * restart class exists to prevent).
 */
async function restart(): Promise<void> {
  if (restarting.value) return;
  restarting.value = true;
  note.value = null;
  try {
    const report = await settingsRestartSessions();
    const restarted = report.restarted.length;
    const skipped = report.skipped.map((entry) => `${entry.thread} (${entry.reason})`).join(", ");
    note.value = `Restarted ${restarted} ${restarted === 1 ? "session" : "sessions"}${
      report.skipped.length === 0
        ? " — everything written so far is in effect."
        : `. ${report.skipped.length} skipped: ${skipped}.`
    }`;
    await read();
  } catch (cause) {
    // No row to hold this one — the restart is the screen's own action — so it goes to the
    // shell's strip, which is where a failure with nowhere to live already goes.
    emit("failed", describe(cause));
  } finally {
    restarting.value = false;
  }
}

/** The approval mode changed, which restarts a sidecar: both the session and this screen move. */
function onChanged(): void {
  emit("changed");
  void read();
}

/** An action row was dispatched: this screen owns both of them. */
function dispatch(action: string): void {
  if (action === "open-config-file") {
    goTo("raw-config", "");
    return;
  }
  // A verb this build has no path for: said out loud rather than silently doing nothing.
  note.value = `this build has no control for \`${action}\``;
}

/** Navigate to a section, optionally putting one of its rows in front of the reader. */
function goTo(sectionId: string, target: string): void {
  active.value = sectionId;
  highlight.value = target === "" ? null : target;
  if (target === "") return;
  void nextTick(() => {
    // Inside the content scroller, so a hit lands centred instead of at the scroll pane's edge.
    const escaped = target.replace(/"/g, '\\"');
    content.value?.querySelector<HTMLElement>(`[data-row-key="${escaped}"]`)?.scrollIntoView({ block: "center" });
  });
}

function describe(cause: unknown): string {
  return typeof cause === "string" ? cause : cause instanceof Error ? cause.message : String(cause);
}
</script>

<template>
  <div class="flex min-h-0 flex-1 bg-canvas" data-view="settings">
    <SettingsRail
      :groups="groups"
      :active="active"
      :query="query"
      :hits="hits"
      :identity="props.identity"
      :sessions="props.sessions"
      @back="emit('back')"
      @select="goTo($event, '')"
      @query="query = $event"
      @hit="(sectionId: string, target: string) => goTo(sectionId, target)"
    />

    <main ref="content" class="min-h-0 flex-1 overflow-auto">
      <div class="mx-auto w-full max-w-[760px] px-10 py-8">
        <p
          v-if="note !== null && !info"
          class="mb-5 rounded-[8px] border border-line/60 bg-surface/60 px-3 py-2 text-[11.5px] leading-relaxed text-dim"
          data-note=""
        >
          {{ note }}
        </p>

        <p v-if="loading" class="mt-6 text-[12.5px] text-faint" data-loading="">
          reading the settings catalog…
        </p>

        <div v-else-if="failure !== null" class="mt-6 flex flex-col items-start gap-2" data-failure="">
          <p class="text-[12.5px] leading-relaxed text-err">{{ failure }}</p>
          <button
            class="rounded-[6px] border border-line px-2.5 py-1 text-[11.5px] text-dim hover:border-line-strong hover:text-fg"
            data-action="settings-retry"
            @click="read"
          >
            read it again
          </button>
        </div>

        <template v-else-if="info && screen !== null">
          <div>
            <h1 class="text-[20px] text-fg">Settings info</h1>
            <p class="mt-1 text-[12.5px] leading-relaxed text-dim">
              Configuration sources and compatibility details for this OMP settings catalog.
            </p>
            <div class="mt-6">
              <SettingsSources
                :lines="lines"
                :drift="screen.drift"
                :catalog-version="screen.catalogVersion"
                :note="note"
                @raw="goTo('raw-config', '')"
              />
            </div>
          </div>
        </template>

        <template v-else-if="section !== null">
          <div>
            <SettingsSectionPage
              v-if="!hatch"
              :section="section"
              :pending="pending"
              :messages="messages"
              :errors="errors"
              :highlight="highlight"
              :locate="needsOf"
              @write="(row: SettingsRow, value: unknown) => request(row, value, false)"
              @reset="request($event, null, true)"
              @restart="restart"
              @navigate="goTo"
              @action="dispatch"
              @changed="onChanged"
            />

            <!--
              The raw config: the host's title and blurb, then the hatch's own panel. Nothing
              here draws the file — that surface owns the two panes, the diff and the backup.
            -->
            <template v-else>
              <h1 class="text-[20px] text-fg">{{ section.title }}</h1>
              <p v-if="section.blurb !== ''" class="mt-1 text-[12.5px] leading-relaxed text-dim">
                {{ section.blurb }}
              </p>
              <div class="mt-6">
                <SettingsHatchPanel />
              </div>
            </template>
          </div>
        </template>
      </div>
    </main>

    <SettingsConfirm
      v-if="held !== null"
      :key-name="held.key"
      :label="held.label"
      :why="held.why"
      @cancel="held = null"
      @confirm="confirmHeld"
    />
  </div>
</template>
