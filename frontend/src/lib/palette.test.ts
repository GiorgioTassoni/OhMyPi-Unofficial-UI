/**
 * The palette's rules, asserted (`docs/12` §7.3).
 *
 * The cases here are the ones a template hides: a command dispatched before the user
 * finished typing its argument, a subcommand list that appears for a command that has
 * none, a panel that stays open over text the engine would treat as prose, and an empty
 * list rendered as if the session had no commands.
 */

import { describe, expect, test } from "bun:test";
import {
  type CommandEntry,
  dispatchOf,
  insertionOf,
  itemDetail,
  itemKey,
  itemLabel,
  move,
  paletteFor,
  replaceToken,
  WINDOW_ACTIONS,
  type PaletteView,
} from "./palette";

/**
 * The rows read as commands.
 *
 * `PaletteItem` is a union the window's own actions are part of now, so a test about the
 * engine's commands says which side of it it means — and a window row that leaked into one of
 * these lists would fail the count rather than being silently skipped.
 */
function names(view: PaletteView | null): string[] {
  return (view?.items ?? []).flatMap((item) => (item.kind === "command" ? [item.command.name] : []));
}

function command(entry: Partial<CommandEntry> & { name: string }): CommandEntry {
  return {
    source: "builtin",
    aliases: [],
    description: null,
    hint: null,
    subcommands: [],
    ...entry,
  };
}

/** Real rows, trimmed from a live `get_available_commands` on v18.2.6. */
const commands: CommandEntry[] = [
  command({
    name: "model",
    aliases: ["models"],
    description: "Show current model selection",
  }),
  command({
    name: "compact",
    description: "Compact the conversation",
    hint: "[instructions]",
    subcommands: [
      { name: "now", description: "Compact immediately" },
      { name: "status", description: "Show compaction state" },
    ],
  }),
  command({
    name: "security",
    description: "Plan, run, inspect, import, and compare security scans",
    hint: "<plan|scan|status>",
    subcommands: [
      { name: "plan", description: "Create a scan plan" },
      { name: "scan", description: "Start a scan" },
      { name: "status", description: "Show scan status" },
    ],
  }),
  command({ name: "todo", description: "Manage the todo list" }),
  command({
    name: "green",
    source: "custom",
    description: "A project's own command",
  }),
  command({ name: "autoresearch", source: "extension" }),
  command({ name: "init", source: "file" }),
];

describe("when the palette shows", () => {
  test("only inside a slash token at the start of the box", () => {
    expect(paletteFor("hello /model", commands)).toBeNull();
    expect(paletteFor("model", commands)).toBeNull();
    expect(paletteFor("", commands)).toBeNull();
    // No command matches: the text is prose, and a blank panel over the box would be a
    // lie about what is available (`docs/12` §5.2).
    expect(paletteFor("/nosuchthing", commands)).toBeNull();
    // The commands have not arrived yet — nothing to offer, so nothing opens.
    expect(paletteFor("/", [])).toBeNull();
  });

  test("a bare slash offers everything, grouped in the reference's source order", () => {
    const view = paletteFor("/", commands);
    expect(view?.level).toBe("commands");
    expect(view?.groups.map((group) => group.source)).toEqual([
      "builtin",
      "custom",
      "extension",
      "file",
    ]);
    expect(names(view)).toHaveLength(commands.length);
  });

  test("an unknown source sorts after the known ones rather than vanishing", () => {
    const view = paletteFor("/", [
      ...commands,
      command({ name: "future", source: "workspace" }),
    ]);

    expect(view?.groups.at(-1)?.source).toBe("workspace");
    expect(names(view)).toContain("future");
  });
});

describe("what the query matches", () => {
  test("the name first, then an alias, then anything that contains it", () => {
    // `/mo` matches eight names in a real list; the one the user meant has to be first.
    const view = paletteFor("/mo", commands);
    expect(names(view)[0]).toBe("model");
  });

  test("an alias finds its command without being offered as text", () => {
    // `model` answers to `models`; the alias matches, and the row still says `/model`
    // because an alias is not necessarily something the engine expands.
    const view = paletteFor("/models", commands);
    expect(names(view)).toEqual(["model"]);
    expect(itemLabel(view!.items[0]!)).toBe("/model");
  });

  test("a name that contains the query ranks below a name that starts with it", () => {
    const view = paletteFor("/den", [
      command({ name: "garden" }),
      command({ name: "dense" }),
    ]);

    expect(names(view)).toEqual(["dense", "garden"]);
  });

  test("the description is searched last, and only when nothing better matched", () => {
    const view = paletteFor("/todo", commands);
    expect(names(view)[0]).toBe("todo");
  });

  test("case does not matter", () => {
    expect(names(paletteFor("/MODEL", commands))[0]).toBe("model");
  });
});

