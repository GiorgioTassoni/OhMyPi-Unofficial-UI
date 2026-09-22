/**
 * What a tool card shows, decided in one place (`docs/12` §3.2).
 *
 * A card is a header plus a body. The header carries what the engine already
 * said — the tool's name, its status, and the model's own one-line intent — and
 * the body carries whatever the call actually produced: a diff for `edit`, the
 * file body for `write`, the engine's result text for everything else.
 *
 * # Why this is a table and not a branch per tool
 *
 * The tool set is open — extensions and custom tools load their own — so a
 * renderer that only knows specific names has to degrade for the names it does
 * not know. Here that degradation is the default path: an unknown tool renders
 * its arguments and its result, both of which every tool has. Only three things
 * need to know a tool by name, and each is a *verified* fact rather than a guess:
 *
 * - which argument holds the one-line summary (checked against each tool's own
 *   schema, e.g. `bash` takes `command`, `glob` takes `path`),
 * - which tools produce a unified diff in `details.diff`,
 * - which tools carry a file body in their arguments.
 *
 * Everything is pure: the components below this only arrange and tint.
 */

import type { ToolSnapshot } from "../bridge";

/** What the card shows below its header. `kind` selects the styling. */
export type ToolBody =
  | { kind: "diff"; text: string }
  | { kind: "file"; text: string }
  | { kind: "output"; text: string }
  | { kind: "args"; text: string };

export interface ToolView {
  /** The one-line summary under the header, absent when nothing verified matches. */
  summary: string | null;
  body: ToolBody;
  /** `artifact://<id>` when the engine spilled truncated output, else null. */
  artifact: string | null;
}

/**
 * Where each tool keeps the argument worth showing in the header.
 *
 * Each entry was read off the tool's own schema rather than inferred from a name:
 * `bash.command`, `eval.title`/`eval.code`, `read.path`, `write.path`,
 * `grep.pattern`, `glob.path`, and `path`/`file_path` for the edit family (the
 * TUI's edit renderer accepts both). A tool that is not listed gets no summary
 * line — not a wrong one — and its arguments are still in the body.
 */
const SUMMARY_FIELDS: Record<string, readonly string[]> = {
  bash: ["command"],
  bash_interactive: ["command"],
  eval: ["title", "code"],
  read: ["path"],
  write: ["path"],
  edit: ["path", "file_path"],
  ast_edit: ["path", "file_path"],
  apply_patch: ["path", "file_path"],
  memory_edit: ["path", "file_path"],
  grep: ["pattern"],
  glob: ["path"],
};

/** Tools whose result carries a unified diff in `details.diff`. */
const DIFF_TOOLS = new Set(["edit", "ast_edit", "apply_patch", "memory_edit"]);

/** Tools whose arguments carry the whole file they wrote. */
const FILE_TOOLS = new Set(["write"]);

/** Every tool that changes a file, whichever way it reports the change. */
const MUTATING_TOOLS = new Set([...DIFF_TOOLS, ...FILE_TOOLS]);

/** A file a tool changed, as the right panel's Changed view lists it. */
export interface TouchedFile {
  path: string;
  /** `written` for a tool that replaces the file's content, `edited` for a diff tool. */
  kind: "written" | "edited";
}

/**
 * The files a mutating tool named — the right panel's Changed view (`docs/12` §8.2).
 *
 * Read from the tool's **result** first, because that is where the engine puts the path it
 * actually resolved and every file a batch touched: `write` reports `resolvedPath`, `edit`
 * reports `path` for one file and `perFileResults[].path` for many, and `ast_edit` reports
 * `fileReplacements[].path`. The arguments are the fallback, for a card rendered from an
 * event that had not produced a result yet. Read tools are deliberately absent: the view
 * answers "what did this session change".
 */
export function touchedFiles(tool: ToolSnapshot): TouchedFile[] {
  if (!MUTATING_TOOLS.has(tool.toolName)) return [];

  const kind = FILE_TOOLS.has(tool.toolName) ? "written" : "edited";
  const details = parseObject(tool.details);
  if (details !== null) {
    const paths = pathsIn(details, tool.toolName);
    if (paths.length > 0) return paths.map((path) => ({ path, kind }));
  }

  const args = parseObject(tool.args);
  if (args === null) return [];
  const path = firstString(args, SUMMARY_FIELDS[tool.toolName] ?? []);
  return path === null ? [] : [{ path, kind }];
}

