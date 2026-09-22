/**
 * Cross-thread search (`docs/12` §7.4, decision D7), as a pure model.
 *
 * The engine has no equivalent — its `history.db` indexes *prompts*, not message text or tool
 * activity — so this is the one surface whose correctness is entirely the app's. The index
 * itself lives in the host (SQLite FTS over the session files, via `omp-store`) because a
 * cold session has no live sidecar to ask; what belongs here is everything between a ranked
 * hit list and a screen: the snippet around the match, how hits collapse into threads, and
 * which row a hit is talking about.
 *
 * **Snippets are character-safe.** A session is full of code, and one `slice()` by code unit
 * through an emoji or a CJK character produces a replacement character in the middle of a
 * result — so the window is measured in code points and cut on their boundaries.
 */

import type { RowSnapshot, SearchHit, SessionSummary } from "../bridge";

export type { SearchHit };

export interface HitGroup {
  thread: string;
  /** The thread's display name, so a result can be read without the sidebar. */
  title: string;
  hits: SearchHit[];
  /** True when the thread has more matches than the group shows. */
  more: boolean;
}

export interface Snippet {
  before: string;
  match: string;
  after: string;
}

/**
 * The first index where `needle` appears in `haystack`, case-insensitively, in **code points**.
 *
 * Folded character by character rather than by lowercasing the whole text: `toLowerCase()`
 * can change a string's length (`İ` becomes two code units), which would shift every index
 * after it and put the window in the wrong place. Comparing one character at a time keeps the
 * index an index into `haystack`, which is what the slicing below assumes — mixing a UTF-16
 * offset with an array of code points is exactly the bug that put emoji in a snippet that
 * should have read `needle`.
 */
function indexOfFolded(haystack: string[], needle: string): number {
  const wanted = [...needle].map((character) => character.toLowerCase());
  if (wanted.length === 0 || wanted.length > haystack.length) return -1;

  outer: for (let start = 0; start <= haystack.length - wanted.length; start += 1) {
    for (let offset = 0; offset < wanted.length; offset += 1) {
      if (haystack[start + offset]?.toLowerCase() !== wanted[offset]) continue outer;
    }
    return start;
  }
  return -1;
}

/** How many hits one thread's group shows before it just says how many more there are. */
const HITS_PER_GROUP = 3;

/** Characters of context either side of the match. */
const SNIPPET_WIDTH = 90;

/**
 * The window around the first match, or `null` when the text does not match at all.
 *
 * Case-insensitive, but the original casing is returned — a snippet that lower-cased the
 * match would make code look like prose.
 */
export function snippet(text: string, query: string, width = SNIPPET_WIDTH): Snippet | null {
  const needle = query.trim();
  if (needle === "") return null;

  const haystack = [...text];
  const at = indexOfFolded(haystack, needle);
  if (at < 0) return null;

  const start = Math.max(0, at - width);
  const end = Math.min(haystack.length, at + needle.length + width);

  return {
    before: (start > 0 ? "…" : "") + haystack.slice(start, at).join("").trimStart(),
    match: haystack.slice(at, at + needle.length).join(""),
    after:
      haystack
        .slice(at + needle.length, end)
        .join("")
        .trimEnd() + (end < haystack.length ? "…" : ""),
  };
}

/**
 * Collapse hits into threads, best first.
 *
 * The host sorts by relevance, one hit at a time, so a thread's position is the position of
 * its best hit — which is what a reader expects when they see threads rather than documents.
 * A thread missing from the catalogue is still shown: it can be a session the index knows and
 * the last listing predates.
 */
export function groupHits(
  hits: SearchHit[],
  sessions: SessionSummary[],
  perGroup = HITS_PER_GROUP,
): HitGroup[] {
  const named = new Map(sessions.map((session) => [session.id, session]));
  const groups = new Map<string, HitGroup>();

  for (const hit of hits) {
    const existing = groups.get(hit.thread);
    if (existing) {
      if (existing.hits.length < perGroup) existing.hits.push(hit);
      else existing.more = true;
      continue;
    }

    const session = named.get(hit.thread);
    groups.set(hit.thread, {
      thread: hit.thread,
      title: session ? (session.title ?? session.firstMessage) : hit.thread,
      hits: [hit],
      more: false,
    });
  }

  return [...groups.values()];
}

/**
 * Which rendered row a hit is talking about, or `null` when none is on screen.
 *
 * Deliberately text-matching rather than ordinal-matching: the app's rows are its own
 * reduction of the message stream (a message can become a card, a notice, or nothing), so a
 * stored ordinal would point at the wrong place the moment the reducer changes. The hit's own
 * text is what the reader is looking for, and it is still in the row.
 */
export function jumpTarget(rows: RowSnapshot[], hit: SearchHit): number | null {
  // A truncated record has "… [truncated]" appended by the index; match on what is real.
  const needle = hit.text.replace(/… \[truncated\]$/, "").slice(0, 120).trim();
  if (needle === "") return null;

  const at = rows.findIndex((row) => row.text.includes(needle) || row.thinking === needle);
  if (at >= 0) return at;

  // A tool record is "name {json}", and the row carries the name and its own formatting of
  // the arguments: match on the first token, which is the tool's name.
  const [name] = needle.split(" ");
  if (name === "") return null;
  // `findIndex` answers -1 for "nowhere"; that is not a row index, and returning it would
  // scroll the transcript to a row that does not exist.
  const tool = rows.findIndex((row) => row.tool?.toolName === name);
  return tool >= 0 ? tool : null;
}

/** The label a hit's kind gets on screen, or `null` when it needs none. */
export function kindLabel(kind: string): string | null {
  switch (kind) {
    case "thinking":
      return "reasoning";
    case "tool":
      return "tool call";
    case "result":
      return "tool result";
    default:
      return null;
  }
}
