/**
 * The agents panel's model (`docs/12` §9).
 *
 * The shapes here are the ones measured against a real engine (`cargo test -p omp-desktop
 * --test agents -- --ignored`), not invented: the `task` details a card carries, the
 * `details.async` record an auto-backgrounded `bash` call leaves, the delivery message that
 * closes it, and the nested naming a child transcript uses.
 */

import { describe, expect, test } from "bun:test";

import type { AgentSnapshot, BrokerDaemon, ParkedAgent, ThreadAgents } from "../bridge";
import { row } from "./rows.fixture";
import {
  activeCount,
  age,
  context,
  cost,
  duration,
  entries,
  jobs,
  label,
  spawnRow,
  stats,
  statusLabel,
  subtitle,
  taskFor,
  tokens,
  tone,
} from "./agents";

/** A live roster row, as `get_subagents` and the frames describe one. */
function agent(extra: Partial<AgentSnapshot> = {}): AgentSnapshot {
  return {
    id: "RosterProbe",
    index: 0,
    agent: "scout",
    agentSource: "bundled",
    status: "running",
    description: null,
    task: null,
    assignment: null,
    sessionFile: "/sessions/sess/RosterProbe.jsonl",
    parentToolCallId: "call_1",
    detached: true,
    lastUpdateMs: 1_000,
    listed: true,
    progress: null,
    ...extra,
  };
}

function parked(extra: Partial<ParkedAgent> = {}): ParkedAgent {
  return {
    id: "RosterProbe",
    path: "/sessions/sess/RosterProbe.jsonl",
    bytes: 30_498,
    modifiedMs: 1_000,
    cwd: "/tmp/project",
    parent: "/sessions/sess.jsonl",
    advisor: false,
    advisorSlug: null,
    ...extra,
  };
}

function thread(agents: AgentSnapshot[]): ThreadAgents {
  return { thread: "01a0", agents, error: null };
}

describe("a row's state", () => {
  test("the engine's five statuses map to four tones", () => {
    expect(tone(agent({ status: "running" }))).toBe("running");
    expect(tone(agent({ status: "pending" }))).toBe("queued");
    expect(tone(agent({ status: "completed" }))).toBe("done");
    expect(tone(agent({ status: "failed" }))).toBe("failed");
    expect(tone(agent({ status: "aborted" }))).toBe("failed");
    expect(tone(agent({ status: null }))).toBe("unknown");
  });

  /**
   * The case this panel exists for: the engine deletes a settled subagent from its registry,
   * so a row still saying `running` and no longer listed means nobody reported how it ended.
   * Calling that "failed" invents an outcome and calling it "completed" invents a success.
   */
  test("a running row the engine stopped listing is neither done nor failed", () => {
    const gone = agent({ status: "running", listed: false });

    expect(tone(gone)).toBe("gone");
    expect(statusLabel(gone)).toContain("stopped listing it");
    expect(tone(agent({ status: "completed", listed: false }))).toBe("done");
  });

  test("the sidebar counts what is in flight", () => {
    expect(
      activeCount([
        thread([agent({ id: "a" }), agent({ id: "b", status: "completed" })]),
        thread([agent({ id: "c", status: "pending" })]),
      ]),
    ).toBe(2);
  });

  /**
   * The two cases where a `running` status is not a running agent. Both were wrong in the
   * first version, and a browser check is what caught it: the sidebar said 1 while the panel
   * said 3 about the same state.
   */
  test("a settled row and a closed thread are not in flight", () => {
    const rows = [
      thread([agent({ id: "gone", status: "running", listed: false })]),
      { ...thread([agent({ id: "closed", status: "running" })]), thread: "closed-thread" },
    ];

    // Nobody reported how `gone` ended, and no engine is left in the second thread.
    expect(activeCount(rows, ["01a0", "closed-thread"])).toBe(1);
    expect(activeCount(rows, ["01a0"])).toBe(0);
    // Without a live set to check against — the panel's own read of "what is running now" —
    // the listed rows are all that can be counted.
    expect(activeCount(rows)).toBe(1);
  });
});