/** Every path a tool's own result named, in the order it named them. */
function pathsIn(details: Record<string, unknown>, tool: string): string[] {
  if (tool === "ast_edit") {
    const replacements = array(details.fileReplacements);
    const replaced = replacements
      .map((entry) => asString(entry.path))
      .filter((path): path is string => path !== null);
    return replaced.length > 0 ? replaced : strings(details.files);
  }

  // The write tool names the path it *resolved*, which is the absolute one worth showing.
  const single = asString(details.resolvedPath) ?? asString(details.path);
  const moved = asString(details.sourcePath);
  if (single !== null) {
    // A move changed two files: the one it landed in, and the one it left.
    return moved === null || moved === single ? [single] : [single, moved];
  }

  return array(details.perFileResults)
    .map((entry) => asString(entry.path))
    .filter((path): path is string => path !== null);
}

/** The string members of a list the engine wrote, ignoring anything else in it. */
function strings(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === "string")
    : [];
}

function array(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value)
    ? value.filter(
        (entry): entry is Record<string, unknown> =>
          entry !== null && typeof entry === "object",
      )
    : [];
}

export function toolView(tool: ToolSnapshot): ToolView {
  const args = parseObject(tool.args);
  const details = parseObject(tool.details);

  return {
    summary: args === null ? null : firstString(args, SUMMARY_FIELDS[tool.toolName] ?? []),
    body: chooseBody(tool, args, details),
    artifact: details === null ? null : artifactId(details),
  };
}

function chooseBody(
  tool: ToolSnapshot,
  args: Record<string, unknown> | null,
  details: Record<string, unknown> | null,
): ToolBody {
  // `details.diff` is the engine's own unified diff, so a diff card renders what
  // the tool reported rather than a comparison this side invents. The argument
  // fallback covers a card rendered from an event that carried the diff in its
  // arguments (the TUI's edit renderer reads both).
  const diff =
    (details === null ? null : asString(details.diff)) ??
    (args === null ? null : asString(args.diff));
  if (diff !== null && DIFF_TOOLS.has(tool.toolName)) {
    return { kind: "diff", text: diff };
  }

  const content = args === null ? null : asString(args.content);
  if (content !== null && FILE_TOOLS.has(tool.toolName)) {
    return { kind: "file", text: content };
  }

  // A finished call has the engine's result text; a running one may have only its
  // arguments, and partial arguments are still worth showing — the reference
  // renderer does the same while a computed preview is unavailable.
  if (tool.output !== "") {
    return { kind: "output", text: tool.output };
  }

  return { kind: "args", text: pretty(tool.args) };
}

/** The artifact a truncated result spilled to (`docs/03` §"Result shape"). */
function artifactId(details: Record<string, unknown>): string | null {
  const meta = details.meta;
  if (meta === null || typeof meta !== "object") {
    return null;
  }

  const id = (meta as Record<string, unknown>).truncation;
  if (id === null || typeof id !== "object") {
    return null;
  }

  const value = (id as Record<string, unknown>).artifactId;
  return typeof value === "string" && value !== "" ? value : null;
}

/**
 * A tool's `details` as an object, or null.
 *
 * Exported because more than one model parses the same field: the panel reads `meta` off it,
 * the agents view reads `async`, and a second copy of this is how two readers end up
 * disagreeing about what a `details` string that is not an object means.
 */
export function parseObject(text: string): Record<string, unknown> | null {
  if (text === "") {
    return null;
  }

  try {
    const value: unknown = JSON.parse(text);
    return value !== null && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    // A streaming call's arguments are not valid JSON until they complete, and an
    // unparseable blob is still the only thing there is to show.
    return null;
  }
}

function firstString(args: Record<string, unknown>, fields: readonly string[]): string | null {
  for (const field of fields) {
    const value = asString(args[field]);
    if (value !== null && value !== "") {
      return value;
    }
  }

  return null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

/** Arguments for the body: indented when they parse, verbatim when they do not. */
function pretty(text: string): string {
  const parsed = parseObject(text);
  return parsed === null ? text : JSON.stringify(parsed, null, 2);
}
