/**
 * The settings screen's model, asserted (`docs/12` §12).
 *
 * The cases here are the ones a rendered screen hides: a query that should reach the key
 * before the prose, a row whose only match is its help text, a nav group the catalog has
 * never used before, and a restart class or origin this build has not seen — the last one
 * because a row that guessed at an unknown class would describe a write it cannot honour.
 */

import { describe, expect, test } from "bun:test";
import type { SettingsRow, SettingsScreen, SettingsSettingRow } from "../bridge";
import {
  itemValue,
  matchRank,
  navGroups,
  numberValue,
  openingSection,
  originHint,
  prioritizeSettings,
  restartHint,
  rowId,
  searchRows,
  SETTINGS_INFO_ID,
  selectorFor,
  sourceLines,
} from "./settings";

/** A settings row with the host's defaults, so a case states only what it is about. */
function row(entry: Partial<SettingsSettingRow> & { key: string }): SettingsSettingRow {
  return {
    role: "setting",
    label: entry.key,
    description: "",
    type: "string",
    control: "text",
    value: null,
    present: false,
    redacted: false,
    choices: [],
    origin: "default",
    gated: null,
    restart: "sidecar",
    danger: null,
    ...entry,
  };
}

function screen(sections: SettingsScreen["sections"], sources?: Partial<SettingsScreen["sources"]>): SettingsScreen {
  return {
    catalogVersion: "18.2.6",
    sources: {
      agentDir: "/home/x/.omp/agent",
      globalFile: { path: "/home/x/.omp/agent/config.yml", exists: true, error: null, refusals: [] },
      projectFile: { path: "/p/.omp/config.yml", exists: false, error: null, refusals: [] },
      overlays: [],
      ...sources,
    },
    drift: { unknownKeys: [], missingKeys: [] },
    sections,
  };
}

const SCREEN = screen([
  {
    id: "general",
    title: "General",
    blurb: "Version and layout.",
    navGroup: "app",
    icon: "sliders",
    groups: [
      {
        name: "About",
        rows: [
          { role: "static", id: "app.version", label: "Version", description: "This app", value: "0.1.0" },
        ],
      },
      {
        name: "Housekeeping",
        rows: [
          row({ key: "gc.coldArchiveAfterDays", label: "Cold archive after days", description: "Days before a transcript is archived." }),
        ],
      },
    ],
  },
  {
    id: "model-and-providers",
    title: "Model & Providers",
    blurb: "Which model answers.",
    navGroup: "agent",
    icon: "sparkle",
    groups: [
      {
        name: "Roles",
        rows: [
          row({ key: "modelRoles", label: "Model Roles", description: "Model per role", control: "record" }),
          row({ key: "enabledModels", label: "Enabled models", description: "Model roles and cycles", control: "list" }),
        ],
      },
    ],
  },
  {
    id: "raw-config",
    title: "Raw config",
    blurb: "The file itself.",
    navGroup: "system",
    icon: "scroll",
    groups: [],
  },
]);

describe("nav", () => {
  test("sections keep the catalog's order inside the three columns", () => {
    const groups = navGroups(SCREEN);
    expect(groups.map((group) => group.id)).toEqual(["app", "agent", "system"]);
    expect(groups.map((group) => group.label)).toEqual(["App", "Agent", "System"]);
    expect(groups[0]?.sections.map((section) => section.id)).toEqual(["general"]);
    expect(groups.find((group) => group.id === "system")?.sections.at(-1)?.id).toBe(SETTINGS_INFO_ID);
  });

  test("a column this build has never seen is kept and named, not dropped", () => {
    const groups = navGroups(
      screen([{ id: "future", title: "Future", blurb: "", navGroup: "workspace", icon: "code", groups: [] }]),
    );
    expect(groups.map((group) => group.id)).toEqual(["workspace", "system"]);
    expect(groups[0]?.label).toBe("Workspace");
    expect(groups[1]?.sections.map((section) => section.id)).toEqual([SETTINGS_INFO_ID]);
  });

  test("the screen opens on General when the catalog has one", () => {
    expect(openingSection(SCREEN)).toBe("general");
    expect(openingSection(screen([]))).toBe("");
  });
});

