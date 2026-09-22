/**
 * The settings screen's model (`docs/12` §12): the nav's shape, the rail's search, and the
 * vocabulary a row speaks.
 *
 * Everything here is a pure function of the host's `SettingsScreen`, which is why it is a
 * module rather than fifty lines inside the screen component: the host composes the catalog,
 * so the only decisions left to the frontend are how to *arrange* what it was handed — and
 * arrangement is exactly the part worth asserting without a browser.
 *
 * Two rules run through the file:
 *
 * 1. **Nothing infers meaning from a key name.** Sections, groups, labels and options arrive
 *    from the host. A short explicit preference list reorders known high-impact entries, while
 *    everything absent from it retains the host's order.
 * 2. **An unknown value is named, not guessed.** A restart class or origin this build has
 *    never seen shows itself verbatim instead of falling into the nearest familiar bucket,
 *    because the one thing a settings row must not do is describe a write incorrectly.
 */

import type {
  SettingsGroup,
  SettingsRow,
  SettingsScreen,
  SettingsSection,
  SettingsSettingRow,
} from "../bridge";
import { ICONS, type IconName } from "./icons";

/**
 * The three nav columns the catalog sorts its sections into (`navGroup`).
 *
 * The catalog's own names, given a label. They are app-owned UI policy rather than schema
 * metadata, so they are the one piece of the nav the frontend does write — and a fourth group
 * from a newer catalog still gets a column (`navGroups` appends it) rather than being dropped.
 */
const NAV_GROUPS: readonly { id: string; label: string }[] = [
  { id: "app", label: "App" },
  { id: "agent", label: "Agent" },
  { id: "system", label: "System" },
];

export interface NavGroup {
  id: string;
  label: string;
  sections: NavSection[];
}

/** A rail destination. Most come from the catalog; Info is app-owned screen context. */
export type NavSection = Pick<SettingsSection, "id" | "title" | "icon">;

/** The one settings destination that describes the catalog rather than editing a setting. */
export const SETTINGS_INFO_ID = "settings-info";

const INFO_SECTION: NavSection = {
  id: SETTINGS_INFO_ID,
  title: "Info",
  icon: "info",
};

/**
 * Product order: frequent, high-impact decisions first; specialist tuning later.
 *
 * These lists are deliberately partial. Anything OMP adds keeps its catalog position after
 * the named items, so ordering the UI never becomes a second copy of the settings catalog.
 */
const SECTION_IMPORTANCE = [
  "general",
  "notifications",
  "appearance",
  "shortcuts-and-keys",
  "usage-and-limits",
  "model-and-providers",
  "agent",
  "context-and-compaction",
  "terminal-and-shell",
  "session",
  "skills",
  "mcp",
  "lsp",
  "memory",
  "advanced",
  "raw-config",
] as const;

const GROUP_IMPORTANCE: Readonly<Record<string, readonly string[]>> = {
  general: ["About", "Power", "Git", "Startup & Updates", "Housekeeping"],
  appearance: ["Theme", "Display", "Images", "Composer", "Status Line"],
  "shortcuts-and-keys": ["Input", "Magic Keywords"],
  "model-and-providers": [
    "Roles & Selection",
    "Thinking",
    "Sampling",
    "Vision",
    "Advisor",
    "Tiny Model",
    "Services",
    "Protocol",
    "Prewalk",
  ],
  agent: [
    "Available Tools",
    "Prompt",
    "Agent Behaviour",
    "Tool Exposure",
    "Editing",
    "Reading",
    "Read Summaries",
    "Subagents",
    "Isolation",
    "Execution",
    "Todos",
    "Output Limits",
    "Grep & Browser",
    "GitHub",
    "Discovery (xdev)",
    "Extensions",
  ],
  "context-and-compaction": ["Compaction", "General", "Rules (TTSR)", "Experimental"],
  memory: ["General", "Auto-Learn", "Mnemopi", "Hindsight", "Legacy Local Memory", "Sharpshooter"],
  skills: ["Skill Discovery", "Commands & Skills"],
  mcp: ["Discovery & MCP"],
  lsp: ["LSP"],
  session: ["Session Behaviour", "Modes"],
  "terminal-and-shell": ["Bash", "Eval & Runtimes", "Shell Minimizer"],
  notifications: ["Notifications", "Speech"],
  "usage-and-limits": ["Retry & Fallback", "Accounts & Concurrency", "Service Tiers", "Timeouts"],
};

