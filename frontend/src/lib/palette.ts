/**
 * The command palette's model and rules (`docs/12` §7.3).
 *
 * The palette is **derived from the draft**, not from its own state: what the user has
 * typed before the caret says which level is showing and what is filtered, and accepting
 * an item is a text edit. That is why this module is pure and why there is no
 * "palette open" flag to get out of sync with the box — `/` opens it, a space moves it to
 * the subcommand level, and anything else closes it.
 *
 * The one rule that is easy to get wrong is what a keypress *means*: a command that wants
 * arguments must not be dispatched on one `Enter` (`docs/12` §7.3 sends dispatchable
 * builtins "verbatim as the prompt", and a bare `/compact` is a different command from
 * `/compact keep the decisions`), while a command with nothing following it is expected to
 * run. So the engine's own data decides: [`CommandEntry.hint`] or a subcommand means
 * "finish the sentence", and neither means "run it".
 */

/** Mirrors `dto::CommandSnapshot`. */
export interface CommandEntry {
  name: string;
  /** `builtin`, `custom`, `extension` or `file` — open, so a new source groups itself. */
  source: string;
  /** Alternative names, for matching only. */
  aliases: string[];
  description: string | null;
  /** What the command wants after its name. */
  hint: string | null;
  subcommands: SubcommandEntry[];
}

/** Mirrors `dto::SubcommandSnapshot`. */
export interface SubcommandEntry {
  name: string;
  description: string | null;
}

/**
 * An action the window owns, offered beside the engine's commands.
 *
 * The palette is the composer's, and a composer is not the only place a user looks for a
 * command — so the window's own surfaces are listed here too, as a source of their own. They
 * are *not* commands: nothing is sent to the engine when one is chosen, which is why
 * [`dispatchOf`] refuses them and the composer runs them itself.
 */
export interface AppAction {
  /** The id the shell dispatches on: `settings`. */
  id: string;
  /** What the row reads as. No leading slash — this is not a command. */
  label: string;
  /** The word that matches it, typed after the `/` like any other row. */
  name: string;
  description: string;
  /** The accelerator, when the same action is reachable without the palette. */
  key: string | null;
}

/** One selectable row. */
export type PaletteItem =
  | { kind: "command"; command: CommandEntry }
  | { kind: "subcommand"; command: CommandEntry; sub: SubcommandEntry }
  | { kind: "app"; app: AppAction };

/** A heading and its rows, in render order. */
export interface PaletteGroup {
  source: string;
  items: PaletteItem[];
}

/** What the palette shows for a given draft. */
export interface PaletteView {
  /** Which list this is: the commands, or one command's subcommands. */
  level: "commands" | "subcommands";
  /** At the subcommand level, the command whose subcommands these are. */
  of: string | null;
  /** What the user has typed after the `/`, lowercased. */
  query: string;
  groups: PaletteGroup[];
  /** The same rows, flattened in render order — the order the arrow keys walk. */
  items: PaletteItem[];
}

/**
 * The order sources group in, fixed by `docs/12` §7.3: the window's own actions first — they
 * are the ones the user cannot reach any other way from inside the box — then the engine's
 * builtins, then what the user or their extensions added. A source this build has never seen
 * sorts after these rather than being dropped, because a newer engine's sixth source is still
 * a command the user can run.
 */
const SOURCE_ORDER = ["window", "builtin", "custom", "extension", "file"];

/** The source the window's own actions group under. */
const WINDOW_SOURCE = "window";

/**
 * What the palette should show, or `null` when it should be closed.
 *
 * `before` is the draft up to the caret, not the whole draft: a palette driven by the
 * whole draft would stay open while the user typed an argument in the middle of the box.
 */
