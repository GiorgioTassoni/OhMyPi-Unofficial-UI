import { invoke } from "./core";

/**
 * The whole settings screen, as the host composes it.
 *
 * It mirrors `omp_settings::screen::Screen`. The host sends *everything* the screen draws —
 * the catalog is not in the frontend — so opening settings is one round trip and the
 * frontend holds no second copy of 505 keys of metadata to drift from.
 */
export interface SettingsScreen {
  /** The engine release the shipped catalog was generated against. */
  catalogVersion: string;
  sources: SettingsSources;
  drift: SettingsDrift;
  sections: SettingsSection[];
}

/** Every config file the app can see, and why one might be unreadable. */
export interface SettingsSources {
  agentDir: string;
  globalFile: SettingsFile;
  projectFile: SettingsFile;
  /** Read-only overlays (`PI_CONFIG_FILES`): shown, never edited. */
  overlays: SettingsFile[];
}

/** One config file, as the banner describes it. */
export interface SettingsFile {
  path: string;
  exists: boolean;
  /** A syntax error's own words, when the file could not be parsed. */
  error: string | null;
  /** Parts of the file the app will not act on. */
  refusals: SettingsRefusal[];
}

/** Something in a config file the app will not act on, and why. */
export interface SettingsRefusal {
  key: string;
  reason: string;
}

/** The engine knows settings this build does not, or the other way round. */
export interface SettingsDrift {
  unknownKeys: string[];
  missingKeys: string[];
}

/** One section of the settings nav. */
export interface SettingsSection {
  id: string;
  title: string;
  blurb: string;
  /** `app`, `agent` or `system` — the nav column it belongs to. */
  navGroup: string;
  /** A name from `lib/icons.ts`. */
  icon: string;
  groups: SettingsGroup[];
}

/** One sub-group inside a section (the engine's own group name). */
export interface SettingsGroup {
  name: string;
  rows: SettingsRow[];
}

/** A row is one of three things, discriminated by `role`. */
export type SettingsRow = SettingsSettingRow | SettingsStaticRow | SettingsActionRow;

/** A settings key with a control. */
export interface SettingsSettingRow {
  role: "setting";
  key: string;
  label: string;
  description: string;
  /** The schema type: `boolean`, `number`, `enum`, `string`, `array` or `record`. */
  type: string;
  /** The widget: `toggle`, `select`, `number`, `text`, `list`, `record` or `secret`. */
  control: string;
  /** The engine's effective value; `null` when it has none, or when it is redacted. */
  value: unknown;
  /** Whether the engine holds a value — a different question from whether we may show it. */
  present: boolean;
  /** The engine holds a credential it will not print. */
  redacted: boolean;
  /** An enum's options, with the engine's own labels. */
  choices: { value: string; label: string }[];
  /** Where the value is written: `overlay`, `project`, `global`, `default` or `unknown`. */
  origin: string;
  /** Why the row cannot be edited, when a `ui.condition` is unmet. */
  gated: { reason: string; needs: string | null } | null;
  /** What a write costs: `live` (applied through RPC now), `sidecar`, or `app`. */
  restart: string;
  /** Why this key is dangerous, when it is. */
  danger: { level: "confirm" | "warn"; why: string } | null;
}

/** A fact the app reports and nothing here edits. */
export interface SettingsStaticRow {
  role: "static";
  id: string;
  label: string;
  description: string;
  value: string;
}

/** A control that does something rather than storing something. */
export interface SettingsActionRow {
  role: "action";
  id: string;
  label: string;
  description: string;
  /** The screen dispatches on this: `open-mode-menu`, `open-config-file`. */
  action: string;
  tone: "normal" | "danger";
}

/** What one write recorded, and what it will cost. */
export interface SettingsWriteOutcome {
  key: string;
  value: unknown;
  restart: string;
  /** The sentence to show afterwards; it never promises more than the restart allows. */
  message: string;
}

/** The escape hatch: the config file as the app will show it. */
export interface SettingsHatch {
  path: string;
  exists: boolean;
  /** The file's text **after redaction** — a credential is never sent to the frontend. */
  text: string;
  /** A syntax error's own words; when set, `text` is empty on purpose. */
  error: string | null;
  /** The keys the file sets, sorted. */
  keys: string[];
  /** Parts of the file the app will not act on. */
  refusals: SettingsRefusal[];
  /** The app's own backups of this file, newest first. */
  backups: SettingsBackup[];
}

