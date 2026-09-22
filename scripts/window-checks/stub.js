// The Tauri host, stubbed, for a webview I own.
//
// Loaded at document-start so the frontend boots against it. Every command is recorded
// and answered with a plausible shape; nothing here is the product, and no assertion in
// the checks reads this file — it is the *environment*, and what it records is what the
// window asked the host to do.

window.__calls = [];
window.__errors = [];
window.addEventListener("error", (event) => {
  window.__errors.push(String(event.message ?? event.error ?? "error"));
});
window.addEventListener("unhandledrejection", (event) => {
  window.__errors.push("unhandled: " + String(event.reason));
});
window.__callbacks = {};
window.__nextCallback = 1;
window.__fired = [];

/**
 * The stub's session, in `dto::SessionStatus`'s shape.
 *
 * Field names and nesting are the contract: `approvalMode` is top-level (the host
 * launched with it), the model and the context window live under `control`, and a
 * flattened `session.model` would render as "no model" on the chip — which is how this
 * stub first fooled me.
 */
window.__status = {
  binary: "omp",
  workspace: "/tmp/omp-8d-app",
  sidecarPid: 4242,
  approvalMode: "write",
  ready: {
    protocolVersion: 1,
    supportedProtocolVersions: [1, 2],
    maxFrameBytes: 1048576,
    maxReassembledFrameBytes: 4194304,
    negotiatedV2: true,
  },
  control: {
    sessionId: "stub-session",
    sessionName: "8d verification",
    model: { provider: "local", id: "tiny", name: "Tiny" },
    thinkingLevel: "high",
    isStreaming: false,
    isCompacting: false,
    messageCount: 2,
    queuedMessageCount: 0,
    context: { tokens: 15145, contextWindow: 1000000, percent: 1.5 },
    todoPhaseCount: 0,
    transcriptRows: 2,
    sessionFile: "/tmp/stub/session.jsonl",
    autoCompactionEnabled: true,
  },
  counters: { eventsSeen: 12, framesSeen: 14, eventKinds: 4, unknownEvents: 0 },
};


/**
 * The palette's list, in `dto::CommandSnapshot`'s shape — flattened, the way the host
 * sends it.
 *
 * Names, sources and hints are the 45 rows a live v18.2.6 session advertised; the
 * subcommand descriptions are trimmed. The shape matters more than the contents: an entry
 * is the *frontend's* contract, so `aliases`, `description`, `hint` and `subcommands` are
 * always present and never absent — a fixture that sent the engine's raw `input: {hint}`
 * made the palette throw, and Vue kept rendering the last good list (which is how a
 * missing `aliases` array looked like "the filter does nothing").
 */
window.__commands = [
  { name: "security", source: "builtin", aliases: [], description: "Plan, run, inspect and compare security scans", hint: "<plan|scan|status|cancel>", subcommands: [
    { name: "plan", description: "Create an immutable security scan plan" },
    { name: "scan", description: "Start a planned or newly planned native scan" },
    { name: "status", description: "Show native scan operation status" },
    { name: "cancel", description: "Cancel a running native scan" },
  ] },
  { name: "model", source: "builtin", aliases: ["models"], description: "Show current model selection", hint: null, subcommands: [] },
  { name: "switch", source: "builtin", aliases: [], description: "Switch the session's model", hint: "<provider/model>", subcommands: [] },
  { name: "compact", source: "builtin", aliases: [], description: "Compact the conversation", hint: "[instructions]", subcommands: [
    { name: "now", description: "Compact immediately" },
    { name: "status", description: "Show compaction state" },
  ] },
  { name: "todo", source: "builtin", aliases: [], description: "Manage the todo list", hint: "<subcommand>", subcommands: [
    { name: "add", description: "Add a task" },
    { name: "list", description: "List the phases" },
  ] },
  { name: "trace", source: "builtin", aliases: [], description: "Toggle the trace log", hint: null, subcommands: [] },
  { name: "usage", source: "builtin", aliases: [], description: "Show token usage", hint: "[days]", subcommands: [] },
  { name: "context", source: "builtin", aliases: [], description: "Show the context breakdown", hint: null, subcommands: [] },
  { name: "tools", source: "builtin", aliases: [], description: "List the available tools", hint: null, subcommands: [] },
  { name: "rename", source: "builtin", aliases: [], description: "Rename this session", hint: "<name>", subcommands: [] },
  { name: "green", source: "custom", aliases: [], description: "A project's own command", hint: "[files]", subcommands: [] },
  { name: "autoresearch", source: "extension", aliases: [], description: "An extension's command", hint: "<topic>", subcommands: [] },
  { name: "init", source: "file", aliases: [], description: "A command from a file in the workspace", hint: null, subcommands: [] },
];

/**
 * The world this stub answers for: the shape the *host* sends, not the engine's.
 *
 * Written after the thread refactor, where the app holds several sessions at once: a
 * catalogue of sessions on disk (two projects, one of them with a fork), the live set, and a
 * transcript per thread. The point is that every id here is a *session id* — the same key
 * the catalogue lists on disk — so a check can assert that a click on a row opened *that*
 * session.
 */