/** The actions the window offers in the palette, in the order it lists them. */
export const WINDOW_ACTIONS: AppAction[] = [
  {
    id: "settings",
    label: "Settings",
    name: "settings",
    description: "Every engine setting this app surfaces, and the raw config file",
    key: "⌘,",
  },
  {
    id: "log-in",
    label: "Log in to a provider",
    name: "log-in",
    description: "Opens a terminal running the engine's own credential-vault login",
    key: null,
  },
];

export function paletteFor(
  before: string,
  commands: readonly CommandEntry[],
  actions: readonly AppAction[] = [],
): PaletteView | null {
  if (!before.startsWith("/")) {
    return null;
  }

  const typed = before.slice(1);
  const [first = "", ...rest] = typed.split(/\s+/);

  // `/name …` — the subcommand level. Only reached when the command exists and has
  // subcommands: `/compact keep it` is an argument, not a subcommand, and typing it must
  // close the palette rather than filter a list that was never there.
  if (rest.length > 0) {
    const command = commands.find((entry) => entry.name === first);
    if (command === undefined || command.subcommands.length === 0) {
      return null;
    }

    const partial = rest.join(" ").toLowerCase();
    // Only while the subcommand itself is being typed: a second word is an argument.
    if (partial.includes(" ")) {
      return null;
    }

    const items: PaletteItem[] = rank(command.subcommands, partial, (sub) => sub.name).map(
      (sub) => ({ kind: "subcommand" as const, command, sub }),
    );

    if (items.length === 0) {
      return null;
    }

    return {
      level: "subcommands",
      of: command.name,
      query: partial,
      groups: [{ source: command.name, items }],
      items,
    };
  }

  // The window's own actions rank against their own list and land in their own group, which
  // sorts first: `/settings` should not have to compete with an engine command for the top row.
  const items: PaletteItem[] = [
    ...rank(actions, first.toLowerCase(), (action) => action.name).map((app) => ({
      kind: "app" as const,
      app,
    })),
    ...rank(commands, first.toLowerCase(), (entry) => entry.name, (entry) => entry.aliases).map(
      (command) => ({ kind: "command" as const, command }),
    ),
  ];
  const groups = group(items);

  // Nothing to show is closed, not empty: the commands may not have arrived yet, and text
  // that matches no command is ordinary prompt text (`docs/12` §5.2) rather than a panel
  // covering the box with a blank list.
  if (groups.length === 0) {
    return null;
  }

  return {
    level: "commands",
    of: null,
    query: first.toLowerCase(),
    groups,
    items: groups.flatMap((group) => group.items),
  };
}

/**
 * The rows that match, best first.
 *
 * Ranked rather than merely filtered, because `/m` matches eight commands and the one the
 * user meant should be first: an exact name, then a name that starts with what was typed,
 * then an alias that does, then a name that contains it, then the description. Ties keep
 * the engine's order, which is its own idea of importance.
 */
function rank<T>(
  entries: readonly T[],
  query: string,
  nameOf: (entry: T) => string,
  aliasesOf: (entry: T) => readonly string[] = () => [],
): T[] {
  if (query === "") {
    return [...entries];
  }

  const score = (entry: T): number => {
    const name = nameOf(entry).toLowerCase();
    const aliases = aliasesOf(entry).map((alias) => alias.toLowerCase());

    if (name === query) return 0;
    if (name.startsWith(query)) return 1;
    if (aliases.some((alias) => alias === query)) return 2;
    if (aliases.some((alias) => alias.startsWith(query))) return 3;
    if (name.includes(query)) return 4;
    if (aliases.some((alias) => alias.includes(query))) return 5;

    return 6;
  };

  return entries
    .map((entry, at) => ({ entry, at, score: score(entry) }))
    .filter((scored) => scored.score < 6)
    .sort((left, right) => left.score - right.score || left.at - right.at)
    .map((scored) => scored.entry);
}