describe("importance order", () => {
  test("high-impact sections lead their nav group and unknown sections keep their relative order", () => {
    const ordered = prioritizeSettings(
      screen([
        { id: "appearance", title: "Appearance", blurb: "", navGroup: "app", icon: "sparkle", groups: [] },
        { id: "future-a", title: "Future A", blurb: "", navGroup: "app", icon: "code", groups: [] },
        { id: "general", title: "General", blurb: "", navGroup: "app", icon: "sliders", groups: [] },
        { id: "future-b", title: "Future B", blurb: "", navGroup: "app", icon: "code", groups: [] },
        { id: "notifications", title: "Notifications", blurb: "", navGroup: "app", icon: "bell", groups: [] },
      ]),
    );

    expect(ordered.sections.map((section) => section.id)).toEqual([
      "general",
      "notifications",
      "appearance",
      "future-a",
      "future-b",
    ]);
  });

  test("important groups and controls move first without dropping unlisted catalog rows", () => {
    const retry = row({ key: "retry.enabled", label: "Enabled" });
    const future = row({ key: "retry.future", label: "Future retry option" });
    const attempts = row({ key: "retry.maxRetries", label: "Retry attempts" });
    const ordered = prioritizeSettings(
      screen([
        {
          id: "usage-and-limits",
          title: "Usage & Limits",
          blurb: "",
          navGroup: "app",
          icon: "gauge",
          groups: [
            { name: "Service Tiers", rows: [] },
            { name: "Retry & Fallback", rows: [attempts, future, retry] },
          ],
        },
      ]),
    );

    expect(ordered.sections[0]?.groups.map((group) => group.name)).toEqual([
      "Retry & Fallback",
      "Service Tiers",
    ]);
    expect(ordered.sections[0]?.groups[0]?.rows.map(rowId)).toEqual([
      "retry.enabled",
      "retry.maxRetries",
      "retry.future",
    ]);
  });

  test("single-group pages still promote their everyday controls", () => {
    const ordered = prioritizeSettings(
      screen([
        {
          id: "lsp",
          title: "LSP",
          blurb: "",
          navGroup: "agent",
          icon: "code",
          groups: [
            {
              name: "LSP",
              rows: [
                row({ key: "lsp.lazy" }),
                row({ key: "lsp.diagnosticsDeduplicate" }),
                row({ key: "lsp.enabled" }),
                row({ key: "lsp.formatOnWrite" }),
                row({ key: "lsp.diagnosticsOnWrite" }),
              ],
            },
          ],
        },
      ]),
    );

    expect(ordered.sections[0]?.groups[0]?.rows.map(rowId)).toEqual([
      "lsp.enabled",
      "lsp.formatOnWrite",
      "lsp.diagnosticsOnWrite",
      "lsp.lazy",
      "lsp.diagnosticsDeduplicate",
    ]);
  });
});

describe("search", () => {
  test("the key is matched before the prose, and a description hit is last", () => {
    const hits = searchRows(SCREEN, "model");
    const keys = hits.flatMap((group) => group.rows.map((hit) => (hit.row.role === "setting" ? hit.row.key : hit.row.id)));
    // `modelRoles` starts with the query; `enabledModels` only contains it, and its
    // description's "Model roles" puts it ahead of a bare description match.
    expect(keys).toEqual(["modelRoles", "enabledModels"]);
  });

  test("a name that *is* the query beats one that contains it", () => {
    expect(matchRank(row({ key: "gc.wal", label: "WAL" }), "WAL")).toBe(1);
    expect(matchRank(row({ key: "gc.wal", label: "The WAL" }), "WAL")).toBe(4);
    expect(matchRank(row({ key: "gc.archive", label: "Something else", description: "a WAL-ish thing" }), "WAL")).toBe(7);
    expect(matchRank(row({ key: "gc.archive", label: "Something else", description: "a bigwal thing" }), "WAL")).toBe(8);
  });

  test("matches are grouped by the section that owns them, in the rail's order", () => {
    const hits = searchRows(SCREEN, "cold");
    expect(hits.map((group) => group.section.id)).toEqual(["general"]);
    expect(hits[0]?.rows.length).toBe(1);
  });

  test("a query that matches nothing finds nothing, and an empty one is not a search", () => {
    expect(searchRows(SCREEN, "zzz")).toEqual([]);
    expect(searchRows(SCREEN, "   ")).toEqual([]);
  });

  test("case does not matter", () => {
    expect(searchRows(SCREEN, "COLD")[0]?.rows.length).toBe(1);
  });
});