window.__world = {
  /**
   * The terminal world (`docs/12` §11).
   *
   * The *host* owns the shells, so the fixture does too: a tab is a row in
   * `dto::TerminalSnapshot`'s shape, opening one appends it and announces the set (which is
   * how the window learns a tab it did not ask for), typing is recorded where a check can read
   * it, and a resize is recorded because the emulator measuring itself is the thing that makes
   * a shell draw correctly.
   *
   * Output is not generated here: a check fires `terminal-output` itself
   * (`window.__fire("terminal-output", {id, data: btoa("...")})`), because what is under test is
   * what the *window* does with a batch, not what a shell would have printed.
   */
  terminals: [
    { id: "pty-1", cwd: "/tmp/omp-shell-app", running: true, exit: null, pid: 5150 },
  ],
  terminalInput: [],
  terminalSizes: [],
  terminalClosed: [],
  todos: {
    "01a0shell0001": [
      {
        name: "Build",
        tasks: [
          { content: "Wire the sidebar", status: "completed", blocker: null },
          { content: "Read the workspace tree", status: "in_progress", blocker: null },
          { content: "Ship it", status: "blocked", blocker: "the reviewer" },
        ],
      },
      { name: "Polish", tasks: [{ content: "Tidy the diff view", status: "pending", blocker: null }] },
    ],
  },
  tree: {
    "/tmp/omp-shell-app": [
      { name: "src", path: "/tmp/omp-shell-app/src", isDir: true, size: 0, modifiedAt: 1 },
      { name: "README.md", path: "/tmp/omp-shell-app/README.md", isDir: false, size: 2048, modifiedAt: 2 },
    ],
    "/tmp/omp-shell-app/src": [
      { name: "lib.rs", path: "/tmp/omp-shell-app/src/lib.rs", isDir: false, size: 4096, modifiedAt: 3 },
    ],
  },
  /**
   * The agents world (`docs/12` §9).
   *
   * The shapes are the measured ones: a roster row as `get_subagents` and the frames describe
   * it, a parked transcript beside a session file, a transcript page with its byte cursor,
   * and `omp ps --json`'s own scopes and daemons.
   */
  agents: {
    "01a0shell0001": [
      {
        id: "Wire",
        index: 0,
        agent: "scout",
        agentSource: "bundled",
        status: "running",
        description: null,
        task: "Find the sidebar's wiring",
        assignment: null,
        sessionFile: "/tmp/omp-shell-app/sessions/shell/Wire.jsonl",
        parentToolCallId: "call_bg",
        detached: true,
        lastUpdateMs: Date.now() - 4000,
        listed: true,
        progress: {
          lastIntent: "reading the sidebar",
          currentTool: "read",
          currentToolArgs: '{"path":"src/components/Sidebar.vue"}',
          toolCount: 7,
          requests: 3,
          tokens: 4200,
          contextTokens: 1800,
          contextWindow: 200000,
          cost: 0.0125,
          durationMs: 9000,
          resolvedModel: "anthropic/sonnet:hi",
          resolvedThinkingLevel: "hi",
          advisor: false,
          retry: null,
          retryFailure: null,
        },
      },
      {
        id: "Wire.Child",
        index: 1,
        agent: "sonic",
        agentSource: "bundled",
        status: "completed",
        description: null,
        task: "Rename the anchor",
        assignment: null,
        sessionFile: "/tmp/omp-shell-app/sessions/shell/Wire/Wire.Child.jsonl",
        parentToolCallId: "call_bg",
        detached: true,
        lastUpdateMs: Date.now() - 60000,
        listed: false,
        progress: null,
      },
    ],
    "01a0shell0004": [
      {
        id: "Stalled",
        index: 0,
        agent: "reviewer",
        agentSource: "user",
        status: "running",
        description: null,
        task: "Review the diff",
        assignment: null,
        sessionFile: null,
        parentToolCallId: null,
        detached: false,
        lastUpdateMs: Date.now() - 90000,
        listed: false,
        progress: null,
      },
    ],
  },
  parked: {
    "01a0shell0001": [
      {
        id: "Older",
        path: "/tmp/omp-shell-app/sessions/shell/Older.jsonl",
        bytes: 30498,
        modifiedMs: Date.now() - 3600000,
        cwd: "/tmp/omp-shell-app",
        parent: "/tmp/omp-shell-app/sessions/shell/01a0shell0001.jsonl",
        advisor: false,
        advisorSlug: null,
      },
      {
        id: "__advisor",
        path: "/tmp/omp-shell-app/sessions/shell/__advisor.jsonl",
        bytes: 1200,
        modifiedMs: Date.now() - 7200000,
        cwd: "/tmp/omp-shell-app",
        parent: "/tmp/omp-shell-app/sessions/shell/01a0shell0001.jsonl",
        advisor: true,
        advisorSlug: null,
      },
    ],
  },
  agentTranscripts: {
    "01a0shell0001": {
      Wire: {
        path: "/tmp/omp-shell-app/sessions/shell/Wire.jsonl",
        source: "engine",
        rows: [
          { role: "assistant", text: "reading the sidebar", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
        ],
        nextByte: 2048,
      },
      Older: {
        path: "/tmp/omp-shell-app/sessions/shell/Older.jsonl",
        source: "file",
        rows: [
          { role: "assistant", text: "the older agent's last words", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
        ],
        nextByte: 30498,
      },
    },
  },
  brokers: [
    {
      kind: "project",
      projectDir: "/tmp/omp-shell-app",
      runtimeDir: "/home/omp/.omp/run/daemons/011f02c4f8183799",
      brokerPid: null,
      daemons: [
        {
          name: "vite",
          state: "running",
          owner: "01a0shell0001",
          command: "bun run dev",
          cwd: "/tmp/omp-shell-app/frontend",
          supervised: false,
          persist: false,
          detached: false,
          restartCount: 0,
          exitCode: null,
          outputBytes: 7898,
          startedAtMs: Date.now() - 600000,
          exitedAtMs: null,
        },
      ],
    },
  ],
  artifacts: {
    "7f3c": {
      id: "7f3c",
      path: "/tmp/omp-shell-app-sessions/2026-09-20_01a0shell0001/7f3c.bash.log",
      bytes: 4194304,
      text: "line 1\nline 2\n… the full output the card truncated",
      truncated: true,
    },
  },
  projects: [
    { path: "/tmp/omp-shell-app", name: "omp-shell-app", sessionCount: 3, hidden: false },
    { path: "/tmp/omp-shell-other", name: "omp-shell-other", sessionCount: 1, hidden: false },
    { path: "/tmp/omp-shell-archived", name: "omp-shell-archived", sessionCount: 1, hidden: true },
  ],
  sessions: [
    {
      id: "01a0shell0001", path: "/tmp/omp-shell-app/a.jsonl", bucket: "-tmp-omp-shell-app",
      cwd: "/tmp/omp-shell-app", title: "Wire the sidebar", titleSource: "user", parentId: null,
      createdAt: "2026-09-20T10:00:00.000Z", modifiedAt: 1000, messageCount: 12, size: 4096,
      firstMessage: "wire the sidebar", status: "complete", pinned: true, suspended: false,
    },
    {
      id: "01a0shell0002", path: "/tmp/omp-shell-app/b.jsonl", bucket: "-tmp-omp-shell-app",
      cwd: "/tmp/omp-shell-app", title: null, titleSource: null, parentId: "01a0shell0001",
      createdAt: "2026-09-20T10:05:00.000Z", modifiedAt: 3000, messageCount: 4, size: 2048,
      firstMessage: "fork me and continue", status: "complete", pinned: false, suspended: false,
    },
    {
      id: "01a0shell0003", path: "/tmp/omp-shell-app/c.jsonl", bucket: "-tmp-omp-shell-app",
      cwd: "/tmp/omp-shell-app", title: null, titleSource: null, parentId: null,
      createdAt: "2026-09-20T09:00:00.000Z", modifiedAt: 2000, messageCount: 30, size: 8192,
      firstMessage: "a session whose last turn was cut off", status: "interrupted", pinned: false, suspended: false,
    },
    {
      id: "01a0shell0004", path: "/tmp/omp-shell-other/d.jsonl", bucket: "-tmp-omp-shell-other",
      cwd: "/tmp/omp-shell-other", title: "Another project", titleSource: "auto", parentId: null,
      createdAt: "2026-09-20T08:00:00.000Z", modifiedAt: 1000, messageCount: 2, size: 512,
      firstMessage: "hello from elsewhere", status: "complete", pinned: false, suspended: false,
    },
    {
      id: "01a0shell0005", path: "/tmp/omp-shell-archived/e.jsonl", bucket: "-tmp-omp-shell-archived",
      cwd: "/tmp/omp-shell-archived", title: "Hidden project session", titleSource: "auto", parentId: null,
      createdAt: "2026-09-20T07:00:00.000Z", modifiedAt: 900, messageCount: 2, size: 512,
      firstMessage: "should not be listed", status: "complete", pinned: false, suspended: false,
    },
  ],
  /** Threads with a sidecar. Empty at first: the browser opens them. */
  live: [],
  /**
   * What the OS was asked to show, in order (`docs/12` §13).
   *
   * The window decides delivery — focus, preferences — but the *plugin* is the host's, so the
   * choice is recorded here and a check asserts the window did not ask for a banner it should
   * not have. The permission starts granted; a check that wants the refusal path flips it.
   */
  /**
   * What the window asked the host to show, in order (`docs/12` §13).
   *
   * The window decides whether a person is interrupted; the host sends the banner. So the
   * choice is recorded at the call the window makes, and a check asserts the words are the
   * host's own and that one event produced one ask.
   *
   * Note where this is *not* recorded: the notification plugin's JavaScript never reaches its
   * own Rust command — `sendNotification` is `new window.Notification(...)` and the crate
   * installs no such global — so a mock that watched `plugin:notification|notify` would have
   * recorded nothing at all.
   */
  notifications: [],
  /** Every attempt, including the ones that failed, so a check can see a refusal was tried. */
  notifyAttempts: [],
  /** Makes the host's own send fail, which is how the fallback is exercised. */
  notifyFails: false,
  /**
   * Whether the window is being looked at.
   *
   * The window decides whether a notification becomes a banner, so the check has to be able to
   * put the window in either state. Both mechanisms the real app could use are served: the
   * `plugin:window|is_focused` answer, and the two events a listener would receive.
   */
  windowFocused: true,
  transcripts: {},
  approvals: {},
  /** Every mutation the app asked for, in order. */
  mutations: [],
  /** Every URL the window asked the host to open, in order. */
  opened: [],
  /** The thread the window says is on screen (`docs/14` step 14): the idle policy's guard. */
  focusedThread: undefined,
  /** Why the index's last pass failed, when a check wants the footer to have to say so. */
  indexError: null,
};

/** The transcript a resumed thread comes back with. */
window.__world.transcripts["01a0shell0001"] = [
  { role: "user", text: "wire the sidebar", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
  {
    // The spawn the roster's `Wire` row came from: the card's own details name the agent id,
    // which is how the panel joins the two.
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [],
    customType: null,
    jobs: [],
    tool: {
      toolCallId: "call_bg",
      toolName: "task",
      intent: null,
      args: '{"tasks":[{"agent":"scout","task":"Find the sidebar wiring"}]}',
      details: '{"progress":[{"id":"Wire","agent":"scout","status":"running","task":"Find the sidebar wiring"}],"async":{"jobId":"bg_7","state":"running","type":"task"}}',
      output: "Backgrounded as job bg_7; result will be delivered automatically.",
      isError: false,
      finished: true,
    },
  },
  {
    // A real auto-backgrounded bash call, with the measured `details.async` record.
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [],
    customType: null,
    jobs: [],
    tool: {
      toolCallId: "call-job",
      toolName: "bash",
      intent: null,
      args: '{"command":"sleep 25; echo done-sleeping"}',
      details: '{"async":{"jobId":"bg_2","state":"running","type":"bash"}}',
      output: "Backgrounded as job bg_2; result will be delivered automatically.",
      isError: false,
      finished: true,
    },
  },
  {
    // Its delivery: a message of its own, which is what closes the job row.
    role: "custom",
    text: "Background job bg_2 has completed. Resume your work using the result below.\n\ndone-sleeping",
    thinking: null,
    streaming: false,
    tool: null,
    attachments: [],
    customType: "async-result",
    jobs: [{ jobId: "bg_2", kind: "bash", durationMs: 25004, label: null }],
  },
  { role: "assistant", text: "the sidebar lists projects and their sessions", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
  {
    role: "tool", text: "", thinking: null, streaming: false, attachments: [], customType: null, jobs: [],
    tool: { toolCallId: "c1", toolName: "bash", intent: null, args: '{"command":"rg sidebar src"}', result: "src/components/Sidebar.vue:1", status: "done", diff: null },
  },
  {
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [], customType: null, jobs: [],
    tool: {
      toolCallId: "call-todo-1",
      toolName: "todo",
      intent: null,
      args: '{"phases":[{"name":"Build","tasks":[{"content":"Read the workspace tree","status":"in_progress"}]}]}',
      details: "",
      output: "updated",
      isError: false,
      finished: true,
    },
  },
  {
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [], customType: null, jobs: [],
    tool: {
      toolCallId: "call-write-1",
      toolName: "write",
      intent: null,
      args: '{"path":"src/lib.rs","content":"fn main() {}"}',
      details: '{"resolvedPath":"/tmp/omp-shell-app/src/lib.rs","madeExecutable":false}',
      output: "wrote 12 bytes",
      isError: false,
      finished: true,
    },
  },
  {
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [], customType: null, jobs: [],
    tool: {
      toolCallId: "call-edit-1",
      toolName: "edit",
      intent: null,
      args: '{"path":"/tmp/omp-shell-app/src/lib.rs"}',
      details: '{"path":"/tmp/omp-shell-app/src/lib.rs","diff":"@@ -1 +1 @@\\n-fn main() {}\\n+fn main() { println!(\\"hi\\"); }"}',
      output: "edited",
      isError: false,
      finished: true,
    },
  },
  {
    role: "tool",
    text: "",
    thinking: null,
    streaming: false,
    attachments: [], customType: null, jobs: [],
    tool: {
      toolCallId: "call-bash-2",
      toolName: "bash",
      intent: null,
      args: '{"command":"seq 1 200000"}',
      details: '{"meta":{"truncation":{"direction":"middle","truncatedBy":"middle","totalBytes":4194304,"outputBytes":8192,"artifactId":"7f3c"}}}',
      output: "line 1\\nline 2",
      isError: false,
      finished: true,
    },
  },
];
window.__world.transcripts["01a0shell0004"] = [
  { role: "user", text: "hello from elsewhere", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
  { role: "assistant", text: "elsewhere", thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
];

/** One thread's status, in `dto::SessionStatus`'s shape. */
// ---------------------------------------------------------------- settings (`docs/12` §12)
//
// The screen payload the host composes, at the size a check can reason about: every row role
// and every control appears once, a row is gated, a credential is redacted, one row has no
// description (44 of the real 309 do), one row is `live`, and the nav's last entry is the
// escape hatch with no rows of its own.
//
// The keys are real ones with their real dispositions, because a fixture that could not occur
// is a check that proves nothing about the app.
window.__world.settings = {
  screen: {
    catalogVersion: "18.2.6",
    sources: {
      agentDir: "/home/gio/.omp/agent",
      globalFile: { path: "/home/gio/.omp/agent/config.yml", exists: true, error: null, refusals: [] },
      projectFile: {
        path: "/tmp/omp-shell-app/.omp/config.yml",
        exists: true,
        error: null,
        refusals: [{ key: "something.new", reason: "`something.new` is not in this build's settings catalog" }],
      },
      overlays: [{ path: "/tmp/omp-overlay.yml", exists: true, error: null, refusals: [] }],
    },
    drift: { unknownKeys: ["something.new"], missingKeys: [] },
    sections: [
      {
        id: "general",
        title: "General",
        blurb: "Version, layout and where the config lives.",
        navGroup: "app",
        icon: "sliders",
        groups: [
          {
            name: "About",
            rows: [
              {
                role: "static",
                id: "app.version",
                label: "Version",
                description: "This app, with the engine it ships",
                value: "0.1.0",
              },
              {
                role: "action",
                id: "app.config-file",
                label: "Config file",
                description: "/home/gio/.omp/agent/config.yml",
                action: "open-config-file",
                tone: "normal",
              },
            ],
          },
          {
            name: "Power",
            rows: [
              {
                role: "setting",
                key: "power.sleepPrevention",
                label: "Sleep prevention",
                description: "Prevent the system sleeping during active sessions.",
                type: "enum",
                control: "select",
                value: "idle",
                present: true,
                redacted: false,
                choices: [
                  { value: "off", label: "Off" },
                  { value: "idle", label: "Idle" },
                  { value: "display", label: "Display" },
                  { value: "system", label: "System" },
                ],
                origin: "global",
                gated: null,
                restart: "sidecar",
                danger: null,
              },
            ],
          },
        ],
      },
      {
        id: "shortcuts-and-keys",
        title: "Shortcuts & Keys",
        blurb: "How keystrokes and prompt text are read.",
        navGroup: "app",
        icon: "keyboard",
        groups: [
          {
            name: "Input",
            rows: [
              {
                role: "setting",
                key: "steeringMode",
                label: "Steering mode",
                description: "How to process queued messages while the agent is working.",
                type: "enum",
                control: "select",
                value: "one-at-a-time",
                present: true,
                redacted: false,
                choices: [
                  { value: "one-at-a-time", label: "One at a time" },
                  { value: "all", label: "All at once" },
                ],
                origin: "default",
                gated: null,
                restart: "live",
                danger: null,
              },
            ],
          },
        ],
      },
      {
        id: "model-and-providers",
        title: "Model & Providers",
        blurb: "Which models serve these sessions.",
        navGroup: "agent",
        icon: "sparkle",
        groups: [
          {
            name: "Roles & Selection",
            rows: [
              {
                role: "setting",
                key: "modelRoles",
                label: "Model roles",
                description: "The model behind each role.",
                type: "record",
                control: "record",
                value: { default: "commandcode/deepseek-v4.1-flash" },
                present: true,
                redacted: false,
                choices: [],
                origin: "project",
                gated: null,
                restart: "sidecar",
                danger: null,
              },
              {
                role: "setting",
                key: "enabledModels",
                label: "Enabled models",
                // The engine has no description for this key, which is true of 44 curated rows.
                description: "",
                type: "array",
                control: "list",
                value: [],
                present: true,
                redacted: false,
                choices: [],
                origin: "default",
                gated: null,
                restart: "app",
                danger: null,
              },
            ],
          },
          {
            name: "Advisor",
            rows: [
              {
                role: "setting",
                key: "advisor.enabled",
                label: "Advisor",
                description: "Run a second model as an advisor.",
                type: "boolean",
                control: "toggle",
                value: false,
                present: true,
                redacted: false,
                choices: [],
                origin: "default",
                gated: null,
                restart: "sidecar",
                danger: null,
              },
              {
                role: "setting",
                key: "advisor.immuneTurns",
                label: "Immune turns",
                description: "Turns after an advisor note where no new note is emitted.",
                type: "number",
                control: "number",
                value: 3,
                present: true,
                redacted: false,
                choices: [],
                origin: "default",
                gated: { reason: "Needs the advisor turned on.", needs: "advisor.enabled" },
                restart: "sidecar",
                danger: null,
              },
            ],
          },
        ],
      },
      {
        id: "memory",
        title: "Memory",
        blurb: "How the agent remembers.",
        navGroup: "agent",
        icon: "brain",
        groups: [
          {
            name: "Mnemosyne",
            rows: [
              {
                role: "setting",
                key: "mnemopi.embeddingApiKey",
                label: "Embedding API key",
                description: "Optional embedding API key passed to Mnemopi.",
                type: "string",
                control: "secret",
                // The engine holds one it will not print.
                value: null,
                present: true,
                redacted: true,
                choices: [],
                origin: "global",
                gated: null,
                restart: "sidecar",
                danger: null,
              },
            ],
          },
        ],
      },
      {
        id: "raw-config",
        title: "Raw config",
        blurb: "The engine's own files, for what this screen does not surface.",
        navGroup: "system",
        icon: "scroll",
        groups: [],
      },
    ],
  },
  hatch: {
    path: "/home/gio/.omp/agent/config.yml",
    exists: true,
    text: "power:\n  sleepPrevention: idle\nmnemopi:\n  embeddingApiKey: ••••••••\n",
    error: null,
    keys: ["mnemopi.embeddingApiKey", "power.sleepPrevention"],
    refusals: [{ key: "something.new", reason: "`something.new` is not in this build's settings catalog" }],
    backups: [
      { path: "/home/gio/.omp/agent/config.yml.app-backup-1000", at: 1000, text: "power:\n  sleepPrevention: idle\n" },
      { path: "/home/gio/.omp/agent/config.yml.app-backup-900", at: 900, text: "power:\n  sleepPrevention: off\n" },
    ],
  },
  writes: [],
  applied: [],
};

window.__statusFor = (thread) => {
  const session = window.__world.sessions.find((entry) => entry.id === thread);
  const live = window.__world.live.find((entry) => entry.id === thread);
  return {
    binary: "/home/GioViale/.bun/bin/omp",
    workspace: session?.cwd ?? live?.workspace ?? "/tmp/omp-shell-app",
    sidecarPid: 4242,
    approvalMode: "write",
    ready: { protocolVersion: 2, supportedProtocolVersions: [1, 2], negotiatedV2: true, maxFrameBytes: 1048576, maxReassembledFrameBytes: 4194304 },
    control: {
      sessionId: thread,
      sessionName: session?.title ?? null,
      model: window.__activeModel ?? { provider: "commandcode", id: "deepseek-v4.1-flash" },
      thinkingLevel: window.__activeThinkingLevel ?? "high",
      isStreaming: live?.streaming ?? false,
      isCompacting: false,
      messageCount: session?.messageCount ?? 0,
      queuedMessageCount: 0,
      context: { tokens: 15145, contextWindow: 1000000, percent: 1.5 },
      todoPhases: structuredClone(window.__world.todos[thread] ?? []),
      transcriptRows: (window.__world.transcripts[thread] ?? []).length,
      autoCompactionEnabled: true,
    },
    counters: { eventsSeen: 11, framesSeen: 4, eventKinds: 6, unknownEvents: 0, malformedEvents: 0, laggedEvents: 0, uiRequests: 0, blockingUiRequests: 0 },
  };
};

window.__models = {
  fetchedAt: 1,
  options: [
    { provider: "local", id: "tiny", name: "Tiny", contextWindow: 8192, reasoning: false, defaultEffort: "off", efforts: ["off", "low"] },
    { provider: "commandcode", id: "deepseek-v4.1-flash", name: "DeepSeek V4.1 Flash", contextWindow: 1000000, reasoning: true, defaultEffort: "high", efforts: ["off", "low", "medium", "high", "xhigh", "max"] },
    { provider: "llama.cpp", id: "Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp", name: "Qwen 27B Custom", contextWindow: 180000, reasoning: true, defaultEffort: null, efforts: ["low", "medium", "high", "max"] },
  ],
};

window.__favourites = [];

/** Fire a host event at the window, the way the Rust side would. */
window.__fire = (event, payload) => {
  const listeners = window.__calls
    .filter((call) => call.cmd === "plugin:event|listen" && call.args.event === event)
    .map((call) => window.__callbacks[call.args.handler])
    .filter(Boolean);

  window.__fired.push({ event, listeners: listeners.length });
  for (const listener of listeners) {
    listener({ event, id: window.__nextCallback++, payload });
  }

  return listeners.length;
};

window.__TAURI_INTERNALS__ = {
  metadata: {
    currentWindow: { label: "main" },
    currentWebview: { label: "main" },
  },
  transformCallback(callback, once) {
    const id = window.__nextCallback++;
    window.__callbacks[id] = (payload) => {
      if (once) delete window.__callbacks[id];
      callback(payload);
    };
    return id;
  },
  unregisterCallback(id) {
    delete window.__callbacks[id];
  },
  convertFileSrc: (path) => path,
  invoke: async (cmd, args) => {
    const recorded = { cmd, args: args ?? {} };
    window.__calls.push(recorded);
    const world = window.__world;

    try {
      return await dispatch(cmd, args ?? {});
    } catch (error) {
      // A throwing handler is invisible in a call log, and that reads exactly like a
      // command the window never sent.
      recorded.error = String(error);
      throw error;
    }
  },
};

/**
 * Suspend or resume a thread the way the host does it (`docs/14` step 14).
 *
 * Two things change and only two: the catalogue row's `suspended` mark, and whether the thread
 * has a sidecar. Everything else about the row — its title, its dot, its place in the sidebar —
 * has to survive that, which is what the check is for.
 */
window.__suspend = (id, suspended = true) => {
  const row = window.__world.sessions.find((entry) => entry.id === id);
  if (row === undefined) throw new Error(`no such session: ${id}`);
  row.suspended = suspended;
  if (suspended) {
    window.__world.live = window.__world.live.filter((entry) => entry.id !== id);
  }
  window.__fire("threads-updated", structuredClone(window.__world.live));
  return row.suspended;
};

/**
 * Focus or blur the window, answering *every* way a window can learn it.
 *
 * There are three in a Tauri webview — the window plugin's own answer, the two events, and
 * `document.hasFocus()` — and the app is free to pick any of them, so the mock keeps all three
 * in agreement rather than assuming which one it chose.
 */
window.__focus = (focused) => {
  window.__world.windowFocused = focused;
  window.__fire(focused ? "tauri://focus" : "tauri://blur", {});
  return window.__world.windowFocused;
};

Object.defineProperty(document, "hasFocus", {
  configurable: true,
  value: () => window.__world.windowFocused,
});

/** What a hand-edit would change, decided by the text so a check can drive both cases. */
window.__planFor = (text) => {
  const source = String(text ?? "");
  if (source.includes("nonsense")) {
    return {
      changes: [],
      refusals: [
        {
          key: "power.sleepPrevention",
          reason: '`power.sleepPrevention` does not take "nonsense" — one of: off, idle, display, system',
        },
      ],
    };
  }
  if (source.includes("stale")) {
    return {
      changes: [{ key: "power.sleepPrevention", action: "set", before: "idle", after: "display", restart: "sidecar" }],
      refusals: [
        {
          key: "power.sleepPrevention",
          reason: "the file changed while this edit was open: `power.sleepPrevention` now holds display — reopen the escape hatch and make the change again",
        },
      ],
    };
  }
  if (source.includes("dev.autoqa")) {
    return {
      changes: [{ key: "dev.autoqa", action: "set", before: false, after: true, restart: "sidecar" }],
      refusals: [],
    };
  }
  return {
    changes: [{ key: "power.sleepPrevention", action: "set", before: "idle", after: "display", restart: "sidecar" }],
    refusals: [],
  };
};

const dispatch = async (cmd, args) => {
  const world = window.__world;
  {
    switch (cmd) {
      case "thread_todos":
        return structuredClone(world.todos[args.thread] ?? []);
      case "set_todos": {
        // The real engine stores what it is given and echoes it back, so the fixture does
        // exactly that: whatever the panel shows afterwards came from here.
        world.todos[args.thread] = structuredClone(args.phases ?? []);
        world.mutations.push({ cmd, thread: args.thread, phases: structuredClone(args.phases ?? []) });
        return structuredClone(world.todos[args.thread]);
      }
      case "workspace_tree": {
        const session = world.sessions.find((entry) => entry.id === args.thread);
        const root = session?.cwd ?? "/tmp/omp-shell-app";
        const where = args.path ?? root;
        if (where !== root && !where.startsWith(root + "/")) {
          throw new Error(`${where} is outside this session's working directory`);
        }
        const level = world.tree[where];
        if (level === undefined) throw new Error(`${where} could not be read`);
        return structuredClone(level);
      }
      case "read_artifact": {
        const found = world.artifacts[args.id];
        if (found === undefined) {
          throw new Error("the artifact is gone — omp gc archives cold sessions");
        }
        return structuredClone(found);
      }
      case "launch_context":
        // No workspace: the window opens on the empty state, and a click in the sidebar is what
        // starts a thread — which is the path worth checking. The identity is filled in,
        // because the footer renders it and a check should see what a user sees.
        return { workspace: null, user: "gio", version: "0.1.0" };
      case "sessions":
        return structuredClone(world.sessions);
      case "projects":
        return structuredClone(world.projects);
      case "threads":
        return structuredClone(world.live);
      case "open_thread": {
        const resume = args.resume ?? null;
        if (resume !== null) {
          // Already live: the host answers with that thread rather than spawning a second
          // sidecar, and the check asserts exactly that.
          const existing = world.live.find((entry) => entry.id === resume);
          if (existing) return structuredClone(existing);
          const session = world.sessions.find((entry) => entry.id === resume);
          if (!session) throw new Error("that session is no longer in the store");
          const thread = {
            id: session.id,
            workspace: session.cwd,
            title: session.title,
            streaming: false,
            pendingApprovals: 0,
            error: null,
          };
          world.live = [...world.live, thread];
          window.__fire("threads-updated", structuredClone(world.live));
          return structuredClone(thread);
        }
        const created = {
          id: `01a0shellnew${world.live.length + 1}`,
          workspace: args.workspace,
          title: null,
          streaming: false,
          pendingApprovals: 0,
          error: null,
        };
        world.live = [...world.live, created];
        world.transcripts[created.id] = [];
        window.__fire("threads-updated", structuredClone(world.live));
        return structuredClone(created);
      }
      case "close_thread": {
        world.live = world.live.filter((entry) => entry.id !== args.thread);
        window.__fire("threads-updated", structuredClone(world.live));
        return null;
      }
      case "thread_status":
        return window.__statusFor(args.thread);
      case "transcript":
        return structuredClone(world.transcripts[args.thread] ?? []);
      case "ui_requests":
        return structuredClone(world.approvals[args.thread] ?? []);
      case "prompt":
      case "steer":
      case "follow_up":
      case "stop_turn_and_send":
        // Recorded with the thread, so a check can assert the composer spoke to the thread
        // the *window* was showing rather than to whatever was open last.
        world.mutations.push({ cmd, thread: args.thread, message: args.message ?? null });
        if (args.message !== undefined) {
          const rows = world.transcripts[args.thread] ?? [];
          const next = [
            ...rows,
            { role: "user", text: args.message, thinking: null, streaming: false, tool: null, attachments: [], customType: null, jobs: [] },
          ];
          world.transcripts[args.thread] = next;
          window.__fire("session-rows", { thread: args.thread, payload: { from: 0, rows: structuredClone(next) } });
        }
        return null;
      case "stop_turn":
        world.mutations.push({ cmd, thread: args.thread, message: null });
        return null;
      case "models":
        return structuredClone(window.__models);
      case "set_model": {
        window.__activeModel = { provider: args.provider, id: args.modelId };
        return null;
      }
      case "set_thinking_level": {
        window.__activeThinkingLevel = args.level;
        return null;
      }
      case "available_commands":
        return structuredClone(window.__commands);
      case "favourites":
        return window.__favourites;
      case "set_favourites":
        window.__favourites = args.keys;
        return window.__favourites;
      // --- the session flows (`docs/12` §2.3), each mutating the fixture so the checks can
      // assert what the *window* does afterwards: a rename has to survive the catalogue
      // refresh, a pin has to reorder the sidebar, a fork has to change the active id.
      case "rename_thread": {
        const session = world.sessions.find((entry) => entry.id === args.thread);
        if (session) {
          session.title = args.name;
          session.titleSource = "user";
        }
        const live = world.live.find((entry) => entry.id === args.thread);
        if (live) live.title = args.name;
        return null;
      }
      case "branch_targets":
        return [
          { entryId: "entry-1", text: "wire the sidebar" },
          { entryId: "entry-2", text: "and then the transcript" },
        ];
      case "branch_thread": {
        const from = args.thread;
        const source = world.sessions.find((entry) => entry.id === from);
        const forked = {
          id: "01a0shellfork",
          path: source ? source.path.replace(/\.jsonl$/, "-fork.jsonl") : "/tmp/fork.jsonl",
          bucket: source?.bucket ?? "-tmp-omp-shell-app",
          cwd: source?.cwd ?? "/tmp/omp-shell-app",
          title: "forked thread",
          titleSource: "auto",
          parentId: from,
          createdAt: "2026-09-20T11:00:00.000Z",
          modifiedAt: 5000,
          messageCount: 1,
          size: 256,
          firstMessage: "wire the sidebar",
          status: "complete",
          pinned: false,
        };
        world.sessions = [...world.sessions, forked];
        world.transcripts[forked.id] = (world.transcripts[from] ?? []).slice(0, 1);
        // The sidecar stays, the session file is new: the old id stops existing.
        world.live = world.live.filter((entry) => entry.id !== from);
        world.live = [...world.live, { id: forked.id, workspace: forked.cwd, title: forked.title, streaming: false, pendingApprovals: 0, error: null }];
        window.__fire("threads-updated", structuredClone(world.live));
        return structuredClone(world.live.find((entry) => entry.id === forked.id));
      }
      case "handoff_thread":
        world.mutations.push({ cmd, thread: args.thread, message: args.instructions ?? null });
        return null;
      case "export_html":
        return `/tmp/omp-shell-config/exports/omp-session-${args.thread}.html`;
      // The URL door, which is a different door from `open_path`'s on purpose: that one admits
      // paths the app itself owns, this one is where the scheme allowlist lives.
      case "open_external": {
        world.opened.push({ url: args.url ?? null });
        return null;
      }
      case "open_path":
        world.mutations.push({ cmd, path: args.path });
        return null;
      case "delete_session": {
        world.sessions = world.sessions.filter((entry) => entry.id !== args.id);
        world.live = world.live.filter((entry) => entry.id !== args.id);
        window.__fire("threads-updated", structuredClone(world.live));
        return null;
      }
      case "pin_session": {
        const session = world.sessions.find((entry) => entry.id === args.id);
        if (session) session.pinned = !session.pinned;
        world.mutations.push({ cmd, thread: args.thread, id: args.id, pinned: session?.pinned ?? null });
        return null;
      }
      case "search": {
        // A tiny index over the fixture: two hits in one thread, one in another, and one in
        // a thread's *tool* record — which is what makes the jump target interesting.
        const needle = String(args.query ?? "").toLowerCase();
        const index = [
          { thread: "01a0shell0001", kind: "answer", ordinal: 2, text: "the sidebar lists projects and their sessions" },
          { thread: "01a0shell0001", kind: "tool", ordinal: 3, text: 'bash {"command":"rg sidebar src"}' },
          { thread: "01a0shell0004", kind: "prompt", ordinal: 1, text: "hello from elsewhere" },
          { thread: "01a0shell0002", kind: "thinking", ordinal: 2, text: "a fork continues the sidebar work" },
        ];
        if (needle === "") return [];
        return index.filter((row) => row.text.toLowerCase().includes(needle)).slice(0, args.limit ?? 50);
      }
      case "index_status":
        return { indexed: 5, total: 5, running: false, completedAt: 1, error: world.indexError ?? null };
      case "reindex":
        return null;
      case "terminals":
        return structuredClone(world.terminals);
      case "terminal_open": {
        // Minted the way the host mints: the id is the window's key, so a fixture that
        // reused one would let the panel mount two emulators on one shell and pass.
        const id = `pty-${world.terminals.length + 1}`;
        const opened = { id, cwd: args.cwd, running: true, exit: null, pid: 5000 + world.terminals.length };
        world.terminals = [...world.terminals, opened];
        window.__fire("terminals-updated", structuredClone(world.terminals));
        return structuredClone(opened);
      }
      case "terminal_write": {
        world.terminalInput.push({ id: args.id, data: args.data });
        return null;
      }
      case "terminal_resize": {
        world.terminalSizes.push({ id: args.id, cols: args.cols, rows: args.rows });
        return null;
      }
      case "terminal_close": {
        world.terminalClosed.push(args.id);
        world.terminals = world.terminals.filter((entry) => entry.id !== args.id);
        window.__fire("terminals-updated", structuredClone(world.terminals));
        return structuredClone(world.terminals);
      }
      // ---------------------------------------------------------- settings (`docs/12` §12)
      //
      // The stub mirrors the *host's* rules, not just its shapes: the confirmation gate for a
      // dangerous hand-edit is enforced here, so a check that sees the window apply without one
      // is seeing a real failure rather than a fixture that let it through.
      case "settings_screen": {
        world.mutations.push({ cmd, thread: args.thread ?? null });
        return structuredClone(world.settings.screen);
      }
      case "settings_write":
      case "settings_reset": {
        const write = {
          kind: cmd === "settings_write" ? "set" : "reset",
          key: args.key,
          value: args.value ?? null,
          confirmation: args.confirmation ?? null,
        };
        world.settings.writes.push(write);
        world.mutations.push(write);

        const row = world.settings.screen.sections
          .flatMap((section) => section.groups)
          .flatMap((group) => group.rows)
          .find((entry) => entry.role === "setting" && entry.key === args.key);
        if (row) {
          row.value = cmd === "settings_write" ? args.value : null;
          row.present = cmd === "settings_write";
          row.origin = cmd === "settings_write" ? "global" : "default";
        }

        return {
          key: args.key,
          value: cmd === "settings_write" ? args.value : null,
          restart: row?.restart ?? "sidecar",
          message:
            cmd === "settings_write"
              ? `\`${args.key}\` is recorded. Each session reads settings when it starts — restart to use it now.`
              : `\`${args.key}\` is back to the engine's default. Each session reads settings when it starts — restart to use it now.`,
        };
      }
      case "settings_hatch": {
        world.mutations.push({ cmd, thread: args.thread ?? null, scope: args.scope ?? "global" });
        return structuredClone(world.settings.hatch);
      }
      case "settings_hatch_plan": {
        world.mutations.push({ cmd, text: args.text ?? "" });
        return structuredClone(window.__planFor(args.text));
      }
      case "settings_hatch_apply": {
        world.mutations.push({ cmd, text: args.text ?? "", confirmation: args.confirmation ?? null });
        const plan = window.__planFor(args.text);
        const risky = plan.changes.some((change) => change.key === "dev.autoqa");
        if (risky && String(args.confirmation ?? "").trim().toLowerCase() !== "confirm") {
          throw new Error(
            "This change writes dev.autoqa, which the app does not surface because it can send data off the machine.\n  Type `confirm` to apply it anyway.",
          );
        }
        if (plan.refusals.length > 0) {
          return { changes: [], refused: structuredClone(plan.refusals) };
        }
        world.settings.applied.push(structuredClone(plan.changes));
        return {
          changes: plan.changes.map((change) => ({ key: change.key, action: change.action, ok: true, error: null })),
          refused: [],
        };
      }
      case "settings_backup": {
        const path = `/home/gio/.omp/agent/config.yml.app-backup-${world.settings.hatch.backups.length + 1100}`;
        world.settings.hatch.backups.unshift({ path, at: 1100, text: world.settings.hatch.text });
        return path;
      }
      case "settings_restart_sessions": {
        // The fixture's streaming thread is skipped, which is the rule the host keeps.
        return {
          restarted: world.live.filter((entry) => !entry.streaming).map((entry) => entry.id),
          skipped: world.live
            .filter((entry) => entry.streaming)
            .map((entry) => ({ thread: entry.id, reason: "a turn is in flight" })),
        };
      }
      case "plugin:event|listen":
        return 1;
      case "agents": {
        return Object.entries(world.agents).map(([thread, agents]) => ({
          thread,
          agents: structuredClone(agents),
          error: null,
        }));
      }
      case "parked_agents":
        return structuredClone(world.parked[args.thread] ?? []);
      case "agent_messages": {
        const page = world.agentTranscripts[args.thread]?.[args.agent];
        if (!page) throw new Error(`no transcript for ${args.agent}`);
        const from = args.fromByte ?? 0;
        // The engine's own rule: a cursor past the end means the file shrank, and the page
        // restarts at zero rather than reporting an empty delta.
        if (from > page.nextByte) {
          return { agent: args.agent, path: page.path, nextByte: page.nextByte, reset: true, source: page.source, rows: structuredClone(page.rows) };
        }
        const fresh = from === 0 ? page.rows : [];
        return {
          agent: args.agent,
          path: page.path,
          nextByte: page.nextByte,
          reset: false,
          source: page.source,
          rows: structuredClone(fresh),
        };
      }
      case "set_focused_thread": {
        world.focusedThread = args.thread ?? null;
        return null;
      }
      case "plugin:window|is_focused":
        return world.windowFocused;
      case "notify_os": {
        const attempt = { title: args.title ?? null, body: args.body ?? null };
        world.notifyAttempts.push(structuredClone(attempt));
        if (world.notifyFails) throw new Error("the notification daemon is not running");
        world.notifications.push({ via: "host", ...structuredClone(attempt) });
        return null;
      }
      case "broker_processes":
        return structuredClone(world.brokers);
      case "stop_broker_process": {
        const daemon = world.brokers.flatMap((scope) => scope.daemons).find((entry) => entry.name === args.name);
        if (!daemon) throw new Error(`no such process: ${args.name}`);
        daemon.state = "exited";
        daemon.exitedAtMs = Date.now();
        return `Stopped ${args.name}`;
      }
      default:
        return null;
    }
  }
};
