# Oh My Pi — unofficial UI

> **Unofficial.** This is an independent, community-made desktop UI wrapper around
> [`omp`](https://omp.sh) (Oh My Pi), the terminal coding agent. It is **not** affiliated
> with, endorsed by, or supported by the Oh My Pi project. Problems with this window are
> this wrapper's problems, not the engine's.

A desktop client for OMP: the same agent, driven from a real GUI instead of the TUI.
Every open thread is an `omp --mode rpc-ui` child process this app supervises, and the
window renders what the engine streams back.

This repository is **not a fork of OMP**. It speaks the engine's documented surfaces only —
the `--mode rpc-ui` NDJSON protocol over stdio, the `omp` CLI, and the engine's own config
and session files — so upstream can move without our code changing, and no OMP internals
are imported or patched.

It ships the pinned engine binary inside the installers (OMP **v18.2.6**), so no separate
`omp` install is needed. The Rust host lives in `src-tauri/` and `crates/`; the window is
Vue 3 + TypeScript + Tailwind in `frontend/`.

## What you can do in it

| Surface | What it gives you |
| --- | --- |
| **Sessions** | A sidebar merging the engine's on-disk session catalogue with the threads that are live, grouped by the project each recorded, pinned first, forks indented under their parent, unread derived from turns finishing elsewhere. Clicking a cold row resumes it by absolute path. The row menu: rename, pin, export, fork, handoff, reveal, copy cwd, stop, delete |
| **Conversation** | User turns with their attachments, assistant markdown (sanitized), thinking collapsed, notice chips, and a tool card per call — the engine's diff for an edit, the body for a write, its own result text for everything else |
| **Composer** | One card with chips for folder, approval mode, model, reasoning effort and context usage. Attachments by paste, drag-and-drop or a typed path; images go to the engine as image content, files on disk go in as their path — never as base64 in the message text |
| **Approvals** | Every dialog the engine blocks on: the four wire kinds, a live countdown, cancel, and per-tool renderers (`Allow tool: bash` + its `Command:`). "Always allow" writes the engine's own `tools.approval.<tool>` config, and says that new sessions stop asking rather than implying this one changes |
| **Plan and files** | The right column: the active thread's todos straight from engine state, plus its files — changed files derived from the transcript's mutating tool calls with diffs, a one-level tree of the thread's directory, and truncated results read back from the session's artifacts |
| **Agents and jobs** | A panel of every subagent the app can account for (running, settled, nested, transcripts read back from disk, jump to the spawning call) plus background-job rows, with `omp ps stop <name>` as the one control the engine actually supports |
| **Terminal** | An app-owned pty per tab in a drawer below the columns, running your own `$SHELL` in the workspace on screen. It is yours alone: `--mode rpc-ui` leaves the agent's own `bash` with no terminal, so it can neither see nor drive it |
| **Settings** | A generated catalogue of the engine's own config keys (505 keys), each row stating where the value is written and what a write costs. Every change goes through `omp config set`/`reset`; there is also a raw-config view with credentials masked and a plan-and-confirm path for hand edits |
| **Search** | Cross-thread full-text search over the app's own index, landing on the matching row |
| **Login** | `/log-in` opens a terminal running the engine's own credential-vault flow (`omp auth-broker login`). The window never holds a credential |
| **Notifications** | Unread rings, in-app notices and OS banners for a finished turn, a dialog waiting, a failed turn and a finished background job. Idle sidecars are released after ten quiet minutes and resumed through the same path any cold session uses |

## Requirements

- 64-bit Linux, macOS 11 or newer (Apple silicon), or Windows 10/11.
- Provider credentials for OMP. The app has no account system of its own; it hosts the
  engine's login (see *Login* above).
- Linux: WebKitGTK 4.1 and GTK 3 — the deb and rpm declare them, an AppImage expects them
  from the host.
- Windows: WebView2, which the installer bootstraps (so installation needs a network
  connection).
- The engine's session store, config and credentials are the ones the TUI uses; this app
  reads and writes the same files, so sessions started in either place show up in both.

## Install and run

Installers are attached to the releases of this repository: `deb`, `rpm` and `AppImage` for
Linux, `dmg`/`.app` for macOS, NSIS `.exe` and `.msi` for Windows. Builds are unsigned
unless the release was cut with signing credentials, so Windows SmartScreen and macOS
Gatekeeper will warn.

Launch it, optionally with a directory to open immediately (like `code .`); without an
argument the window opens idle and you pick a project from the sidebar's `+`:

```bash
omp-desktop /path/to/project
```

The app finds its engine in this order: **`OMP_BIN`** if set, then an `omp` binary **beside
the app's own executable** (what the installers ship), then **`omp` on your `PATH`**.

## Using it

1. **Open a project** — the `+` next to a project in the sidebar, or the directory you
   passed on the command line. A thread is a session in that directory.
2. **Ask for something** — type in the composer and press Enter. The chips under it tell
   you which folder, mode, model and effort the turn will run with, and how much of the
   context window is left; each one opens its own popover.
3. **Answer the dialogs** — when the engine wants permission it stops and waits for you.
   The window shows the call it wants to make; `Allow`, `Always allow` or `Deny`.
4. **Watch the work** — the right column holds the plan and every file the thread touched,
   the sidebar marks threads that finished while you were elsewhere, and the terminal
   drawer is there if you want a shell in the same directory.

| Keys | |
| --- | --- |
| `Enter` | Send when idle; **steer** into the running turn while it streams |
| `⌥Enter` | Queue the message behind the running turn |
| `⇧Enter` | Insert a line break |
| `Esc` | Stop the running turn (a pending approval is refused first, since `abort` alone cannot end a parked run); while idle, clear the draft and its attachments |
| `/` | Command palette, derived from the draft: `/` lists commands, `/security ` its subcommands, `/security sc` narrows them. `Tab` completes, `Enter` runs a row when the engine says it is a command |
| `⌘/Ctrl K` | Search across threads |
| `⌘/Ctrl N` | New thread in the project on screen |
| `⌘/Ctrl ,` | Settings |

There is no native menu bar by design; the titlebar carries the window's own controls, and
the panels on either side are resizable by dragging their edge (double-click resets).

## Status and scope

Version 0.1.0 — first usable version; expect rough edges. The wrapper targets the wire
protocol of OMP v18.2.6 and is verified against a real engine on Linux. There is no
auto-updater, and macOS/Windows installers are built but have not been exercised on those
platforms yet.

## Attribution

Oh My Pi (`omp`) is a separate project by its own authors, distributed under the MIT
licence. The installers bundle its binary along with the third-party attribution the
engine's dependencies require (`THIRD-PARTY-NOTICES.txt`). This repository's own code
declares MIT in its manifests, though no `LICENSE` file has been added yet.
