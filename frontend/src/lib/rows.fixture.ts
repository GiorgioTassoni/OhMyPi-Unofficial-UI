/**
 * The conversation rows the model tests build.
 *
 * A fixture rather than a helper beside each test: `rows.ts`, `search.ts`, `panel.ts` and
 * `agents.ts` all take rows, and four factories meant four places to touch whenever the host
 * added a field to a row — which is exactly how three of them silently drifted into
 * "whatever the last author needed" shapes. Only tests import this, so it is tree-shaken out
 * of the bundle.
 *
 * Defaults are the *quiet* row: an assistant message with no tool, nothing streaming, no
 * attachments. Every field a test cares about is passed in, so a test that reads as if it
 * were about tools cannot accidentally be about something else.
 */

import type { RowSnapshot } from "../bridge";

export function row(extra: Partial<RowSnapshot> = {}): RowSnapshot {
  return {
    role: "assistant",
    text: "",
    thinking: null,
    streaming: false,
    tool: null,
    attachments: [],
    customType: null,
    jobs: [],
    ...extra,
  };
}

/** An assistant row carrying text. */
export function said(text: string, extra: Partial<RowSnapshot> = {}): RowSnapshot {
  return row({ text, ...extra });
}