/** One backup of the config file, with its text redacted the same way. */
export interface SettingsBackup {
  path: string;
  /** Seconds since the epoch, when the app named it. */
  at: number | null;
  text: string;
}

/** What a hand-edit would change, before anything is written. */
export interface SettingsHatchPlan {
  changes: SettingsHatchChange[];
  /** Anything the plan will not do. A plan with a refusal applies **nothing**. */
  refusals: SettingsRefusal[];
}

/** One edit a hand-edit would make. */
export interface SettingsHatchChange {
  key: string;
  action: "set" | "reset";
  before: unknown;
  after: unknown;
  restart: string;
}

/** What applying a hand-edit actually did. */
export interface SettingsApplyReport {
  changes: { key: string; action: string; ok: boolean; error: string | null }[];
  refused: SettingsRefusal[];
}

/** What a restart of the live sessions did. */
export interface SettingsRestartReport {
  restarted: string[];
  skipped: { thread: string; reason: string }[];
}

/**
 * The whole settings screen: catalog, engine values, and where each value comes from.
 *
 * `thread` is the session whose workspace the app treats as the one in view: its project
 * config file (`<workspace>/.omp/config.yml`) is the one the screen reports provenance from.
 * Omit it and the screen shows no project file at all, which is the honest answer for a
 * window with nothing open.
 */
export async function settingsScreen(thread?: string | null): Promise<SettingsScreen> {
  return invoke<SettingsScreen>("settings_screen", { thread: thread ?? null });
}

/**
 * Record a value for one settings key.
 *
 * `confirmation` is the key's own name, and it is required for the handful of keys that can
 * widen what the agent may execute or where data goes; the host refuses them without it, so a
 * dialog that forgot to ask cannot get through.
 */
export async function setSetting(
  key: string,
  value: unknown,
  confirmation?: string | null,
): Promise<SettingsWriteOutcome> {
  return invoke<SettingsWriteOutcome>("settings_write", { key, value, confirmation: confirmation ?? null });
}

/** Return a key to the engine's default. */
export async function resetSetting(
  key: string,
  confirmation?: string | null,
): Promise<SettingsWriteOutcome> {
  return invoke<SettingsWriteOutcome>("settings_reset", { key, confirmation: confirmation ?? null });
}

/**
 * Read a config file the way the escape hatch draws it.
 *
 * `scope` picks which file: `global` is `~/.omp/agent/config.yml` (the one the engine writes
 * for every project), `project` is `<thread's workspace>/.omp/config.yml`. Overlays
 * (`PI_CONFIG_FILES`) are shown in the sources banner and are never editable.
 */
export async function settingsHatch(
  thread?: string | null,
  scope?: "global" | "project",
): Promise<SettingsHatch> {
  return invoke<SettingsHatch>("settings_hatch", { thread: thread ?? null, scope: scope ?? "global" });
}

/** What an edited copy of the config file would change — validation and diff, no writing. */
export async function settingsHatchPlan(text: string): Promise<SettingsHatchPlan> {
  return invoke<SettingsHatchPlan>("settings_hatch_plan", { text });
}

/**
 * Apply an edited copy of the config file.
 *
 * `confirmation` must be `"confirm"` when the plan touches a key on the do-not-surface list;
 * the host refuses the whole plan otherwise. A plan with a refusal applies nothing at all.
 */
export async function settingsHatchApply(
  text: string,
  confirmation?: string | null,
): Promise<SettingsApplyReport> {
  return invoke<SettingsApplyReport>("settings_hatch_apply", { text, confirmation: confirmation ?? null });
}

/** Copy the config file aside, and answer with the copy's path. */
export async function settingsBackup(): Promise<string> {
  return invoke<string>("settings_backup");
}

/** Restart every live session so a written setting is in effect now. */
export async function settingsRestartSessions(): Promise<SettingsRestartReport> {
  return invoke<SettingsRestartReport>("settings_restart_sessions");
}

