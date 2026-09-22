# OhMyPiApp

A **Tauri desktop application** that wraps the [`omp`](https://omp.sh) (Oh My Pi)
coding agent, so the same engine can be driven from a real GUI instead of the TUI.

This repository is **not a fork of OMP**. It is a separate client that speaks
OMP's public integration protocols. That boundary is deliberate and load-bearing:
upstream can be upgraded without our code changing.

## Status

| Phase | Deliverable | State |
| --- | --- | --- |
| **1. Understand everything** | Exhaustive functionality report of `omp` v18.2.6 | **done** — `docs/00`–`docs/10` |
| 2. First-version specs | What v1 must contain, against UI references | **done and signed off** — `docs/11`–`docs/13` |
| 3. Follow-ups | Build order, risks, v2+ roadmap | **done** — `docs/14-build-plan.md` |

Phase 2/3 decisions are recorded in `docs/12` §16 and `docs/13` §"Sign-off record".

## Build

| Step (`docs/14-build-plan.md`) | State |
| --- | --- |
| 1 `sidecar` | **done** — `Sidecar` in `crates/omp-transport/src/sidecar.rs`: spawn, bounded stderr drain, kill-on-drop, graceful stdin-close shutdown |
| 2 `rpc::frame` | **done** — `FrameDecoder` with v2 `rpc_chunk` reassembly and every documented rejection path |
| 2b sidecar acquisition | **done** — `scripts/fetch-sidecar.ts` pins `v18.2.6`, verifies SHA-256, all 8 targets audited against the release manifest |
| 3 `rpc::client` | **done** — `OmpClient`: `ready` handshake, v2 negotiation, `id` correlation, timeout diagnostics, and the outbound frame-limit check (`rpc.md`: inbound commands are never chunked, so a payload that does not fit is refused with both sizes rather than sent) |
| 4 `rpc::events` | **done** — `SessionEvent`/`MessageDelta`: all 28 event kinds + 13 delta kinds typed, unknown-tolerant, completion rule |
| 4b prompt attachments | **done — transport only** — `commands::prompt` carries `ImageContent[]` in its own field (never folded into the message text); the composer that prepares and attaches them is step 8, and `docs/12` §5.1 fixes which input takes which route |
| 5 `state` reducers + transcript + restore | **done** — `crates/omp-session`: message model, live/restore transcript, control state, `get_messages_page` paging with `session_busy`/`stale_cursor` |
| 0 Tauri shell + Vue/TS/Tailwind frontend | **done — M0 reached** — `src-tauri/` (Rust host) + `frontend/` (Vue 3 + TS + Tailwind 4); launches a sidecar, negotiates v2, shows the `ready` fields, reduces a live turn |
| 9 session browser + flows | **done, verified against a real engine and in WebKit.** A thread's id **is** the engine's session id, so "resume this session" and "this live thread" are one key. The window holds several sessions at once (D5) — one `omp --mode rpc-ui` sidecar each — and every session-scoped command and event is addressed by thread. The sidebar renders the **catalogue** (every session on disk, read by the host's new `omp-store` crate) merged with the **live set** (the threads with a sidecar), grouping by the cwd each session recorded, ordering pinned-first like the engine's own picker, indenting a fork under its parent, and deriving unread from the live set going quiet — none of which the engine has any concept of. Clicking a cold row resumes it by **absolute path**; `+` starts a thread in a project. The row menu (rename, pin, export, fork, handoff, reveal, copy cwd, stop, delete) is frontend-complete and its host half is being built; `docs/12` §2.3 lists what each item is measured to be able to do |
| 7 `rpc::ui` layer + approvals | **responder done — M3 reached** — the dialogs the engine waits on are decoded, stored, answered, and rendered; the production dialog (countdown, per-tool renderers, "always allow") follows with step 6 |
| 6 conversation view | **done** — the patch pipeline (coalesced `{from, rows}`, no read per event), the `docs/12` §3.1 row kinds (right-aligned user turns with attachments, sanitized markdown for answers, collapsed thinking, notice chips) and §3.2's cards (`edit` diff, `write` body, the engine's result text for everything else). §3.2 lists what each later step still owns |
| 8 composer | **done — 8a–8e.** `Composer.vue` owns the draft and the turn controls: idle `Enter` sends, streaming `Enter` steers, `⌥Enter` queues, `⇧Enter` breaks the line, `Esc` stops, and the actions on screen follow the state (empty → `stop`; draft → `steer`, `queue`, `stop and send`). The keyboard map is a pure function with tests, and `stop` refuses a pending approval before aborting because `abort` alone cannot end a parked run (measured — `docs/12` §3.4). **8c** is the chip row and its popovers: `ComposerChips.vue` renders folder, mode, model, effort and the context ring from values the session already reported, each opening one panel through a shared `Popover.vue` (teleported and measured, so the scrolling column cannot clip it). The model picker (`ModelPicker.vue` + `lib/models.ts`) searches, groups by provider and keeps app-owned favourites; the host caches the 1540 KiB catalogue so the picker never waits on it. The Mode popover writes `tools.approvalMode` **and restarts the sidecar on the same session file** — measured: there is no runtime setter, and a resumed thread only shows its history through the pager, which `session::open` now does. The Effort popover marks levels the current model does not list in `thinking.efforts`, measured per row rather than guessed. Attachments are 8d. **8b** is the approval surface: `DialogPanel.vue` renders every blocking dialog — the four wire kinds, a live countdown, cancel — and an approval as the engine's own prompt (`Allow tool: bash` + `Command: …`), with the body for `Content:`. "Always allow" records `tools.approval.<tool>: allow` through the engine's own config writer; measured, that is **not live**, so the dialog says the new sessions stop asking and this one keeps asking until it restarts. The launch passes `--approval-mode write` explicitly — without it the engine's default is `yolo` and nothing ever asks. Measured while building it: `abort` cannot end a run parked on an approval (the dialog is the only way out),  so `stop` refuses pending dialogs first. **8d** is attachments, in the three routes the engine models: bytes the webview holds (a paste, or an image through `+`) go as `prompt{images}`, anything already on disk (a drop, a typed path) goes in as its path for the agent to read, and nothing becomes prompt text — base64 in a message is text tokens, and one screenshot is hundreds of thousands of them. The strip (`AttachmentStrip.vue`) is the app's account of what was attached, not a preview of what the model sees. Two measured facts shape it: **WebKitGTK cannot encode WebP** (`toBlob("image/webp")` succeeds and returns `image/png`), so the encoder's own `blob.type` decides and the fallback is JPEG — PNG only where alpha would be flattened — and the budget is arithmetic on the advertised frame, so a 16.6 MB PNG left as 665 KB and a message that cannot fit is refused before the send with both numbers. Only the composer card accepts a drop. **8e** is the command palette, and it is derived from the draft rather than owning a mode: `/` is the command list, `/security ` that command's subcommands, `/security sc` narrows them, and `/security scan --json` is prose. The engine's own `hint` decides whether `Enter` runs a row or finishes its name — `/model` is dispatched verbatim, `/compact` is not — and `Tab` only ever completes. Measured while building it: the startup `available_commands_update` arrives before a subscriber can attach (so the list is fetched and the push only refreshes), `command_output` is a *frame* carrying one string (so a local command's output needed a new path into the transcript), and a prompt sent right after a local command is refused for a moment while the engine is busy |
| 10 right panel: Todos, Files | **done, verified against a real engine and in WebKit.** The right column is the active thread's plan and its files. Todos come from `get_state.todoPhases` — there is no todo *event* in the engine's stream (`todo_reminder` and `todo_auto_clear` are the only todo kinds), so the host re-reads the state whenever the `todo` tool runs, on the same stale-reducer path the model chip uses. The Files tab has three views and only one of them asks the host for anything: **Changed** is derived from the transcript's own mutating tool calls (each tool names its file in its *result* — `write.resolvedPath`, `edit.path`/`perFileResults[].path`, `ast_edit.fileReplacements[].path`), **Tree** is the host's one-level walk of the thread's cwd, and **Artifacts** is the truncated results in the transcript, read back from the session's artifacts directory (`<session>.jsonl` minus the suffix, `<id>.<tool>.log`) — the engine has **no** RPC command for files or artifact bytes at all, so everything here is measured by reading what the engine wrote. A task's identity is its content, so the jump back to the transcript is a text match over the `todo` tool's own rows, and `set_todos` is memory-only — the panel says so rather than pretending a tick was saved |
| 11 agents panel + background jobs | **done, verified against a real engine and in WebKit.** `Active agents N` in the sidebar opens a global panel of every subagent the app can account for. Three measured facts shape it. **The engine's roster is live-only** — a terminal lifecycle deletes the row, and a fork, a switch or a restart clears the registry — so the frames are the only place a settled agent is still a row, and a poll cannot bring one back; the app folds them in and marks a row the engine stopped listing as *settled, without saying how it ended* rather than inventing a success or a failure. **A transcript outlives the engine that served it**: `get_subagent_messages` pages at a byte cursor (a cursor past the end means the file shrank, and the page restarts at zero), and once the registry is cleared the host reads `<session>.jsonl`'s sibling `<agentId>.jsonl` itself — the same filesystem walk the engine's own Hub does. **Background jobs have no command at all**: no RPC lists, cancels or reads one, because `hub` is a tool the *agent* calls. So the job rows are derived — the card that started a job carries `details.async = {jobId, state, type}`, and its result arrives later as a message with `customType: "async-result"` and the same `jobId` — and the panel says that instead of implying a control. The one real interaction is `omp ps stop <name>`, the engine's only supported path for a broker-owned daemon; steering a subagent is not available to a host, and the panel's footer says so |
| 12 terminal panel (D6) | **done, verified against real shells and in WebKit.** An app-owned pty per tab, running the user's own `$SHELL` in the workspace on screen, drawn by xterm.js in a drawer below the columns. Nothing in it involves the engine — `--mode rpc-ui` sets `PI_NO_PTY=1`, so the agent's `bash` has no terminal and can neither see nor drive this one, which the panel says in as many words. The host reads the pty in 8 KiB reads, posts base64 batches of whatever is already queued (a keystroke's echo goes out at once; a screenful is one event), and lets a `sync_channel(8)` be the flow control, so a shell that outruns the webview blocks on its own pty buffer instead of the host growing. Sizes are clamped rather than refused (a webview that has not been laid out reports `0`), and ending a tab escalates: `SIGHUP`, a 250 ms grace, then `SIGKILL` to the child **and its process group** — measured by starting `sleep` in a tab, closing it, and finding no such process. A tab outlives its shell (the row keeps how it ended, and `stty`-verified resizes keep a full-screen program correct), and a death by signal reports no exit code rather than portable-pty's placeholder `1` |
| 12b the visual pass | **done, verified in WebKit and against the references.** The window is drawn in the language of the owner's screenshots: the palette is *sampled* from them (page `#1f1e33`, rail `#1a1a2a`, card `#1c1c2f`, field `#242443`, selected row `#28283e`, accent `#a397e9` — a card darker than the page it sits on, a field lighter than the card), prose is sans with mono kept for code, paths, ids, counts and the terminal, and the icons are our own (`ui/Icon.vue` over `lib/icons.ts` — the reference's are explicitly not assets for this app). The titlebar is four icon controls and no longer a directory field ("open somewhere new" moved to the sidebar's project `+`, which is where a directory belongs); the sidebar has icon rows, hover `+` per project and an identity footer filled from the launch context; the composer is a single raised card with a leading `+` that grows with the prompt, its chip row beneath it; an empty thread gets the `docs/12` §4 greeting it never had; and every panel, popover, menu and dialog was restyled onto the same surfaces (`docs/12` §0 records the system) |
| 13 settings | **done, verified against a real engine and driven in WebKit.** The screen renders a **generated** catalog: `scripts/gen-settings-catalog.ts` reads the pinned binary's own `config list`, the pinned package's schema and `docs/13-settings-mapping.md`, asserts nine things the three must agree on, and writes `crates/omp-settings/catalog.json` — 505 keys, 309 rows, 196 reached through the raw config. CI re-runs it and fails on any difference, so an engine bump that moves the key set cannot ship a stale screen. Each row states where its value is written (`overlay · project · global · default · unknown`), what a write costs (`live` for the six keys with an RPC setter, `sidecar` for 486, `app` for 13 — in the host's own words, never a shorter claim), and why it is disabled when a `ui.condition` is unmet (visible and disabled, with a link to the key that would change it). **The app never writes the config file**: every change goes through `omp config set|reset`, because the engine's writer holds a native cross-process lock and resolves symlinks to their physical target, while the app keeps what is its to keep — validation before the write, a backup before a hand-edit applies, the quarantine state surfaced. The raw-config escape hatch shows the file with credentials **masked in both panes**, returns a plan before anything is written (a plan with a refusal applies nothing), takes its confirmation from the *host's* refusal rather than a second list of dangerous keys in the UI, and lets a backup be reverted as a plan |
| 14 idle suspension, notifications, packaging | **done, verified against a real engine, in WebKit, and in a built installer.** A sidecar is released after ten quiet minutes and resumed through the same cold-open path any other thread uses — proved live with the pid **gone** (not merely unregistered), its catalogue row reading `suspended: true`, its conversation coming back row for row, and a command answered afterwards, with no orphan left behind. Five guards hold it, and the one the host cannot derive — the thread on screen — the window now says out loud (`set_focused_thread`), which it did not until this step. The four notification triggers (a terminal `agent_end`, a dialog arriving once per id, a failed turn, a background job leaving the broker's list) are reported by the host as facts and delivered by the window as it sees fit: an unread ring, an in-app notice, and an OS banner only when the window is not in front of someone and the thread is not the one on screen. The banner goes through the host, not the notification plugin's JavaScript, which is a bare `new window.Notification(...)` with nothing installing that global. The six extension-chrome ops the app used to drop are decoded and rendered, per thread, with a null clearing status and widget. And the bundle builds: a deb was produced and **opened**, carrying the pinned engine (252,405,216 bytes), the notices, the desktop entry and the icons, with `--verify-staged` in `beforeBuildCommand` so a swapped sidecar cannot be bundled at all |
| 15 the login affordance | **done.** The engine has no `omp login` and its `/login` is TUI-only, so the app hosts the flow instead of reimplementing it: a palette row (`/log-in`) opens a terminal in the workspace on screen and runs `omp auth-broker login`, the credential vault's own OAuth flow. The window never holds a credential. Confirmed against the engine's own `--help` rather than a live login — no browser and no credential exist here. The git surface the same audit found stays open, first in `docs/14` §6's v2 roadmap by your call |

`cargo test` — the unit suites of every crate (301 tests), clippy clean at `-D warnings`;
`bun run --cwd frontend build` — `vue-tsc` type check plus the production bundle; `bun test
--cwd frontend` — the frontend's own suites, from the markdown sanitizer (the one place model
output becomes markup in a webview that can call the host) to the terminal panel's model and the
settings model and the notification rules: 279 tests over 18 files.

`bun scripts/gen-settings-catalog.ts --check` — the settings catalog against the pinned engine
and the mapping document, which CI runs after staging the sidecar. Cargo's ignored suites add
the ones that need a real engine: `cargo test -p omp-settings --test write -- --ignored` (the
write path, in a profile it creates and removes) and `cargo test -p omp-desktop --test
settings_live -- --ignored` (the restart claim, in three assertions, with no credentials),
`idle` (a focused sidecar survives its window, an idle one is released with its pid gone, and the
resume brings the conversation back and answers a command, with no orphan left behind) and
`notifications` (a turn's report belongs to its own thread; a sidecar killed with `kill -9` is
reported as failed with the reader's own sentence).

The window's own behaviour — 62 checks in real WebKit against a mock host, including the shell's
chrome (the titlebar has no directory field, the sidebar's `+` is what opens one, the identity
row says who is here), the greeting on an empty thread, and the settings screen (the nav is the
host's sections, a toggle writes with no confirmation, a `live` row offers no restart where a
`sidecar` row does, a gated row names the key that would free it, a credential has no reveal
path and the escape hatch's text pane shows the mask, a refused plan never reaches the host, and
the typed confirmation is driven by the host's own refusal). The notification group reloads the
window first, because a group about delivery rules should not also be a test of whatever an
earlier check left on screen: it asserts that one event is one ask and it goes through the host
with the host's own sentence, that the thread on screen is never announced, that a banner the
host cannot show still leaves the notice with nothing unhandled, that a notice opens *its*
thread, that an extension's own `notify` shows even for the thread on screen, that a suspended
row keeps its place, says `suspended`, and resumes through the same call any cold row makes, and
that the window tells the host which thread is on screen, and that the login row opens a terminal
running the engine's own credential-vault flow. Run it with the frontend dev server up:
`python3 scripts/window-checks/harness.py scripts/window-checks/window.py`.

The terminal is the one feature with no engine in it (`docs/11` §3.4), so its checks are unit
tests over real processes rather than a live suite: `cargo test -p omp-desktop --lib pty::`
spawns `/bin/sh` on a real pty and asserts the directory it starts in, the size the kernel
reports to it, that output arrives whole and batched, exit codes versus signal deaths, and that
closing a tab — or shutting the app — leaves no process behind, a background job included.

One check launches the app itself, because nothing else in the repository can see that it starts:

| Command | What it proves |
| --- | --- |
| `cargo test -p omp-desktop --test smoke -- --ignored` | the real binary gets through `setup` and is still alive afterwards. It exists because that is exactly what broke: step 14 started two background loops from `setup` with a bare `tokio::spawn`, and `setup` has no Tokio runtime, so the app panicked on launch — "there is no reactor running" — while 301 unit tests, 37 live suites and 62 window checks stayed green, because every one of them has the runtime the app does not. Needs a display (`xvfb-run` when headless); passes **no** workspace argument, so it starts no sidecar and writes nothing to the session store |

Live suites, each `--ignored` (they spawn a real agent; the prompts need credentials):

| Command | What it proves |
| --- | --- |
| `cargo test -p omp-transport --test live_sidecar -- --ignored` | `ready` handshake, the 1.58 MB chunked model catalogue, the command palette, the uncorrelatable-error path, and a real prompt whose streamed sequence contains nothing this build fails to decode |
| `cargo test -p omp-session --test live_session -- --ignored` | the live stream and the restored history **agree** on the same turn; the engine's real `stale_cursor` refusal; a real bash call produces a card with the engine's own result |
| `cargo test -p omp-desktop --test m0 -- --ignored` | the shell opens a session, negotiates v2, streams a turn into the transcript, and **leaves no orphan process** behind; a second test sends a prompt image and asserts it arrives as a row attachment with the prompt text untouched |
| `cargo test -p omp-desktop --test approval -- --ignored` | **the safety boundary**: a denied `bash` call does not execute (checked by the file it would have created) and an approved one does |
| `cargo test -p omp-desktop --test composer -- --ignored` | the composer's turn controls: a message sent mid-stream is accepted and reaches the transcript (before this, `prompt` was rejected while streaming); stop ends a streaming turn; and stop ends a run **parked on an approval**, which `abort` alone cannot |
| `cargo test -p omp-desktop --test approval -- --ignored` (the 8b cases) | the approval contract end to end: exec prompts while read/write do not, a project `.omp/config.yml` policy suppresses the prompt and the call still runs, and the `allow` write the dialog performs is honoured by the next session — with the shared user config restored byte-for-byte afterwards |
| `cargo test -p omp-desktop --test mode_switch -- --ignored` | the Mode popover's whole mechanism: a restart on the same session file shows **exactly** the rows the live session had (the hydration the event stream cannot provide), `yolo` then runs a `bash` call with no dialog, and the recorded mode is what the engine reports back — with the user config byte-identical afterwards |
| `cargo test -p omp-desktop --test threads -- --ignored` | **two sessions live at once**: two ids, two PIDs, one of them streaming while the other is idle; the catalogue finds the second in the store (its cwd, first message and lifecycle); closing one leaves the other running; and a resume by id comes back with both transcript rows — then no sidecar survives |
| `bun scripts/session-parity.ts` | the app's catalogue against **the engine's own listing**, field by field on this machine's real store. It is the check that found a real bug: a small session's lifecycle was being derived from its *first* message instead of its last (`omp-store` had reused the 4 KB prefix as if it were the tail), which disagreed with the engine on 10 of 29 sessions |
| `cargo test -p omp-desktop --test palette -- --ignored` | the palette's two open questions: that a fresh session's list is *populated* (45 rows, four sources, `input.hint` the only thing inside `input`) and cached after the first fetch, and that a local-only command answers `agentInvoked: false` while its `command_output` **frame** becomes a notice row — a path that did not exist before |
| `cargo test -p omp-desktop --test composer -- --ignored an_image_steered` | an attachment on the **steering** path: `docs/rpc.md` gives `images` to `steer` and the composer relies on it, so this proves the engine honours it — one attachment on the steered user row, its bytes absent from the text |
| `cargo test -p omp-desktop --test chip_host -- --ignored` | the catalogue: 601 rows with the shape the picker reads, a warm read that does not refetch, and the three session commands (`set_thinking_level`, `set_auto_compaction`, `set_model`) round-tripping through `get_state` |

### How the UI was verified, and why in three ways

The app's own window cannot be driven reliably on this workstation: synthetic input goes to
whatever holds focus, and focus moves on its own here (a game window took it back mid
sequence), so `ydotool`-style tools need a KWin permission prompt and even a granted one
loses its target between two keystrokes. What worked, in order of how much of the stack each
covers:

1. **The real app, with a virtual keyboard.** `/dev/uinput` is granted to this user, so a
   scripted uinput keyboard drives the focused window with no portal and no prompt. That
   connected a real session (real `omp` child, `negotiated v2`, `maxFrame 1,048,576 B`) and
   confirmed the chip row rendering live values, the Effort popover with its model-specific
   ladder, `Escape` closing it, and `+` opening the native file chooser. Screenshots in
   `/tmp/8d-*.png`.
2. **The real frontend in the real engine, harnessed.** `/tmp/wharness.py` runs a WebKitGTK
   webview — the engine Tauri embeds on Linux — against the vite-served frontend with the
   host stubbed, and drives it from Python: no keyboard, no focus, no interference. This is
   where attachments were measured end to end, canvas included: pass-through for an image
   that fits, the WebP→JPEG fallback for a 14 MB screenshot, PNG kept where alpha mattered,
   the ladder stepping down until the frame fits, base64 exactly 4/3 of the bytes, one chip
   per path after a drop (and **not** two for a path named twice — a bug this pass found and
   fixed), a drag elsewhere attaching nothing, `Esc` clearing the strip, and the refusal
   sentence carrying both numbers. It re-ran the chip popovers in this engine as well.
3. **Chromium, for the one thing WebKitGTK will not show here.** A real `Ctrl+V` against a
   real clipboard: one `DataTransferItem`, `kind: "file"`, `type: "image/png"` — the shape
   the paste handler filters on. Measured alongside it: WebKit's *programmatic* `Paste`
   editing command fires a `paste` event with an **empty** `clipboardData`, so only a
   trusted paste carries data and a synthetic one cannot stand in for the user.

**Step 9 closed with the menu's flows and the search index.** The eight app-owned commands
(`rename_thread`, `branch_targets`, `branch_thread`, `handoff_thread`, `export_html`,
`open_path`, `delete_session`, `pin_session`) are driven against the installed engine in
`src-tauri/tests/flows.rs` — measured there: a fork returns a **new** session id and the old
one stops existing, a handoff writes **no file at all** (artifacts empty before and after,
which is what makes it different from Export), `export_html` lands inside the app's own
directory, and Delete is refused while the thread is live. The cross-thread search (D7) is the
app's own SQLite FTS5 index under the app's config directory: a cold pass over this machine's
real store took **678 ms for 13,595 records** (26 MB of index), an incremental pass over
unchanged files **6 ms**, and a query **under a millisecond** — every term quoted, because raw
input like `foo:bar` or `c++` is an FTS5 syntax error rather than a search.

**Step 12b was the visual pass, and it paid for itself in one finding.** Three browser checks
and one terminal theme were reading things the restyle changed — the `◐` glyph that marked an
in-progress task, the `ring-accent-500` class that marked a flash, a popover entry's lower-case
copy, and four token names the new palette had deleted (that last one had been *silently* falling
back to hard-coded hexes). All four are fixed, and the checks now select a control by what it is
(`data-icon`, `data-todo-mark`, `data-flash`, `data-action`, `data-row`, `data-chip`) rather than
by how it is drawn — a check may name the interface, never the rendering.

**Step 12 built the terminal panel, and it is the one part of the app the engine has no part
in.** Eleven unit checks drive real shells on real ptys in under a second (`pty::`), and seven
more drive the drawer in WebKit. The browser pass found the bug that mattered: **nothing
selected a tab** — the host can publish tabs this window never opened, and a drawer with no
tab selected renders nothing at all (the emulator's host measured 0×0, so no size ever reached
the shell). The harness had a matching bug, and the two looked identical from inside the failing
check: it read the folder chip out of `main button`, which finds the *first* thread's column
while every open thread stays mounted, so it reported another session's directory.

**Step 11 built the agents panel, and moved the window checks into the repo.** They were the
only verification artifact living outside it (`scripts/window-checks/`: the WebKitGTK harness,
the mock host, and the checks). Seven new checks cover the panel — the sidebar's count, a
running row with its counters, a settled row, a nested child, a parked transcript read from
disk, the jump back to the spawning call, the derived job rows and `omp ps stop` — and the pass
found three real bugs: a cost of 1.25 cents shown as `$0.01`, a live child that never indented
(the model read the child's own transcript path as its parent link), and the sidebar's count
disagreeing with the panel's because the panel counted sessions whose engine had gone.

**Step 10 built the right column and re-used the same pass to prove it.** Six new checks drive
the panel in WebKit against a mock host: the plan renders with its counts, marks and a blocked
task's reason; clicking a task reveals and flashes the `todo` card it was written in; the mark
toggles a status and the panel shows *the engine's* answer rather than its own; the Changed
view lists a file written once and edited twice as one row, at twice, relative to the thread's
own workspace, with the engine's diff, and does not list a `bash` write; the tree unfolds one
level at a time and shows sizes; and an artifact listed from a truncated card reads back with
its real size and says what it is showing. The pass found one bug in the *harness* worth
naming because it cost an hour: the prelude already answered to `panel` for the command
palette, so the panel helper was silently overwritten and every "is the panel on screen?" check
was asking about a closed palette — which read exactly like a component that never rendered.
Two toggles sharing one column is also the reason the helper now re-reads the column after
every click instead of assuming what a click did.

**Step 9 re-ran the second pass over the new window**, and it is now the pass that covers
navigation: a click on a cold row calling `open_thread` with that session's id *and its
recorded cwd*, its transcript appearing, its dot going live, typing dispatching `prompt` with
that thread's id, a second thread taking the column while both stay mounted, and a turn ending
elsewhere marking that row unread and never the one on screen. It found a bug that would have
shipped invisible, and it is worth naming because the type checker cannot see it: **Vue's
`defineExpose` unwraps refs**, so a parent reads `view.status`, not `view.status.value` — my
shell wrote the second form for a field the view had never exposed either, so the diagnostics
drawer threw while rendering and simply never appeared whenever a thread was open. Two
lessons, both now in the code: an exposed surface is a *runtime* contract (the shell reads it
with `?.` and the view's `defineExpose` names every field the shell uses), and a `reactive()`
map of component instances is the wrong container for refs (a deep proxy unwraps them; the
registry is `shallowReactive`).

Two more bugs were found by the third pass and by nothing else, which is the argument for
keeping it:

- **`v-model` plus an explicit `@input` on the same native element is a trap.** Vue's
  compiler keeps the explicit handler and drops the model's, so the box filled with text
  and the component never heard about it: **nothing typed in the app would have reached the
  draft.** No unit test and no type check can see it — the template compiles, the types are
  right, and the app is silently read-only. The binding is hand-written now
  (`:value` + one `onInput` that takes both the text and the caret), and the check that
  catches it is "type a command and watch the list narrow".
- **A fixture in the *host's* shape, not the engine's.** The first palette check drove the
  frontend with the engine's raw `input: { hint }` and no `aliases` array; the palette threw,
  and Vue kept rendering the last good list — which looked exactly like "the filter does
  nothing". Stubs stand in for the host, so they have to speak the host's contract. The
  harness now also captures page errors, because a swallowed render error is otherwise
  indistinguishable from a control that simply does nothing.

The host half is covered by the live suites above: the bytes route reaches the engine as
`{type: "image", data, mimeType}` — asserted against the wire object in `bridge.rs` — while
`m0` and `composer` prove a real image arrives on the user row, that it never appears in the
message text, and that `steer` carries one mid-turn.

**The app itself is launched by a check now, and writing that check is how this was found.**
Nothing in the repository had ever started the real binary, and the app had been broken since
step 14 with every test still green: the idle supervisor and the broker watcher were both started
from Tauri's `setup` with a bare `tokio::spawn`, and `setup` runs **without** a Tokio runtime, so
launch died with "there is no reactor running" before a single window was drawn. The tests passed
because they all have the runtime the app does not — the supervisor's own live suite drove it
inside `#[tokio::test]` and never once started the thing that starts it in production. Both loops
now spawn on `tauri::async_runtime` (the runtime the rest of the app runs on, which
`search::scan_in_background` already used), and `src-tauri/tests/smoke.rs` launches the binary and
asserts it stays up. Reverting the one-line fix makes that check fail with the same panic, which is
how the guard itself was verified. The app was then run for real, twice: window, engine sidecar
(`omp --mode rpc-ui --approval-mode write`), 82 sessions in the sidebar, and the chip row reading
the engine's own model and effort.

### Running the app

```bash
bun install --cwd frontend
bun scripts/fetch-sidecar.ts            # required: Tauri validates externalBin at build time
bun run --cwd frontend dev              # dev server on 127.0.0.1:5173
cargo run -p omp-desktop -- /path/to/project
```

A workspace argument opens that project immediately (like `code .`); without one the window
opens idle. Linux needs `webkit2gtk-4.1` and `libsoup-3.0`.

### Repository layout

```
Cargo.toml                  Rust workspace
crates/omp-transport/       the only crate that knows the wire protocol
  src/frame.rs              physical line -> logical frame (v2 chunk reassembly)
  src/protocol.rs           ready handshake, frame classification, command constructors
  src/events.rs             typed session-event decoding (28 kinds + 13 deltas)
  src/sidecar.rs            child process supervision + binary resolution
  src/client.rs             request/response correlation + typed event broadcast
  src/error.rs              one variant per distinguishable failure
  examples/frame_sizes.rs   protocol instrument: per-command sizes and chunk counts
crates/omp-session/         session state: what the app shows
  src/messages.rs           the domain view of a message (roles, blocks, tool results)
  src/transcript.rs         conversation rows from events or restored history, plus
                            the change marker a view re-renders from
  src/control.rs            get_state snapshot + event-driven liveness updates
  src/restore.rs            history paging incl. session_busy / stale_cursor recovery
  src/ui_requests.rs        the dialogs the engine waits on: one answer each, or it hangs
src-tauri/                  the desktop host
  src/dto.rs                the frontend's data contract (hand-written, not derived)
  src/session.rs            one live agent: reducers + the pump that feeds them
  src/dialogs.rs            the pending-dialog store; mutation and announcement are one call
  src/idle.rs               the release policy: a pure decision over a fingerprint, and the supervisor
  src/notify.rs             the four facts the host reports; the window decides who hears about them
  src/bridge.rs             Tauri commands, thin adapters with no business rules
  src/external.rs           handing a URL to the user's browser, under a scheme allowlist
  tauri.conf.json           window, CSP, externalBin, bundle
frontend/                   Vue 3 + TypeScript + Tailwind (Vite)
  src/bridge.ts             the typed wrappers over the Tauri commands and events
  src/App.vue               the shell: status, dialogs, the row list
  src/components/           one component per `docs/12` row kind, plus the tool card
  src/lib/markdown.ts       assistant markdown, sanitized (asserted in markdown.test.ts)
  src/lib/toolView.ts       what a tool card shows, decided in one pure function
docs/                       phases 1-3 (the report, specs, build plan)
docs/reference/             the UI reference images and their provenance
scripts/fetch-sidecar.ts    pinned, digest-verified sidecar acquisition
```

`target/`, `frontend/node_modules`, `frontend/dist` and the staged sidecars are git-ignored;
binaries are fetched, never committed.

### Cutting a build

```sh
bun scripts/fetch-sidecar.ts                          # stage the pinned engine + the notices
bun frontend/node_modules/@tauri-apps/cli/tauri.js build
```

The CLI has to be started from the repository root (from `frontend/` it cannot find
`src-tauri/tauri.conf.json`), and there is no root `package.json`, so `bun run tauri build` does
not exist as a shortcut. `beforeBuildCommand` runs the frontend build and then
`fetch-sidecar.ts --verify-staged`, which re-hashes the staged engine against the pin without the
network — so a bundle cannot be produced from a binary that is not the pinned release. What each
platform produces, what ships inside it, and the secrets signing needs are `docs/15-packaging-and-release.md`.

## Engineering rules (apply to every line we write)

1. **Clean** — no dead code, no speculative abstraction, no TODOs shipped as features.
2. **Maintainable** — someone can change one behavior without reading ten files.
3. **Decoupled** — the app depends on documented OMP surfaces only (`--mode rpc-ui`
   wire protocol, CLI, config files). No importing OMP internals, no patching it.
4. **Easy to extend** — new capability = new module behind an existing interface,
   not an edit to a central switchboard.

## The report

`docs/` holds the phase-1 capability inventory. It is the input to phase 2: you
cannot decide what v1 contains until you know everything the engine can do.

| File | Contents |
| --- | --- |
| `docs/00-report-index.md` | How the report is organised; how to read it |
| `docs/01-capability-matrix.md` | Master list: every capability, its surface, and whether a GUI can express it |
| `docs/02-integration-surfaces.md` | RPC / RPC-UI / ACP / SDK / print mode; host tools & URIs; approval + extension-UI sub-protocols |
| `docs/03-tool-catalog.md` | Every agent tool: parameters, approval tier, gating, output shape |
| `docs/04-cli-and-commands.md` | CLI flags/commands, slash commands, keybindings |
| `docs/05-configuration-and-providers.md` | Settings keys, model roles, providers, auth, approval config |
| `docs/06-sessions-and-orchestration.md` | Sessions, branching, subagents, todos, plan mode, compaction, retry |
| `docs/07-context-and-extensibility.md` | System prompt, rules, skills, hooks, extensions, memory, secrets |
| `docs/08-media-and-device-integrations.md` | TTS/STT, browser, computer use, debugger, SSH, blobs, images |
| `docs/09-platform-services.md` | MCP, LSP, web search, GitHub, collab, stream, export/share, stats |
| `docs/10-runtime-and-tui-internals.md` | Rendering pipeline, themes, natives, provider dialects, telemetry |
| `docs/11-v1-scope.md` | **Phase 2** — locked v1 decisions, capability boundary, sidecar/protocol/build contract |
| `docs/12-v1-ia-and-screens.md` | **Phase 2** — window anatomy, sidebar, composer, popovers, panels, approval dialog, terminal |
| `docs/13-settings-mapping.md` | **Phase 2** — all settings keys mapped into the app's settings nav, for sign-off |
| `docs/14-build-plan.md` | **Phase 3** — build order, milestones, risks, v2+ roadmap |

The design references that `docs/12` was built against are archived in
`docs/reference/` (six screenshots, with provenance and the list of points where the
engine overrules them). They are reference material, not specification.

### Conventions used throughout

- **Version pin**: `omp` **v18.2.6** (installed via `bun install -g`, tag-matched
  upstream source). Everything in the report describes that version.
- **GUI vocabulary** — every capability carries exactly one token:

  | Token | Meaning |
  | --- | --- |
  | `direct` | Maps cleanly to a widget/panel/stream; an RPC command, SDK call, or event exists today |
  | `adapt` | Doable, but needs real design work (TUI-shaped semantics, terminal assumptions, keyboard model) |
  | `internal` | No user-facing UI; affects app behaviour/config only |
  | `external` | Needs an external service, extra binary, hardware, or network account |

- **Citations** are repo-relative (`packages/coding-agent/src/...`, `docs/...`)
  against the pinned upstream commit. Claims that could not be verified are marked
  `[UNVERIFIED]` or `[INFERENCE]`.

## Research scaffolding

`.research/` is **not** part of the deliverable and is git-ignored:

| Path | Purpose |
| --- | --- |
| `.research/oh-my-pi/` | Sparse git checkout of upstream at tag `v18.2.6` (`docs/` + `packages/coding-agent/`), plus the authoritative feature docs |
| `.research/REPORT-CONTRACT.md` | The template and rules every report file follows |
| `.research/scripts/rpc-probe.ts` | Throwaway probe that exercises the RPC-UI protocol against a real `omp` process |

The installed package used as ground truth is
`~/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent` (same v18.2.6).

## Architecture direction (to be finalised in phase 2)

Phase 1 established that the intended integration surface is
**`omp --mode rpc-ui`** — a documented, versioned NDJSON protocol over stdio —
driven from a per-session managed child process, with the app implementing the
host side (event stream, approval dialogs, extension-UI vocabulary, subagent
views). Rationale and the alternatives considered are in
`docs/02-integration-surfaces.md` § *Implications for the desktop wrapper*.

Open questions deferred to phase 2: how `omp` is bundled (standalone compiled
binary vs. requiring the user's install), session/settings management that the
protocol does not expose, diff/transcript rendering, and the permission UX.