const ROW_IMPORTANCE: Readonly<Record<string, readonly string[]>> = {
  "general/About": ["app.approval-mode", "app.config-file", "app.version", "app.config-dir"],
  "appearance/Theme": ["colorBlindMode"],
  "appearance/Display": [
    "display.smoothStreaming",
    "display.hideToolActivity",
    "display.showTokenUsage",
    "display.showTurnTime",
    "display.collapseCompacted",
    "display.shimmer",
    "tui.renderMermaid",
    "tui.reactions",
    "display.pinnedAgents",
    "task.showResolvedModelBadge",
    "display.cacheMissMarker",
  ],
  "appearance/Images": ["images.autoResize", "images.blockImages"],
  "appearance/Composer": ["composer.tokenRate"],
  "shortcuts-and-keys/Input": ["steeringMode", "followUpMode", "interruptMode"],
  "shortcuts-and-keys/Magic Keywords": [
    "magicKeywords.enabled",
    "magicKeywords.ultrathink",
    "magicKeywords.orchestrate",
    "magicKeywords.workflow",
  ],
  "model-and-providers/Roles & Selection": [
    "modelRoles",
    "enabledModels",
    "disabledProviders",
    "cycleOrder",
    "modelProviderOrder",
    "modelTags",
  ],
  "model-and-providers/Thinking": [
    "defaultThinkingLevel",
    "hideThinkingBlock",
    "proseOnlyThinking",
    "omitThinking",
    "providers.autoThinkingModel",
    "providers.autoThinkingMaxEffort",
    "externalThinking",
    "model.loopGuard.enabled",
    "model.loopGuard.checkAssistantContent",
    "model.loopGuard.toolCallReminder",
    "model.toolCallLoopGuard.enabled",
    "model.toolCallLoopGuard.threshold",
    "model.toolCallLoopGuard.exemptTools",
    "thinkingBudgets.minimal",
    "thinkingBudgets.low",
    "thinkingBudgets.medium",
    "thinkingBudgets.high",
    "thinkingBudgets.xhigh",
    "thinkingBudgets.max",
  ],
  "model-and-providers/Sampling": [
    "temperature",
    "textVerbosity",
    "topP",
    "presencePenalty",
    "repetitionPenalty",
    "topK",
    "minP",
  ],
  "model-and-providers/Advisor": [
    "advisor.enabled",
    "advisor.immuneTurns",
    "advisor.syncBacklog",
    "advisor.maxNotesPerUpdate",
  ],
  "model-and-providers/Tiny Model": [
    "providers.tinyModel",
    "providers.judgmentProvider",
    "providers.unexpectedStopModel",
    "providers.tinyModelDevice",
    "providers.tinyModelDtype",
  ],
  "model-and-providers/Services": [
    "providers.webSearchOrder",
    "providers.fetch",
    "providers.imageOrder",
    "exa.enabled",
    "providers.webSearchTimeoutSeconds",
    "providers.webSearchExclude",
    "providers.webSearchGeminiModel",
    "providers.antigravityEndpoint",
    "searxng.endpoint",
    "searxng.token",
    "searxng.basicUsername",
    "searxng.basicPassword",
    "searxng.categories",
    "searxng.engines",
    "searxng.language",
    "searxng.safesearch",
    "exa.searchDelayMs",
  ],
  "model-and-providers/Protocol": [
    "providers.openaiWebsockets",
    "providers.cacheRetention",
    "providers.openrouterVariant",
    "providers.kimiApiFormat",
    "provider.appendOnlyContext",
  ],
  "agent/Available Tools": [
    "todo.enabled",
    "ask.enabled",
    "grep.enabled",
    "glob.enabled",
    "fetch.enabled",
    "web_search.enabled",
    "generate_image.enabled",
    "github.enabled",
    "astGrep.enabled",
    "astEdit.enabled",
    "launch.enabled",
    "checkpoint.enabled",
    "security.enabled",
    "vault.enabled",
  ],
  "agent/Prompt": [
    "personality",
    "includeWorkspaceTree",
    "skillful",
    "includeModelInPrompt",
    "inlineToolDescriptors",
    "modelRoleStorage",
  ],
  "agent/Editing": [
    "edit.mode",
    "edit.fuzzyMatch",
    "edit.fuzzyThreshold",
    "edit.blockAutoGenerated",
    "edit.enforceSeenLines",
    "edit.streamingAbort",
    "edit.recoverInlineEdits",
    "edit.autoRepair.enabled",
    "edit.blackbox.enabled",
  ],
  "agent/Reading": [
    "read.renderMarkdown",
    "read.toolResultPreview",
    "readLineNumbers",
    "read.defaultLimit",
  ],
  "agent/Read Summaries": [
    "read.summarize.enabled",
    "read.summarize.prose",
    "read.summarize.minTotalLines",
    "read.summarize.minBodyLines",
    "read.summarize.minCommentLines",
    "read.summarize.unfoldUntil",
    "read.summarize.unfoldLimit",
  ],
  "agent/Execution": [
    "async.enabled",
    "tools.maxTimeout",
    "tools.abortOnFabricatedResult",
    "images.questionTimeoutMs",
    "tools.intentTracing",
    "tools.speculativeExecution.enabled",
    "tools.speculativeExecution.maxInFlight",
    "irc.timeoutMs",
  ],
  "agent/Subagents": [
    "task.eager",
    "task.maxConcurrency",
    "task.batch",
    "task.enableEffort",
    "task.maxEffort",
    "task.enableLsp",
    "task.maxRecursionDepth",
    "task.maxRuntimeMs",
    "task.agentIdleTtlMs",
    "task.softRequestBudget",
    "task.softRequestBudgetNotice",
    "task.disabledAgents",
    "task.agentModelOverrides",
    "task.agentServiceTierOverrides",
    "task.prewalk",
    "task.agentPrewalk",
    "task.agentAdvisor",
  ],
  "agent/Isolation": [
    "task.isolation.enabled",
    "isolation.backend",
    "task.isolation.apply",
    "task.isolation.merge",
    "task.isolation.commits",
    "worktree.clone",
    "worktree.cleanSource",
    "worktree.base",
  ],
  "agent/Todos": [
    "todo.eager",
    "todo.reminders",
    "todo.remindersMax",
    "tasks.todoClearDelay",
  ],
  "agent/Output Limits": [
    "tools.outputMaxColumns",
    "tools.artifactSpillThreshold",
    "tools.artifactHeadBytes",
    "tools.artifactTailBytes",
    "tools.artifactTailLines",
  ],
  "agent/Grep & Browser": ["grep.contextBefore", "grep.contextAfter"],
  "agent/GitHub": ["github.cache.enabled", "github.cache.softTtlSec", "github.cache.hardTtlSec"],
  "agent/Discovery (xdev)": ["tools.xdev", "tools.xdevDocs", "tools.xdevInlineDevices"],
  "context-and-compaction/Compaction": [
    "compaction.enabled",
    "compaction.thresholdPercent",
    "compaction.thresholdTokens",
    "compaction.autoContinue",
    "compaction.midTurnEnabled",
    "compaction.asyncEnabled",
    "compaction.idleEnabled",
    "compaction.idleThresholdTokens",
    "compaction.idleTimeoutSeconds",
    "compaction.supersedeReads",
    "compaction.dropUseless",
    "compaction.methodOrder",
    "compaction.handoffSaveToDisk",
    "compaction.remoteStreamingV2Enabled",
    "compaction.experimentalContextManagement",
  ],
  "context-and-compaction/General": ["extendedContext", "contextPromotion.enabled"],
  "context-and-compaction/Rules (TTSR)": [
    "ttsr.enabled",
    "ttsr.contextMode",
    "ttsr.interruptMode",
    "ttsr.repeatMode",
    "ttsr.repeatGap",
    "ttsr.builtinRules",
  ],
  "memory/General": ["memory.backend", "providers.memoryModel"],
  "memory/Auto-Learn": ["autolearn.enabled", "autolearn.autoContinue"],
  "memory/Mnemopi": [
    "mnemopi.bank",
    "mnemopi.scoping",
    "mnemopi.autoRecall",
    "mnemopi.autoRetain",
    "mnemopi.enhancedRecall",
    "mnemopi.polyphonicRecall",
    "mnemopi.proactiveLinking",
    "mnemopi.dbPath",
    "mnemopi.noEmbeddings",
    "mnemopi.embeddingModel",
    "mnemopi.embeddingVariant",
    "mnemopi.embeddingApiKey",
    "mnemopi.llmMode",
    "mnemopi.llmModel",
    "mnemopi.llmApiKey",
  ],
  "memory/Hindsight": [
    "hindsight.apiToken",
    "hindsight.bankId",
    "hindsight.scoping",
    "hindsight.autoRecall",
    "hindsight.autoRetain",
    "hindsight.retainMode",
    "hindsight.mentalModelsEnabled",
    "hindsight.mentalModelAutoSeed",
  ],
  "skills/Skill Discovery": [
    "skills.enabled",
    "skills.enablePiProject",
    "skills.enablePiUser",
    "skills.enableAgentsProject",
    "skills.enableAgentsUser",
    "skills.enableCodexUser",
    "skills.enableClaudeProject",
    "skills.enableClaudeUser",
    "skills.includeSkills",
    "skills.ignoredSkills",
  ],
  "skills/Commands & Skills": [
    "skills.enableSkillCommands",
    "commands.enableClaudeProject",
    "commands.enableClaudeUser",
    "commands.enableOpencodeProject",
    "commands.enableOpencodeUser",
  ],
  "mcp/Discovery & MCP": [
    "mcp.enableProjectConfig",
    "mcp.renderMarkdownResults",
    "mcp.notifications",
    "mcp.notificationDebounceMs",
  ],
  "lsp/LSP": [
    "lsp.enabled",
    "lsp.formatOnWrite",
    "lsp.diagnosticsOnWrite",
    "lsp.diagnosticsOnEdit",
    "lsp.lazy",
    "lsp.shared",
    "lsp.diagnosticsDeduplicate",
  ],
  "terminal-and-shell/Bash": [
    "bash.enabled",
    "bash.direnv",
    "bash.autoBackground.enabled",
    "bashInterceptor.enabled",
    "bash.direnvLoadTimeoutMs",
  ],
  "terminal-and-shell/Eval & Runtimes": [
    "eval.py",
    "eval.js",
    "python.kernelMode",
    "eval.tools.enabled",
    "eval.autoBackground.enabled",
    "eval.workpool.freshAgents",
  ],
  "terminal-and-shell/Shell Minimizer": [
    "shellMinimizer.enabled",
    "shellMinimizer.only",
    "shellMinimizer.except",
    "shellMinimizer.sourceOutlineLevel",
    "shellMinimizer.maxCaptureBytes",
    "shellMinimizer.settingsPath",
    "shellMinimizer.legacyFilters",
  ],
  "notifications/Notifications": [
    "completion.notify",
    "error.notify",
    "ask.notify",
    "ask.timeout",
    "recap.enabled",
    "recap.idleSeconds",
  ],
  "session/Session Behaviour": [
    "title.refreshOnReplan",
    "workspace.additionalDirectories",
    "branchSummary.enabled",
  ],
  "usage-and-limits/Retry & Fallback": [
    "retry.enabled",
    "retry.maxRetries",
    "retry.maxDelayMs",
    "retry.waitForUsageReset",
    "retry.modelFallback",
    "retry.usageAwareFallback",
    "retry.usageReservePct",
    "retry.usageReservePolicy",
    "retry.fallbackChains",
    "retry.fallbackRevertPolicy",
    "providers.anthropic.serverSideFallback",
  ],
  "usage-and-limits/Accounts & Concurrency": [
    "providers.maxInFlightRequests",
    "codexResets.autoRedeem",
    "codexResets.minBlockedMinutes",
    "codexResets.keepCredits",
    "codexResets.salvageHorizonHours",
    "providers.ollama-cloud.maxConcurrency",
  ],
  "usage-and-limits/Service Tiers": [
    "tier.openai",
    "tier.anthropic",
    "tier.google",
    "tier.subagent",
    "tier.advisor",
    "providers.fireworksTier",
  ],
  "usage-and-limits/Timeouts": [
    "providers.streamFirstEventTimeoutSeconds",
    "providers.streamIdleTimeoutSeconds",
  ],
};