/** Rows under their source headings, in the reference's source order. */
function group(items: readonly PaletteItem[]): PaletteGroup[] {
  const groups = new Map<string, PaletteItem[]>();

  for (const item of items) {
    const source = item.kind === "app" ? WINDOW_SOURCE : item.command.source;
    const bucket = groups.get(source);
    if (bucket === undefined) {
      groups.set(source, [item]);
    } else {
      bucket.push(item);
    }
  }

  return [...groups.entries()]
    .sort(([left], [right]) => order(left) - order(right))
    .map(([source, rows]) => ({ source, items: rows }));
}

function order(source: string): number {
  const at = SOURCE_ORDER.indexOf(source);

  // Unknown sources after the known ones — a stable position rather than a random one.
  return at === -1 ? SOURCE_ORDER.length : at;
}

/** A stable key for a row, for `v-for` and for the selection. */
export function itemKey(item: PaletteItem): string {
  if (item.kind === "app") return `a:${item.app.id}`;
  return item.kind === "command" ? `c:${item.command.name}` : `s:${item.command.name}:${item.sub.name}`;
}

/** What the row reads as, before its description. */
export function itemLabel(item: PaletteItem): string {
  if (item.kind === "app") return item.app.label;
  return item.kind === "command" ? `/${item.command.name}` : item.sub.name;
}

/** The row's one-line explanation, when the engine gave one. */
export function itemDetail(item: PaletteItem): string | null {
  if (item.kind === "app") return item.app.description;
  if (item.kind === "subcommand") return item.sub.description;

  // A hint is what has to follow; it is more useful than the prose when both exist.
  return item.command.hint ?? item.command.description;
}

/**
 * The text a row puts in the box, or `null` when it says nothing about the text.
 *
 * Accepting *always* leaves the caret after the name and a trailing space, which is what
 * makes the levels fall out of the text: `/security ` is the subcommand list, and
 * `/security scan` is a message.
 */
export function insertionOf(item: PaletteItem): string {
  if (item.kind === "app") return "";
  return item.kind === "command" ? `/${item.command.name} ` : `/${item.command.name} ${item.sub.name} `;
}

/**
 * The prompt to send when the row can be dispatched as it stands, or `null` when the
 * command still wants something from the user.
 *
 * `docs/12` §7.3: dispatchable builtins are sent **verbatim** and the engine expands them.
 */
export function dispatchOf(item: PaletteItem): string | null {
  // An app action is never a prompt: the composer runs it, and returning text here would put
  // a word the engine does not know into a live session.
  if (item.kind === "app" || item.kind === "subcommand") {
    return null;
  }

  const { command } = item;
  if (command.hint !== null || command.subcommands.length > 0) {
    return null;
  }

  return `/${command.name}`;
}

/**
 * Replace the `/` token being typed with `text`.
 *
 * Called only while the palette is showing, which is only while the caret sits inside a
 * `/token` at the very start of the box — so the token being replaced is exactly
 * `[0, caret)` and everything after the caret is left alone. Returns the new draft and
 * where the caret lands (after the inserted text, ready for an argument).
 */
export function replaceToken(
  draft: string,
  caret: number,
  text: string,
): { text: string; caret: number } {
  const after = draft.slice(caret);
  // The insertion ends with the space that invites an argument. If what it lands against
  // already begins with one — text after the caret, which happens when the caret is moved
  // back into the command while writing — only one of them survives, because the user's
  // own space is the one that was already there.
  const joined =
    text.endsWith(" ") && after.startsWith(" ") ? text + after.slice(1) : text + after;

  return { text: joined, caret: text.length };
}

/**
 * The next selection, wrapping.
 *
 * Wrapping rather than stopping: the list is short and the top row is one `↑` from the
 * bottom, which is how a palette with no mouse is expected to behave.
 */
export function move(key: "up" | "down", at: number, count: number): number {
  if (count === 0) {
    return -1;
  }
  if (at < 0) {
    return key === "down" ? 0 : count - 1;
  }

  return key === "down" ? (at + 1) % count : (at - 1 + count) % count;
}
