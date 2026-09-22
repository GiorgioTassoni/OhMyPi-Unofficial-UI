/**
 * Reading the approval prompt the engine formats, and what the dialog offers for it
 * (`docs/12` §10).
 *
 * An approval is not structured on the wire. The engine sends one `select` whose
 * `title` is a prompt **it** laid out, and whose `options` are the labels to send
 * back. Measured against `omp` v18.2.6, a `bash` approval arrives as:
 *
 *     Allow tool: bash
 *     Command: touch /tmp/x
 *
 * and a `write` approval as:
 *
 *     Allow tool: write
 *     Path: /tmp/x
 *     Content:
 *     hello probe
 *
 * So this is a *presentation* of the engine's own layout, not a re-derivation of
 * the tool arguments: `formatApprovalDetails` (`src/tools/*`) already decided what
 * the user needs in order to decide, and inventing our own reading of the call
 * would mean showing something other than what is being approved. Lines we cannot
 * label are kept verbatim — a prompt we fail to understand is still a prompt the
 * user has to read.
 *
 * A label with an empty value (`Content:`, `Code:`) owns every line after it.
 * That is true of every tool measured — `bash`, `write`, `ast-edit`, `eval`,
 * `debug` — and it is the safe direction to be wrong in: the body being approved
 * is never truncated to protect a label that might follow it.
 */

/** One `Label: value` line the engine emitted. */
export interface ApprovalField {
  label: string;
  value: string;
}

/** A `Label:` line that owns the lines beneath it. */
export interface ApprovalBlock {
  label: string;
  body: string;
}

export interface ApprovalPrompt {
  /** The tool the run is asking about — the first line's value. */
  tool: string;
  fields: ApprovalField[];
  blocks: ApprovalBlock[];
  /** Lines that carry no label we know. Kept, in order, never dropped. */
  notes: string[];
}

/** The engine's own option labels for an approval (`wrapper.ts` mints both). */
export const APPROVE = "Approve";
export const DENY = "Deny";

const TOOL_LABEL = "Allow tool";

/** Labels whose value is on the same line. */
const FIELD_LABELS = new Set([
  TOOL_LABEL,
  "Origin",
  "Reason",
  "Command",
  "Path",
  "Pattern",
  "Replacement",
  "Language",
  "Action",
  "Program",
]);

/** Labels that introduce a body. */
const BLOCK_LABELS = new Set(["Content", "Code"]);

const LABEL_LINE = /^([A-Za-z][A-Za-z ]*):[ \t]?(.*)$/;

/**
 * The approval inside a dialog, or `null` when the dialog is something else.
 *
 * Recognized by shape rather than by title alone: a `select` whose two options are
 * exactly the engine's pair. An extension's own `select` that happens to start with
 * "Allow tool:" is the engine's prompt too — nothing else mints that pair.
 */
export function approvalOf(dialog: {
  kind: string;
  title: string;
  options: readonly string[];
}): ApprovalPrompt | null {
  if (dialog.kind !== "select") {
    return null;
  }
  if (dialog.options.length !== 2 || dialog.options[0] !== APPROVE || dialog.options[1] !== DENY) {
    return null;
  }
  return parseApprovalPrompt(dialog.title);
}

/** Parse the engine's approval prompt, or `null` if this is not one. */
export function parseApprovalPrompt(title: string): ApprovalPrompt | null {
  const lines = title.split("\n");
  const first = /^Allow tool:[ \t]*(.*?)[ \t]*$/.exec(lines[0] ?? "");
  if (first === null || first[1] === "") {
    return null;
  }

  const prompt: ApprovalPrompt = { tool: first[1], fields: [], blocks: [], notes: [] };
  let block: ApprovalBlock | null = null;

  for (const line of lines.slice(1)) {
    if (block !== null) {
      // Everything after a block label belongs to it, labels included.
      block.body = block.body === "" ? line : `${block.body}\n${line}`;
      continue;
    }

    if (line.trim() === "") {
      // Blank lines are the engine's spacing, not a field. Inside a body they are
      // preserved; here there is nothing to show.
      continue;
    }

    const label = LABEL_LINE.exec(line);
    const name = label?.[1];
    const value = label?.[2] ?? "";

    if (name === undefined || (!FIELD_LABELS.has(name) && !BLOCK_LABELS.has(name))) {
      prompt.notes.push(line);
      continue;
    }

    if (value !== "" || FIELD_LABELS.has(name)) {
      prompt.fields.push({ label: name, value });
      continue;
    }

    block = { label: name, body: "" };
    prompt.blocks.push(block);
  }

  // A block label with nothing under it is still a body: show it as empty rather
  // than as a field with a value of "" and let the card decide how to say so.
  return prompt;
}

/**
 * Milliseconds left on the engine's own deadline, or `null` when it set none.
 *
 * The engine sends the *duration* it armed when it sent the request, so the anchor
 * has to be when the host first saw the dialog; counting down from the duration
 * itself would never move. An approval carries no deadline at all (measured), which
 * is why this returns `null` more often than not.
 */
export function remainingMs(
  timeoutMs: number | null,
  firstSeenAt: number,
  now: number,
): number | null {
  if (timeoutMs === null) {
    return null;
  }
  return Math.max(0, timeoutMs - (now - firstSeenAt));
}

/** The deadline, said plainly. It answers the dialog itself, so this is a warning. */
export function formatRemaining(ms: number): string {
  const seconds = Math.ceil(ms / 1000);
  if (seconds >= 120) {
    return `resolves itself in ${Math.round(seconds / 60)}m unless you answer`;
  }
  return `resolves itself in ${seconds}s unless you answer`;
}

/**
 * What "always allow" does, said exactly — both halves.
 *
 * The write is *not* live: measured on v18.2.6, a session keeps asking after the
 * record changes, and the next session honours it. Copy that promised "won't ask
 * again" would read as a broken button the moment the next `bash` call asked again.
 */
export function allowEffect(tool: string): string {
  return `saved — new sessions run ${tool} without asking; this one keeps asking until it restarts`;
}