/** Stable preference sort: named items first, everything else keeps the host's order. */
function preferred<T>(items: readonly T[], order: readonly string[], key: (item: T) => string): T[] {
  const rank = new Map(order.map((name, index) => [name, index]));
  return items
    .map((item, index) => ({ item, index, rank: rank.get(key(item)) ?? order.length }))
    .sort((left, right) => left.rank - right.rank || left.index - right.index)
    .map(({ item }) => item);
}

/** Apply the app's importance policy without mutating the host-owned catalog response. */
export function prioritizeSettings(screen: SettingsScreen): SettingsScreen {
  const sections = preferred(screen.sections, SECTION_IMPORTANCE, (section) => section.id).map(
    (section): SettingsSection => ({
      ...section,
      groups: preferred(
        section.groups,
        GROUP_IMPORTANCE[section.id] ?? [],
        (group: SettingsGroup) => group.name,
      ).map((group) => ({
        ...group,
        rows: preferred(
          group.rows,
          ROW_IMPORTANCE[`${section.id}/${group.name}`] ?? [],
          rowId,
        ),
      })),
    }),
  );

  return { ...screen, sections };
}

/** The rail's columns, in the catalog's section order and with the known groups first. */
export function navGroups(screen: SettingsScreen): NavGroup[] {
  const byGroup = new Map<string, SettingsSection[]>();
  for (const section of screen.sections) {
    const list = byGroup.get(section.navGroup);
    if (list === undefined) byGroup.set(section.navGroup, [section]);
    else list.push(section);
  }

  const groups: NavGroup[] = [];
  for (const known of NAV_GROUPS) {
    const sections = byGroup.get(known.id);
    if (sections === undefined) continue;
    groups.push({ id: known.id, label: known.label, sections });
    byGroup.delete(known.id);
  }
  // A group this build has never heard of keeps its own column, named after itself.
  for (const [id, sections] of byGroup) groups.push({ id, label: titleCase(id), sections });

  // Sources and catalog drift describe the settings system as a whole. Give them one stable
  // home instead of repeating the same diagnostic card above every editable section.
  if (!groups.some((group) => group.sections.some((section) => section.id === SETTINGS_INFO_ID))) {
    const system = groups.find((group) => group.id === "system");
    if (system === undefined) groups.push({ id: "system", label: "System", sections: [INFO_SECTION] });
    else system.sections.push(INFO_SECTION);
  }

  return groups;
}