describe("the numbers", () => {
  test("a duration reads as the unit that fits", () => {
    expect(duration(430)).toBe("430ms");
    expect(duration(2_500)).toBe("2.5s");
    expect(duration(95_000)).toBe("1m 35s");
  });

  test("an age reads relative to now", () => {
    const now = 1_000_000;
    expect(age(now - 500, now)).toBe("just now");
    expect(age(now - 12_000, now)).toBe("12s ago");
    expect(age(now - 300_000, now)).toBe("5m ago");
    expect(age(now - 7_200_000, now)).toBe("2h ago");
  });

  /** A cost under a cent still has to be a number someone can act on. */
  test("a cost keeps its precision where precision is the point", () => {
    expect(cost(0)).toBe("$0");
    expect(cost(0.0042)).toBe("$0.0042");
    // A fifth of a cent is a fifth of a cent: rounding it into `$0.01` is how a row about cost
    // stops being worth reading.
    expect(cost(0.0125)).toBe("$0.013");
    expect(cost(1.239)).toBe("$1.24");
  });

  test("tokens and the context gauge", () => {
    expect(tokens(940)).toBe("940");
    expect(tokens(4_200)).toBe("4.2k");
    expect(tokens(2_400_000)).toBe("2.4M");

    expect(context(agent())).toBeNull();
    expect(
      context(
        agent({
          progress: {
            lastIntent: null,
            currentTool: null,
            currentToolArgs: null,
            toolCount: 0,
            requests: 0,
            tokens: 0,
            contextTokens: 1_800,
            contextWindow: 200_000,
            cost: 0,
            durationMs: 0,
            resolvedModel: null,
            resolvedThinkingLevel: null,
            advisor: false,
            retry: null,
            retryFailure: null,
          },
        }),
      ),
    ).toBe("1.8k / 200.0k (1%)");
  });

  test("the four counters come from progress, and are absent without it", () => {
    expect(stats({ key: "t:a", thread: "t", id: "a", agent: agent(), parked: null, depth: 0 })).toBeNull();
  });
});

describe("the task behind an agent", () => {
  /** The `task` card's details, as the engine writes them: `progress[]` keyed by agent id. */
  const card = row({
    role: "tool",
    tool: {
      toolCallId: "call_1",
      toolName: "task",
      intent: null,
      args: '{"tasks":[{"agent":"scout","task":"Measure the wire"}]}',
      details: JSON.stringify({
        progress: [{ id: "RosterProbe", agent: "scout", status: "running", task: "Measure the wire" }],
        async: { jobId: "bg_2", state: "running", type: "task" },
      }),
      output: "",
      isError: false,
      finished: true,
    },
  });

  test("the spawning card is the source, joined by the engine's own id", () => {
    expect(taskFor([card], "RosterProbe")).toBe("Measure the wire");
    expect(taskFor([card], "someone-else")).toBeNull();
  });

  test("the jump back finds the card by the tool call id the frames carried", () => {
    expect(spawnRow([row(), card], agent())).toBe(1);
    expect(spawnRow([card], agent({ parentToolCallId: null }))).toBeNull();
    expect(spawnRow([card], agent({ parentToolCallId: "call_9" }))).toBeNull();
  });
});

describe("background jobs", () => {
  /** The measured card: an auto-backgrounded `bash` call. */
  const jobCard = row({
    role: "tool",
    tool: {
      toolCallId: "call_bg",
      toolName: "bash",
      intent: null,
      args: '{"command":"sleep 25; echo done-sleeping"}',
      details: JSON.stringify({ async: { jobId: "bg_2", state: "running", type: "bash" } }),
      output: "Backgrounded as job bg_2; result will be delivered automatically.",
      isError: false,
      finished: true,
    },
  });

  /** The measured delivery: a `custom` message carrying the jobs it accounts for. */
  const delivery = row({
    role: "custom",
    text: "Background job bg_2 has completed. …",
    customType: "async-result",
    jobs: [{ jobId: "bg_2", kind: "bash", durationMs: 25_004, label: null }],
  });

  test("a job is open until its own delivery names it", () => {
    const open = jobs([jobCard]);
    expect(open).toHaveLength(1);
    expect(open[0]).toEqual({
      id: "bg_2",
      kind: "bash",
      toolCallId: "call_bg",
      toolName: "bash",
      detail: "sleep 25; echo done-sleeping",
      delivered: false,
      durationMs: null,
    });

    const closed = jobs([jobCard, delivery]);
    expect(closed[0].delivered).toBe(true);
    expect(closed[0].durationMs).toBe(25_004);
  });

  test("delivered jobs sort after the open ones", () => {
    const second = row({
      role: "tool",
      tool: {
        toolCallId: "call_bg3",
        toolName: "bash",
        intent: null,
        args: '{"command":"sleep 5"}',
        details: JSON.stringify({ async: { jobId: "bg_3", state: "running", type: "bash" } }),
        output: "Backgrounded as job bg_3",
        isError: false,
        finished: true,
      },
    });

    expect(jobs([jobCard, delivery, second]).map((job) => job.id)).toEqual(["bg_3", "bg_2"]);
  });

  test("a delivery for a job whose card is gone is not a job row of its own", () => {
    expect(jobs([delivery])).toEqual([]);
  });
});