describe("the subcommand level", () => {
  test("a command with subcommands opens a second level", () => {
    const view = paletteFor("/security ", commands);

    expect(view?.level).toBe("subcommands");
    expect(view?.of).toBe("security");
    expect(view?.items.map((item) => itemLabel(item))).toEqual([
      "plan",
      "scan",
      "status",
    ]);
  });

  test("the subcommand list narrows as the user types it", () => {
    const view = paletteFor("/security sc", commands);
    expect(view?.items.map((item) => itemLabel(item))).toEqual(["scan"]);
  });

  test("an argument closes the palette instead of filtering an empty list", () => {
    // The case a naive implementation gets wrong: `/compact keep the decisions` is a
    // message, not a subcommand of `compact`, and offering "no matching subcommand" over
    // the user's own words would be wrong twice.
    expect(paletteFor("/compact keep the decisions", commands)).toBeNull();
    expect(paletteFor("/security scan --json", commands)).toBeNull();
    expect(paletteFor("/model warm", commands)).toBeNull();
  });
});

describe("what a row does when it is accepted", () => {
  test("a command with nothing after it is dispatched, verbatim", () => {
    const item = paletteFor("/model", commands)!.items[0]!;

    expect(dispatchOf(item)).toBe("/model");
    expect(insertionOf(item)).toBe("/model ");
  });

  test("a command that wants arguments is never dispatched on one keypress", () => {
    // The rule that protects the user's intent: `/compact` and `/compact keep it` are
    // different commands, and the engine's own `hint` is what says so.
    for (const typed of ["/compact", "/security"]) {
      const item = paletteFor(typed, commands)!.items[0]!;
      expect(dispatchOf(item)).toBeNull();
      expect(insertionOf(item)).toBe(`${typed} `);
    }
  });

  test("a subcommand is offered as text, never as a finished command", () => {
    const item = paletteFor("/security ", commands)!.items[0]!;

    expect(dispatchOf(item)).toBeNull();
    expect(insertionOf(item)).toBe("/security plan ");
  });

  test("a row shows what has to follow it, or failing that what it does", () => {
    const withHint = paletteFor("/compact", commands)!.items[0]!;
    const withProse = paletteFor("/model", commands)!.items[0]!;

    expect(itemDetail(withHint)).toBe("[instructions]");
    expect(itemDetail(withProse)).toBe("Show current model selection");
  });

  test("a row is keyed by what it is, not by where it is", () => {
    const first = paletteFor("/security ", commands)!.items[0]!;
    const second = paletteFor("/security ", commands)!.items[1]!;

    expect(itemKey(first)).toBe("s:security:plan");
    expect(itemKey(second)).toBe("s:security:scan");
    expect(itemKey(paletteFor("/model", commands)!.items[0]!)).toBe("c:model");
  });
});

describe("the keyboard", () => {
  test("the selection wraps in both directions", () => {
    expect(move("down", 0, 3)).toBe(1);
    expect(move("down", 2, 3)).toBe(0);
    expect(move("up", 0, 3)).toBe(2);
    expect(move("up", 2, 3)).toBe(1);
  });

  test("nothing selected yet means the ends, by direction", () => {
    // Reaching the palette with `↑` should land on the last row, not on the first: the
    // user is coming from below.
    expect(move("down", -1, 4)).toBe(0);
    expect(move("up", -1, 4)).toBe(3);
    expect(move("down", -1, 0)).toBe(-1);
  });

  test("accepting replaces the token and leaves the caret after it", () => {
    expect(replaceToken("/mo", 3, "/model ")).toEqual({
      text: "/model ",
      caret: 7,
    });
    // Text after the caret survives, and the join is not a double space: the caret was
    // moved back into the command, so the user's own space is the one that stays.
    expect(replaceToken("/mo and more", 3, "/model ")).toEqual({
      text: "/model and more",
      caret: 7,
    });
  });
});

/**
 * The window's own rows (`docs/12` §12's entry point).
 *
 * A palette row that opens settings is not a command and must never behave like one: the one
 * thing that would go wrong quietly is `Enter` sending `/settings` to the engine as a prompt,
 * so that is asserted rather than assumed.
 */
describe("the window's own actions", () => {
  test("they group under their own source, first, and are offered alongside the commands", () => {
    const view = paletteFor("/", commands, WINDOW_ACTIONS);

    expect(view?.groups[0]?.source).toBe("window");
    expect(view?.items[0]?.kind).toBe("app");
    expect(names(view)).toHaveLength(commands.length);
  });

  test("typing the name finds it and nothing else", () => {
    const view = paletteFor("/settings", commands, WINDOW_ACTIONS);
    expect(view?.items).toHaveLength(1);
    expect(itemLabel(view!.items[0]!)).toBe("Settings");
    expect(itemDetail(view!.items[0]!)).toContain("raw config");
  });

  test("it is never dispatched as a prompt, and completing it writes no text", () => {
    const item = paletteFor("/settings", commands, WINDOW_ACTIONS)!.items[0]!;
    expect(dispatchOf(item)).toBeNull();
    expect(insertionOf(item)).toBe("");
  });

  test("they are offered before the engine's commands have arrived, because they are not ones", () => {
    const view = paletteFor("/", [], WINDOW_ACTIONS);
    // Every window action, and no assumption about how many there are: a count would fail the
    // next time the window offers another, which is not what this test is about.
    expect(view?.items.map((item) => itemLabel(item))).toEqual(
      WINDOW_ACTIONS.map((action) => action.label),
    );
    // And with no window actions either, the palette stays closed rather than empty.
    expect(paletteFor("/", [])).toBeNull();
  });
});