/** The section the screen opens on: General when the catalog has it, else the first one. */
export function openingSection(screen: SettingsScreen): string {
  const general = screen.sections.find((section) => section.id === "general");
  return general?.id ?? screen.sections[0]?.id ?? "";
}

/**
 * The glyph a section draws, from the name the catalog carries.
 *
 * The generator names a glyph per section out of `lib/icons.ts`, so every name the shipped
 * catalog can send exists — but the two halves are versioned separately, so a name that is not
 * a glyph this build draws falls back to the settings glyph rather than to a blank square.
 */
export function sectionIcon(name: string): IconName {
  return Object.hasOwn(ICONS, name) ? (name as IconName) : "sliders";
}

/**
 * The identity a row is addressed by: a setting row by its key, the other two by their id.
 *
 * One function rather than a `row.role === "setting" ? ...` written in five places, because
 * the search's highlight, the "needs" link and the row's `data-*` hook all have to agree on
 * what "this row" means.
 */
export function rowId(row: SettingsRow): string {
  return row.role === "setting" ? row.key : row.id;
}

/** Every row of a section, in draw order. */
export function sectionRows(section: SettingsSection): SettingsRow[] {
  return section.groups.flatMap((group) => group.rows);
}

/** Where a key lives, for a gated row's "needs" link. */
export function locate(
  screen: SettingsScreen,
  key: string,
): { section: SettingsSection; row: SettingsSettingRow } | null {
  for (const section of screen.sections) {
    for (const row of sectionRows(section)) {
      if (row.role === "setting" && row.key === key) return { section, row };
    }
  }
  return null;
}