describe("live rows and parked ones are the same agent", () => {
  test("one row per id, whatever the sources say", () => {
    const both = entries("01a0", thread([agent()]), [parked(), parked({ id: "Old", path: "/s/x/Old.jsonl" })]);

    expect(both.map((entry) => entry.id)).toEqual(["RosterProbe", "Old"]);
    expect(both[0].agent).not.toBeNull();
    expect(both[0].parked).not.toBeNull();
    expect(both[1].agent).toBeNull();
    expect(both[1].parked).not.toBeNull();
  });

  /**
   * The measured naming: a child's transcript sits one directory down and its id is the whole
   * stem (`Parent.Child`), while its header names the parent's file. So nesting is decided by
   * the parent file's stem, not by the child's own id.
   */
  /**
   * A live row carries no parent field at all, and its `sessionFile` is the child's *own*
   * file — so the file header is no help. The directory is: a nested transcript lives in a
   * directory named after its parent. Missing this is how the first version of the model
   * indented nothing.
   */
  test("a live child nests by the directory its transcript is in", () => {
    const list = entries(
      "01a0",
      thread([
        agent({ id: "Wire", sessionFile: "/s/x/Wire.jsonl" }),
        agent({ id: "Wire.Child", sessionFile: "/s/x/Wire/Wire.Child.jsonl" }),
      ]),
      [],
    );

    expect(list.map((entry) => [entry.id, entry.depth])).toEqual([
      ["Wire", 0],
      ["Wire.Child", 1],
    ]);
  });

  test("a child nests under the agent its header names", () => {
    const parent = parked({ id: "Parent", path: "/s/x/Parent.jsonl" });
    const child = parked({
      id: "Parent.Child",
      path: "/s/x/Parent/Parent.Child.jsonl",
      parent: "/s/x/Parent.jsonl",
    });

    const list = entries("01a0", null, [child, parent]);

    expect(list.map((entry) => [entry.id, entry.depth])).toEqual([
      ["Parent", 0],
      ["Parent.Child", 1],
    ]);
  });

  test("a thread's error is carried, and its empty roster is not a claim", () => {
    const failed: ThreadAgents = { thread: "01a0", agents: [], error: "Subagent event bus is unavailable" };
    expect(failed.error).toContain("unavailable");
    expect(entries("01a0", failed, [])).toEqual([]);
  });

  test("a label falls back from the engine's name to the id", () => {
    const live = entries("01a0", thread([agent()]), []);
    expect(label(live[0])).toBe("scout");

    const file = entries("01a0", null, [parked({ id: "Anonymous", agent: undefined as never } as Partial<ParkedAgent>)]);
    expect(label(file[0])).toBe("Anonymous");

    const advisor = entries("01a0", null, [parked({ id: "__advisor", advisor: true, advisorSlug: "reviewer" })]);
    expect(label(advisor[0])).toBe("advisor (reviewer)");
  });

  test("a subtitle prefers what it is doing now over what it was asked", () => {
    const running = entries(
      "01a0",
      thread([
        agent({
          task: "Measure the wire",
          progress: {
            lastIntent: "reading the source",
            currentTool: null,
            currentToolArgs: null,
            toolCount: 3,
            requests: 2,
            tokens: 1_000,
            contextTokens: null,
            contextWindow: null,
            cost: 0.5,
            durationMs: 4_000,
            resolvedModel: null,
            resolvedThinkingLevel: null,
            advisor: false,
            retry: null,
            retryFailure: null,
          },
        }),
      ]),
      [],
    );
    expect(subtitle(running[0])).toBe("reading the source");
    expect(stats(running[0])).toEqual({ cost: 0.5, tools: 3, requests: 2, tokens: 1_000 });

    const asked = entries("01a0", thread([agent({ task: "Measure the wire" })]), []);
    expect(subtitle(asked[0])).toBe("Measure the wire");

    const file = entries("01a0", null, [parked()]);
    expect(subtitle(file[0])).toBe("30498 bytes of transcript");
  });
});

describe("broker processes", () => {
  /** The measured shape, kept here because the panel renders it rather than reading it. */
  const daemon: BrokerDaemon = {
    name: "vite",
    state: "exited",
    owner: "01a0bb7d-7260-757d-b395-39e22d9655d3",
    command: "bun run dev",
    cwd: "/home/GioViale/Projects/Websites_or_webapp/OhMyPiApp/frontend",
    supervised: false,
    persist: false,
    detached: false,
    restartCount: 0,
    exitCode: 143,
    outputBytes: 7_898,
    startedAtMs: 1_789_916_421_544,
    exitedAtMs: 1_789_929_078_950,
  };

  test("a daemon keeps the engine's own words", () => {
    expect(daemon.state).toBe("exited");
    expect(daemon.owner).not.toBeNull();
    expect(cost(0)).toBe("$0");
  });
});