describe("what a write costs", () => {
  test("the three classes say what the host promises, and nothing more", () => {
    expect(restartHint("live").chip).toBe("live");
    expect(restartHint("live").action).toBeNull();
    expect(restartHint("live").note).toContain("running sessions now");
    expect(restartHint("sidecar").note).toContain("each session restarts");
    expect(restartHint("sidecar").action).toBe("restart");
    expect(restartHint("app").note).toContain("relaunch");
    expect(restartHint("app").action).toBe("relaunch");
  });

  test("a class this build has not seen is named rather than rounded to a familiar one", () => {
    expect(restartHint("hot-reload").chip).toBe("hot-reload");
    expect(restartHint("hot-reload").note).toContain("hot-reload");
  });

  test("an origin says where the value is written, and an unknown one says that", () => {
    expect(originHint("project").chip).toBe("project");
    expect(originHint("overlay").note).toContain("read-only");
    expect(originHint("somewhere").chip).toBe("somewhere");
  });
});

describe("sources", () => {
  test("a file that cannot be parsed is flagged, and an overlay is always read-only", () => {
    const lines = sourceLines(
      screen([], {
        globalFile: { path: "/c/config.yml", exists: true, error: "line 3: bad indent", refusals: [] },
        overlays: [{ path: "/tmp/overlay.yml", exists: true, error: null, refusals: [] }],
      }),
    );
    expect(lines[0]?.bad).toBe(true);
    expect(lines[0]?.note).toContain("bad indent");
    expect(lines[2]?.note).toContain("read-only");
  });

  test("no thread means no project file, said rather than invented", () => {
    const lines = sourceLines(screen([], { projectFile: { path: "", exists: false, error: null, refusals: [] } }));
    expect(lines[1]?.path).toBe("no project file");
    expect(lines[1]?.bad).toBe(false);
  });
});

describe("values a sub-editor writes", () => {
  test("a string item stays a string, and a literal one must parse", () => {
    expect(itemValue("42", false)).toEqual({ ok: true, value: "42" });
    expect(itemValue("42", true)).toEqual({ ok: true, value: 42 });
    expect(itemValue("not json", true).ok).toBe(false);
  });

  test("a number field refuses what is not a number rather than writing NaN", () => {
    expect(numberValue(" 12 ")).toEqual({ ok: true, value: 12 });
    expect(numberValue("abc").ok).toBe(false);
    expect(numberValue("").ok).toBe(false);
  });

  test("picking a model keeps the effort suffix the selector already carried", () => {
    expect(selectorFor("anthropic", "claude-opus-4-5", "openai/gpt-4.1-mini:high")).toBe(
      "anthropic/claude-opus-4-5:high",
    );
    expect(selectorFor("anthropic", "claude-opus-4-5", "")).toBe("anthropic/claude-opus-4-5");
  });

  test("a row that is not a setting has no key to search, and is still findable", () => {
    const action: SettingsRow = {
      role: "action",
      id: "app.config-file",
      label: "Config file",
      description: "/home/x/.omp/agent/config.yml",
      action: "open-config-file",
      tone: "normal",
    };
    expect(matchRank(action, "config")).toBe(3);
  });
});