export interface SearchGroup {
  section: SettingsSection;
  rows: { row: SettingsRow; rank: number }[];
}

/**
 * How well one row answers a query, lower being better; `null` is "does not match".
 *
 * The order is the whole point of the field's placement in the rail, so it is spelled out
 * rather than left to a substring count: the identifier a user types is matched before the
 * prose, a name that *is* the query beats one that contains it, and a description hit is a
 * last resort — a row whose only claim to match is a sentence of help text should never
 * outrank the row whose title the user typed. Everything is case-insensitive.
 *
 * | rank | match |
 * | --- | --- |
 * | 0 | the key is the query |
 * | 1 | the label is the query |
 * | 2 | the key starts with it |
 * | 3 | the label starts with it |
 * | 4 | a word of the label starts with it |
 * | 5 | the key contains it |
 * | 6 | the label contains it |
 * | 7 | a word of the description starts with it |
 * | 8 | the description contains it |
 */
export function matchRank(row: SettingsRow, query: string): number | null {
  const needle = query.trim().toLowerCase();
  if (needle === "") return null;

  const key = row.role === "setting" ? row.key.toLowerCase() : "";
  const label = row.label.toLowerCase();
  const description = row.description.toLowerCase();

  // A word inside the label or the description ("compaction" matching "Auto compaction")
  // counts, which is why these split rather than only looking at the string's start.
  if (key !== "" && key === needle) return 0;
  if (label === needle) return 1;
  if (key.startsWith(needle)) return 2;
  if (label.startsWith(needle)) return 3;
  if (label.split(/[^a-z0-9.]+/).some((word) => word !== "" && word.startsWith(needle))) return 4;
  if (key.includes(needle)) return 5;
  if (label.includes(needle)) return 6;
  if (description.split(/[^a-z0-9.]+/).some((word) => word !== "" && word.startsWith(needle))) return 7;
  if (description.includes(needle)) return 8;
  return null;
}

