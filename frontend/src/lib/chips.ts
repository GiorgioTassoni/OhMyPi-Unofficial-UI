import type { ContextSnapshot } from "../bridge";
import type { ModelOption } from "./models";

/**
 * What the composer's chips say (`docs/12` §5.1) and what their popovers offer.
 *
 * Everything here is presentation of values the session already holds: the label a
 * chip shows, the ladder the effort popover walks, the sentence a mode row carries,
 * the ring's geometry. It is a module rather than template expressions because these
 * are the parts that go quietly wrong — a chip that names a model the session is not
 * running, an effort level offered for a model that cannot take it, a ring that
 * claims 100% at zero — and a pure function is where a test can pin them.
 */

/**
 * Everything the chip row renders from.
 *
 * One object rather than eleven props: the composer passes it through untouched, so
 * adding a chip is a change in two files instead of three.
 */
export interface ChipRow {
  /** The session's working directory, as the sidecar was launched. */
  workspace: string;
  /** Directories this window has been pointed at — the switcher's short list. */
  recent: string[];
  /** The approval mode in force (`--approval-mode`); null when the host did not say. */
  mode: string | null;
  model: SessionModel | null;
  models: ModelOption[];
  refreshing: boolean;
  favourites: string[];
  thinkingLevel: string | null;
  /** `get_state.contextUsage`, as the bridge reports it. */
  context: ContextSnapshot | null;
  autoCompaction: boolean | null;
  compacting: boolean;
}

/**
 * The catalogue's own row type, re-used rather than declared twice: the chip and the
 * picker must agree about what a model *is*, and two structurally identical interfaces
 * drift the first time one of them gains a field.
 */
export type { ModelOption };

/** The session's model, as `get_state` reports it. */
export interface SessionModel {
  provider?: string | null;
  id?: string | null;
  name?: string | null;
}

/**
 * The model chip's text.
 *
 * The catalogue's `name` is the display name the engine chose, so it wins; a session
 * whose model is not in the catalogue (or a catalogue that has not been fetched yet)
 * falls back to the id's last segment, which is still the model the session is
 * running. Never the raw `provider/id`, which reads as a path.
 */
export function modelChipLabel(
  model: SessionModel | null,
  catalogue: readonly ModelOption[] = [],
): string {
  if (model === null || (model.provider == null && model.id == null && model.name == null)) {
    return "no model";
  }
  if (model.name != null && model.name !== "") {
    return model.name;
  }
  const known = catalogue.find(
    (option) => option.provider === model.provider && option.id === model.id,
  );
  if (known !== undefined && known.name !== "") {
    return known.name;
  }
  const id = model.id ?? "";
  const tail = id.slice(id.lastIndexOf("/") + 1);
  return tail === "" ? (model.provider ?? "no model") : tail;
}

/** The effort ladder, in the order the popover shows it (`docs/12` §7.6). */
export const EFFORT_LEVELS = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "auto",
] as const;

export type EffortLevel = (typeof EFFORT_LEVELS)[number];

/** The effort chip's text. */
export function effortLabel(level: string | null): string {
  return level === null || level === "" ? "Effort —" : `Effort ${level}`;
}

/**
 * Whether the model can take this level.
 *
 * `off` is always allowed — not thinking is not a capability — and `auto` is the
 * engine's own classifier, which every model reaches through the same path. A model
 * with no `efforts` at all (`efforts` empty, `reasoning: false`) supports neither, so
 * the popover says so instead of letting the choice fail silently (§7.6).
 */
export function effortSupported(level: string, model: ModelOption | null): boolean {
  if (level === "off" || level === "auto") {
    return true;
  }
  if (model === null) {
    return true;
  }
  if (model.reasoning && model.efforts.length === 0) {
    return true;
  }
  return model.efforts.includes(level);
}

/** The permission ladder (`docs/12` §7.2), in the order the popover shows it. */
export const APPROVAL_MODES = [
  {
    mode: "always-ask",
    label: "Ask for approval",
    detail: "Always asks before making changes",
    /** `Full access` is tinted: it removes the prompt entirely. */
    warning: false,
  },
  {
    mode: "write",
    label: "Auto-accept edits",
    detail: "Accepts edits, asks before risky commands",
    warning: false,
  },
  {
    mode: "yolo",
    label: "Full access",
    detail: "Runs edits and commands without asking",
    warning: true,
  },
] as const;

/** The mode chip's text, and the row the popover marks as current. */
export function modeRow(mode: string | null): (typeof APPROVAL_MODES)[number] | null {
  return APPROVAL_MODES.find((row) => row.mode === mode) ?? null;
}

export function modeLabel(mode: string | null): string {
  return modeRow(mode)?.label ?? "Mode —";
}

/** The context ring's text: the percentage the engine reported, or nothing yet. */
export function percentLabel(percent: number | null): string {
  if (percent === null || !Number.isFinite(percent)) {
    return "—";
  }
  const clamped = Math.min(100, Math.max(0, percent));
  return clamped < 10 ? `${clamped.toFixed(1)}%` : `${Math.round(clamped)}%`;
}

/**
 * The ring's `stroke-dasharray` for a used fraction.
 *
 * An SVG circle rather than a conic gradient so the ring is one element that scales
 * with the chip text. A zero-usage session draws no arc at all — a ring that showed a
 * stub at 0% would look like usage that is not there.
 */
export function ringDash(percent: number | null, radius: number): { dash: string; empty: boolean } {
  const circumference = 2 * Math.PI * radius;
  if (percent === null || !Number.isFinite(percent) || percent <= 0) {
    return { dash: `0 ${circumference.toFixed(2)}`, empty: true };
  }
  const used = (Math.min(100, percent) / 100) * circumference;
  return { dash: `${used.toFixed(2)} ${(circumference - used).toFixed(2)}`, empty: false };
}

/**
 * The folder chip's text.
 *
 * The last path segment, because the chip sits beside three others and a full path
 * pushes them off the row; the popover shows the whole thing. Trailing separators are
 * ignored, and a path that is only separators (`/`) answers with itself rather than
 * with an empty chip.
 */
export function folderName(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, "");
  if (trimmed === "") {
    return path === "" ? "no folder" : path;
  }
  const tail = trimmed.slice(Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\")) + 1);
  return tail === "" ? trimmed : tail;
}

/** `tokens` and window, said the way the popover's bar needs them (`docs/12` §7.5). */
export function contextSummary(
  tokens: number,
  window: number,
): { text: string; percent: number | null } {
  if (tokens <= 0 || window <= 0) {
    return { text: "No usage yet", percent: null };
  }
  const percent = Math.min(100, (tokens / window) * 100);
  return {
    text: `${tokens.toLocaleString()} / ${window.toLocaleString()} tokens`,
    percent,
  };
}