/**
 * The hits, grouped by the section that owns them.
 *
 * Grouped rather than flat because the result is read as "where is this setting": a list of
 * bare row titles would make the user open sections one at a time to find out. Sections stay
 * in catalog order and rows are ranked inside each, so the same query always reads the same
 * way.
 */
export function searchRows(screen: SettingsScreen, query: string): SearchGroup[] {
  const needle = query.trim();
  if (needle === "") return [];

  const groups: SearchGroup[] = [];
  for (const section of screen.sections) {
    const hits = sectionRows(section)
      .map((row, at) => ({ row, rank: matchRank(row, needle), at }))
      .filter((hit): hit is { row: SettingsRow; rank: number; at: number } => hit.rank !== null)
      .sort((left, right) => left.rank - right.rank || left.at - right.at)
      .map((hit) => ({ row: hit.row, rank: hit.rank }));
    if (hits.length > 0) groups.push({ section, rows: hits });
  }
  return groups;
}

/** How many rows a search found, for the rail's own count. */
export function hitCount(groups: SearchGroup[]): number {
  return groups.reduce((count, group) => count + group.rows.length, 0);
}

export interface RestartHint {
  /** The chip's two or three words. */
  chip: string;
  /** What it costs, in one short line the row can afford on every row it has. */
  note: string;
  /**
   * The same promise with the engine's own reasoning, for the chip's tooltip.
   *
   * The row is one of three hundred, so the line is short; nothing is lost by it, because the
   * sentence the *host* writes after a write is shown verbatim on the row that was written.
   */
  detail: string;
  /** What the user can do about it, or `null` when the write is already in effect. */
  action: "restart" | "relaunch" | null;
}

/**
 * What a write to a row costs, in the row's own words.
 *
 * The three classes are the host's (`crates/omp-settings/src/apply.rs::recorded_message`), and
 * the lines here promise exactly what they do: `live` is applied to the running session through
 * RPC, `sidecar` lands when each session is built, `app` only at launch. An unrecognised class
 * says so instead of borrowing one of the three.
 */
export function restartHint(restart: string): RestartHint {
  switch (restart) {
    case "live":
      return {
        chip: "live",
        note: "Applies to running sessions now.",
        detail: "Applied to the running session through RPC; new sessions start with it too.",
        action: null,
      };
    case "sidecar":
      return {
        chip: "on restart",
        note: "Applies when each session restarts.",
        detail:
          "Each session reads its settings when it starts, so this is in effect after that session restarts.",
        action: "restart",
      };
    case "app":
      return {
        chip: "after relaunch",
        note: "Takes effect after a relaunch of the app.",
        detail:
          "Baked into app state the app builds once at launch, so only a relaunch picks it up — and this window has no relaunch command.",
        action: "relaunch",
      };
    default:
      return {
        chip: restart === "" ? "unknown" : restart,
        note: `Nothing here knows what \`${restart}\` costs — restart the sessions after writing it.`,
        detail: `The host reported a restart class this build does not know ("${restart}"), so the honest action is to restart every session after writing.`,
        action: "restart",
      };
  }
}

/** Where a row's value is written, and what that means for editing it. */
export function originHint(origin: string): { chip: string; note: string } {
  switch (origin) {
    case "overlay":
      return { chip: "overlay", note: "Set by a read-only overlay file; a write here is masked." };
    case "project":
      return { chip: "project", note: "Written in this project's config file." };
    case "global":
      return { chip: "global", note: "Written in the global config file." };
    case "default":
      return { chip: "default", note: "Nothing writes this; it is the engine's default." };
    default:
      return { chip: origin === "" ? "unknown" : origin, note: "The engine did not say where this comes from." };
  }
}

/** `sourcesBanner`'s input, kept as one shape so the component renders and decides nothing. */
export interface SourceLine {
  path: string;
  /** One word for what this file is. */
  role: string;
  /** What to say about it beyond its path. */
  note: string | null;
  /** A file that exists and cannot be parsed, or carries parts the app will not act on. */
  bad: boolean;
}

/** The files the app reads settings from, as lines a banner can draw. */
export function sourceLines(screen: SettingsScreen): SourceLine[] {
  const sources = screen.sources;
  const lines: SourceLine[] = [
    describeFile(sources.globalFile, "global"),
    // No thread means no workspace, and the host reports an empty path rather than inventing
    // one; the banner says that instead of drawing a file that is not there.
    sources.projectFile.path === ""
      ? {
          path: "no project file",
          role: "project",
          note: "No session is open, so there is no workspace to read one from.",
          bad: false,
        }
      : describeFile(sources.projectFile, "project"),
  ];

  for (const overlay of sources.overlays) {
    lines.push({
      ...describeFile(overlay, "overlay"),
      note: overlayNote(overlay),
    });
  }
  return lines;
}

/** One file, with the two ways it can be wrong named in a few words. */
function describeFile(file: SettingsScreen["sources"]["globalFile"], role: string): SourceLine {
  const refusals =
    file.refusals.length === 0
      ? null
      : `${file.refusals.length} ${file.refusals.length === 1 ? "key" : "keys"} the app will not act on`;
  return {
    path: file.path === "" ? "(no path)" : file.path,
    role,
    note:
      file.error !== null
        ? `cannot be parsed: ${file.error}`
        : refusals ?? (file.exists ? null : "not created yet"),
    bad: file.error !== null || file.refusals.length > 0,
  };
}

function overlayNote(file: SettingsScreen["sources"]["globalFile"]): string {
  const described = describeFile(file, "overlay");
  const readOnly = "read-only — this app never writes an overlay";
  return described.note === null ? readOnly : `${described.note} · ${readOnly}`;
}

/** A word from a slug: `raw-config` → `Raw Config`. For a nav group the host did not label. */
function titleCase(slug: string): string {
  return slug
    .split(/[-_ ]+/)
    .filter((word) => word !== "")
    .map((word) => word.slice(0, 1).toUpperCase() + word.slice(1))
    .join(" ");
}

export interface ListItem {
  /** Position in the array, which is what an edit or a removal addresses. */
  index: number;
  /** The item as text: what the field shows. */
  text: string;
  /** Items that are not strings are shown as JSON and may only be replaced by valid JSON. */
  literal: boolean;
}

/**
 * A `list` row's items.
 *
 * A plain array of strings is the common case, but the schema's arrays also carry numbers and
 * objects, and an editor that silently turned one into the string `"[object Object]"` — or
 * dropped it — would rewrite a config the user never touched. So a non-string item travels as
 * its JSON and is parsed back on edit (`itemValue`).
 */
export function listItems(value: unknown): ListItem[] {
  if (!Array.isArray(value)) return [];
  return value.map((item, index) => ({
    index,
    text: textOf(item),
    literal: typeof item !== "string",
  }));
}

/** A `record` row's entries, in the order the engine listed them. */
export function recordEntries(value: unknown): { key: string; value: unknown }[] {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return [];
  return Object.entries(value as Record<string, unknown>).map(([key, entry]) => ({ key, value: entry }));
}

/** A value as a field shows it: a string as itself, anything else as its JSON. */
export function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === undefined) return "";
  return JSON.stringify(value) ?? "";
}

/**
 * A field's text read back as the value it replaces.
 *
 * `literal` decides how hard to try: an item that arrived as a string stays a string — so a
 * user typing `42` into a list of paths gets the path `42`, not a number — while an item that
 * arrived as anything else must parse as JSON, and a refusal is reported rather than written.
 */
export function itemValue(text: string, literal: boolean): { ok: true; value: unknown } | { ok: false; why: string } {
  if (!literal) return { ok: true, value: text };
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch {
    return { ok: false, why: "This item is not text — write it as JSON, or change it in the raw config." };
  }
}

/** A number field's text, or why it is not one. */
export function numberValue(text: string): { ok: true; value: number } | { ok: false; why: string } {
  const trimmed = text.trim();
  if (trimmed === "") return { ok: false, why: "A number is required — use Reset to clear the key." };
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return { ok: false, why: `"${trimmed}" is not a number.` };
  return { ok: true, value: parsed };
}

/**
 * The thinking suffix a model selector carries (`anthropic/claude-opus-4-5:high`), written
 * the way the selector writes it — `:high` — or `""` when there is none.
 *
 * Kept when the picker changes the model: the suffix is the user's choice about effort, and
 * it is not what they came to the picker to change.
 */
export function selectorSuffix(selector: string): string {
  const colon = selector.lastIndexOf(":");
  if (colon <= 0) return "";
  const suffix = selector.slice(colon + 1);
  return /^[a-z]+$/.test(suffix) ? `:${suffix}` : "";
}

/** A picked model as the selector `modelRoles` stores (`provider/modelId`, suffix and all). */
export function selectorFor(provider: string, modelId: string, was: string): string {
  return `${provider}/${modelId}${selectorSuffix(was)}`;
}

/**
 * The model a selector names, without its effort suffix — the key the model picker compares
 * its rows against (`provider/id`), or `null` when the selector is empty.
 */
export function selectorModel(selector: string): string | null {
  const base = selector.replace(/:[a-z]+$/, "");
  return base === "" ? null : base;
}

/** The sentence a locked row shows in place of its control. */
export function gatedLine(reason: string, needs: string | null): string {
  return needs === null ? reason : `${reason} (needs \`${needs}\`)`;
}
