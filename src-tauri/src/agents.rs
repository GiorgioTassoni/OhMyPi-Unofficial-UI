//! The agents panel's engine-free surface (`docs/12` §9).
//!
//! Three things the panel lists, and only two of them come from the engine:
//!
//! * **what is running now** — `set_subagent_subscription` frames folded into
//!   `omp_session::AgentRoster`, corrected by `get_subagents` (`docs/12` §9);
//! * **what a subagent is doing** — the same frames, and its transcript by byte cursor
//!   (`omp_session::read_transcript` when the engine will not serve it);
//! * **what this session has spawned, ever** — this module, from disk.
//!
//! The third one has to be a filesystem walk, because the engine's own registry is
//! *live-only*: a terminal lifecycle deletes the entry, and a fork, a resume or a sidecar
//! restart starts from an empty registry with no replay. What survives is the file the
//! subagent itself wrote, and the engine's Hub reads exactly this directory to rebuild its
//! parked rows (`registry/persisted-agents.ts`).
//!
//! # The layout, as measured against a real session
//!
//! A session's artifacts live in a directory named after the session file minus its
//! `.jsonl` suffix — the same directory `panel.rs` reads spilled tool results from — and a
//! subagent's own transcript is `<agentId>.jsonl` inside it. A *nested* subagent (an agent
//! that spawned its own) nests one level: `<agentId>/<agentId>.<childId>.jsonl`. Both
//! levels are read here, because the engine reads both.
//!
//! The stem is the agent id, which is the id the parent's own transcript records: a `task`
//! tool result carries `details.progress[].id`, and the frames carry the same value. So a
//! row here joins to a card there by string equality, with nothing inferred.
//!
//! Two naming rules are the engine's and are honoured rather than invented:
//! `<id>.jsonl` only (the same directory holds `.md` yielded output, `.json` structured
//! sides, `.patch` worktree diffs and `<n>.<tool>.log` spilled results — none of them a
//! transcript), a name containing `.bak` is skipped, and `__advisor.jsonl` /
//! `__advisor.<slug>.jsonl` are the advisor's transcript rather than a peer agent's
//! (`isAdvisorTranscriptName`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use omp_store::listing;
use serde_json::Value;

use crate::dto::{BrokerDaemon, BrokerScope, ParkedAgent, SubagentFileChange};
use crate::panel;

#[derive(Clone)]
struct SubagentTranscriptCacheEntry {
    mtime: Option<SystemTime>,
    len: u64,
    agent_id: String,
    agent_name: String,
    changes: Vec<SubagentFileChange>,
}

fn subagent_transcript_cache() -> &'static Mutex<HashMap<PathBuf, SubagentTranscriptCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, SubagentTranscriptCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
pub fn clear_subagent_transcript_cache() {
    if let Ok(mut guard) = subagent_transcript_cache().lock() {
        guard.clear();
    }
}

/// How many entries one level of the scan will look at.
///
/// The same bound the workspace tree uses, and for the same reason: this runs on a
/// blocking worker at a panel's request, and a directory with a million files in it must
/// not turn into a window that will not open.
const MAX_ENTRIES: usize = 2000;

/// Maximum number of recently active subagent transcripts to scan for file mutations
/// when no specific agent ID is requested. Caps disk I/O on large historical threads.
pub const MAX_SCANNED_SUBAGENT_DIFFS: usize = 5;

/// The advisor's transcript file name, and the prefix a named one carries.
const ADVISOR: &str = "__advisor";
const JSONL_SUFFIX: &str = ".jsonl";

/// Every subagent transcript this session's artifacts directory holds.
///
/// Ordered by the agent id the engine uses, which is also the order the parent's cards
/// appear in only when the batch dispatched them that way — so the panel sorts for display
/// rather than pretending this order means anything.
pub fn parked(session_file: &str) -> Result<Vec<ParkedAgent>, String> {
    let directory = panel::artifacts_dir(session_file)?;

    Ok(scan_directory(&directory, 0))
}

/// The parked row for one agent id, if this session has a transcript for it.
///
/// The read path resolves an id by asking what is actually there rather than by building a
/// path from it: the engine nests a child's transcript one directory down
/// (`<parentId>/<parentId>.<childId>.jsonl`, and the child's *id* is that whole stem), so
/// the same id can live at either level. Reading the directory to find it also means no
/// caller-supplied string is ever used as a path component, which is the property the
/// alternative would have had to be careful about.
pub fn parked_by_id(session_file: &str, agent: &str) -> Result<ParkedAgent, String> {
    parked(session_file)?
        .into_iter()
        .find(|row| row.id == agent)
        .ok_or_else(|| {
            format!(
                "this session has no transcript for `{agent}`: the engine never reported \
                 that agent for it, and no file named after it is beside the session"
            )
        })
}

/// Added and removed lines in a unified diff, headers aside.
pub fn diff_line_counts(diff: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            continue;
        }
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            removed += 1;
        }
    }
    (added, removed)
}

fn is_mutating(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write" | "edit" | "ast_edit" | "apply_patch" | "memory_edit"
    )
}

/// A successful preview is not a mutation. Explicit no-op results are not either.
fn confirmed_change(details: Option<&Value>) -> bool {
    let Some(d) = details else { return true };
    if d.get("applied").and_then(Value::as_bool) == Some(false)
        || d.get("totalReplacements").and_then(Value::as_u64) == Some(0)
    {
        return false;
    }
    if let (Some(old), Some(new)) = (
        d.get("oldText").and_then(Value::as_str),
        d.get("newText").and_then(Value::as_str),
    ) {
        if old == new {
            return false;
        }
    }
    d.get("op").and_then(Value::as_str) != Some("update")
        || d.get("diff")
            .and_then(Value::as_str)
            .is_none_or(|diff| diff_line_counts(diff) != (0, 0))
}

fn transcript_cwd(path: &Path) -> Option<PathBuf> {
    use std::io::BufRead;
    let file = fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines().take(5).flatten() {
        let header: Value = serde_json::from_str(&line).ok()?;
        if header.get("type").and_then(Value::as_str) == Some("session") {
            return header.get("cwd").and_then(Value::as_str).map(PathBuf::from);
        }
    }
    None
}

fn external_scratch(path: &str, cwd: Option<&Path>) -> bool {
    let Some(cwd) = cwd else { return false };
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    let temporary = normalized.starts_with(std::env::temp_dir())
        || normalized.starts_with("/tmp")
        || normalized.starts_with("/var/tmp");
    temporary && !normalized.starts_with(cwd)
}

fn extract_touched_paths(tool_name: &str, details: Option<&Value>, args: &Value) -> Vec<String> {
    if let Some(details) = details {
        if tool_name == "ast_edit" {
            if let Some(replacements) = details.get("fileReplacements").and_then(Value::as_array) {
                let paths: Vec<String> = replacements
                    .iter()
                    .filter_map(|r| {
                        r.get("path")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
                    .collect();
                if !paths.is_empty() {
                    return paths;
                }
            }
            if let Some(files) = details.get("files").and_then(Value::as_array) {
                let paths: Vec<String> = files
                    .iter()
                    .filter_map(|f| f.as_str().map(ToString::to_string))
                    .collect();
                if !paths.is_empty() {
                    return paths;
                }
            }
        }

        let single = details
            .get("resolvedPath")
            .and_then(Value::as_str)
            .or_else(|| details.get("path").and_then(Value::as_str));

        if let Some(single_path) = single {
            let moved = details.get("sourcePath").and_then(Value::as_str);
            if let Some(moved_path) = moved {
                if moved_path != single_path {
                    return vec![single_path.to_string(), moved_path.to_string()];
                }
            }
            return vec![single_path.to_string()];
        }

        if let Some(per_file) = details.get("perFileResults").and_then(Value::as_array) {
            let paths: Vec<String> = per_file
                .iter()
                .filter_map(|r| {
                    r.get("path")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .collect();
            if !paths.is_empty() {
                return paths;
            }
        }
    }

    for key in &["path", "file_path"] {
        if let Some(p) = args.get(*key).and_then(Value::as_str) {
            if !p.is_empty() {
                return vec![p.to_string()];
            }
        }
    }

    Vec::new()
}

fn extract_diff(details: Option<&Value>, args: &Value) -> Option<String> {
    let diff = details
        .and_then(|d| d.get("diff").and_then(Value::as_str))
        .or_else(|| args.get("diff").and_then(Value::as_str));
    diff.filter(|d| !d.is_empty()).map(ToString::to_string)
}

/// Parse the session header in the JSONL to discover the agent's name if written.
/// Avoids returning the generic tool/type name `task`.
fn resolve_agent_name(path: &Path, default_id: &str) -> String {
    let fallback = if default_id.is_empty() || default_id == "task" {
        String::new()
    } else {
        default_id.to_string()
    };
    let Ok(file) = fs::File::open(path) else {
        return fallback;
    };
    use std::io::BufRead;
    let reader = std::io::BufReader::new(file);
    for line in reader.lines().take(5) {
        let Ok(line) = line else { break };
        if let Ok(entry) = serde_json::from_str::<Value>(&line) {
            if entry.get("type").and_then(Value::as_str) == Some("session") {
                if let Some(agent_name) = entry.get("agentName").and_then(Value::as_str) {
                    if !agent_name.is_empty() && agent_name != "task" {
                        return agent_name.to_string();
                    }
                }
            }
        }
    }
    fallback
}

/// Scan a single subagent transcript for all file mutations.
pub fn scan_subagent_file_changes(
    path: &Path,
    agent_id: &str,
    agent_name: &str,
) -> Vec<SubagentFileChange> {
    let (mtime, file_len) = match fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => return Vec::new(),
    };

    if let Ok(guard) = subagent_transcript_cache().lock() {
        if let Some(entry) = guard.get(path) {
            if entry.len == file_len
                && entry.mtime == mtime
                && entry.agent_id == agent_id
                && entry.agent_name == agent_name
            {
                return entry.changes.clone();
            }
        }
    }

    let Ok(page) = omp_session::read_transcript(path, 0) else {
        return Vec::new();
    };

    let mut transcript = omp_session::Transcript::default();
    transcript.restore(&page.messages);
    let cwd = transcript_cwd(path);

    struct PathAccumulator {
        kind: &'static str,
        touches: usize,
        diffs: Vec<String>,
        lines_added: usize,
        lines_removed: usize,
    }

    let mut map: std::collections::BTreeMap<String, PathAccumulator> =
        std::collections::BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for row in transcript.rows() {
        let omp_session::Row::Tool(card) = row else {
            continue;
        };
        if !card.is_finished() {
            continue;
        }
        let omp_session::ToolOutcome::Done(ref result) = card.outcome else {
            continue;
        };
        if result.is_error {
            continue;
        }
        // Resolve reports the applied result separately from the original preview.
        let (tool_name, details) = if card.tool_name == "resolve" {
            let Some(d) = result.details.as_ref() else {
                continue;
            };
            if d.get("action").and_then(Value::as_str) != Some("apply") {
                continue;
            }
            let Some(source) = d.get("sourceToolName").and_then(Value::as_str) else {
                continue;
            };
            (source, d.get("sourceResultDetails"))
        } else {
            (card.tool_name.as_str(), result.details.as_ref())
        };
        if !is_mutating(tool_name) || !confirmed_change(details) {
            continue;
        }

        let is_write = tool_name == "write";
        let touched = extract_touched_paths(tool_name, details, &card.args)
            .into_iter()
            .filter(|p| !external_scratch(p, cwd.as_deref()))
            .collect::<Vec<_>>();
        if touched.is_empty() {
            continue;
        }

        let diff = if is_write {
            None
        } else {
            extract_diff(details, &card.args)
        };

        let (added, removed) = if let Some(ref d) = diff {
            diff_line_counts(d)
        } else {
            (0, 0)
        };

        for file_path in touched {
            if let Some(acc) = map.get_mut(&file_path) {
                acc.touches += 1;
                if is_write {
                    acc.kind = "written";
                }
                if let Some(ref d) = diff {
                    acc.diffs.push(d.clone());
                    acc.lines_added += added;
                    acc.lines_removed += removed;
                }
            } else {
                order.push(file_path.clone());
                map.insert(
                    file_path,
                    PathAccumulator {
                        kind: if is_write { "written" } else { "edited" },
                        touches: 1,
                        diffs: diff.clone().map(|d| vec![d]).unwrap_or_default(),
                        lines_added: added,
                        lines_removed: removed,
                    },
                );
            }
        }
    }

    let changes: Vec<SubagentFileChange> = order
        .into_iter()
        .filter_map(|path| {
            let acc = map.remove(&path)?;
            let diff = if acc.diffs.is_empty() {
                None
            } else {
                Some(acc.diffs.join("\n"))
            };
            let final_name = if agent_name.is_empty() || agent_name == "task" {
                if agent_id.is_empty() || agent_id == "task" {
                    String::new()
                } else {
                    agent_id.to_string()
                }
            } else {
                agent_name.to_string()
            };
            Some(SubagentFileChange {
                agent_id: agent_id.to_string(),
                agent_name: final_name,
                path,
                kind: acc.kind.to_string(),
                diff,
                lines_added: acc.lines_added,
                lines_removed: acc.lines_removed,
                touches: acc.touches,
            })
        })
        .collect();

    if let Ok(mut guard) = subagent_transcript_cache().lock() {
        if guard.len() > 1000 {
            guard.clear();
        }
        guard.insert(
            path.to_path_buf(),
            SubagentTranscriptCacheEntry {
                mtime,
                len: file_len,
                agent_id: agent_id.to_string(),
                agent_name: agent_name.to_string(),
                changes: changes.clone(),
            },
        );
    }

    changes
}

/// Every file touched by subagents for this session.
///
/// If `agent_id` is supplied, only that subagent's changes are returned.
/// Otherwise, only the 5 most recently active subagents are scanned to keep
/// disk I/O and UI responsiveness instantaneous with large sessions.
pub fn subagent_file_changes(
    session_file: &str,
    agent_id: Option<&str>,
) -> Result<Vec<SubagentFileChange>, String> {
    let mut parked_agents = parked(session_file)?;
    let mut changes = Vec::new();

    if let Some(requested_id) = agent_id {
        if let Some(agent) = parked_agents.iter().find(|a| a.id == requested_id) {
            if !agent.advisor {
                let agent_name = resolve_agent_name(Path::new(&agent.path), &agent.id);
                changes.extend(scan_subagent_file_changes(
                    Path::new(&agent.path),
                    &agent.id,
                    &agent_name,
                ));
            }
        }
        return Ok(changes);
    }

    // Sort newest modified first and limit scan to top recent subagents.
    parked_agents.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    for agent in parked_agents
        .into_iter()
        .filter(|a| !a.advisor)
        .take(MAX_SCANNED_SUBAGENT_DIFFS)
    {
        let agent_name = resolve_agent_name(Path::new(&agent.path), &agent.id);
        let agent_changes =
            scan_subagent_file_changes(Path::new(&agent.path), &agent.id, &agent_name);
        changes.extend(agent_changes);
    }

    Ok(changes)
}

/// Read one level of the artifacts tree.
///
/// Recursion is one level deep because that is as deep as the engine nests a child
/// transcript, and its own scan recurses the same way (`persisted-agents.ts`).
fn scan_directory(directory: &Path, depth: usize) -> Vec<ParkedAgent> {
    let Ok(entries) = fs::read_dir(directory) else {
        // A session that never spilled anything has no artifacts directory at all, which is
        // a session with no parked agents rather than an error to report.
        return Vec::new();
    };

    let mut agents = Vec::new();
    let mut children: Vec<(String, PathBuf)> = Vec::new();

    for entry in entries.flatten().take(MAX_ENTRIES) {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();

        if path.is_dir() {
            // A nested agent's own directory, named after its id.
            if depth == 0 {
                children.push((name, path));
            }
            continue;
        }

        if !name.ends_with(JSONL_SUFFIX) || name.contains(".bak") {
            continue;
        }

        let id = name
            .strip_suffix(JSONL_SUFFIX)
            .unwrap_or_default()
            .to_string();
        let (advisor, slug) = advisor_of(&id);

        let size = entry.metadata().map(|meta| meta.len()).unwrap_or_default();
        let modified_ms = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_millis() as u64)
            .unwrap_or_default();

        // The header is read through the catalogue's own decoder rather than a second one
        // here: it is the same file, the same two-line prefix, and a session summary that
        // disagreed with the sidebar's about the same session would be a bug with nowhere
        // to point.
        let summary = listing::scan(&path, "");
        let cwd = summary
            .as_ref()
            .map(|summary| summary.cwd.clone())
            .unwrap_or_default();
        // The header's own `parentSession`, verbatim: a session *file* for a nested child,
        // absent for an agent this session spawned directly. The scan knows the nesting
        // from the directory it is in, but the panel wants the link the file states, not a
        // second one this code inferred.
        let parent_session = summary.as_ref().and_then(|summary| summary.parent.clone());

        agents.push(ParkedAgent {
            id,
            path: path.display().to_string(),
            bytes: size,
            modified_ms,
            cwd,
            parent: parent_session,
            advisor,
            advisor_slug: slug,
        });
    }

    for (_, directory) in children {
        agents.extend(scan_directory(&directory, depth + 1));
    }

    agents
}

/// The engine's advisor naming rule (`isAdvisorTranscriptName`).
///
/// `__advisor.jsonl` is the default advisor; `__advisor.<slug>.jsonl` is a named one. Both
/// are transcripts of a second opinion rather than of a peer agent, which is why the panel
/// labels them instead of counting them as subagents.
fn advisor_of(id: &str) -> (bool, Option<String>) {
    if id == ADVISOR {
        return (true, None);
    }

    match id.strip_prefix(&format!("{ADVISOR}.")) {
        Some(slug) if !slug.is_empty() => (true, Some(slug.to_string())),
        _ => (false, None),
    }
}

/// How long `omp ps` gets before the panel reports that it did not answer.
///
/// Generous for a command that lists files and reads pid files, and short enough that a
/// wedged broker (the very thing this view exists to find) cannot hold a panel open.
const PS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// OMP's default daemon-broker idle grace is three seconds. Give brokers used by this app
/// a little longer to finish their own graceful shutdown before an AppImage unmounts the
/// executable they were launched from.
const APPIMAGE_BROKER_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Let project brokers used by this app exit normally before an AppImage is unmounted.
///
/// The broker is deliberately not killed here: it may own work another OMP client is
/// using. `omp ps --json --dir` only observes an existing broker; closing that brief
/// inspection connection starts the broker's normal idle-shutdown timer. Persistent work
/// is never stopped; if it keeps the broker alive past the bound, the broker is left alone.
#[cfg(target_os = "linux")]
pub async fn wait_for_appimage_brokers(workspaces: &[String]) {
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }

    let mut pids = std::collections::HashSet::new();
    for cwd in workspaces {
        match project_broker_for_shutdown(cwd).await {
            Ok(Some(scope)) => {
                if let Some(pid) = scope.broker_pid {
                    pids.insert(pid);
                }
            }
            Ok(None) => {}
            Err(error) => eprintln!("[omp-desktop] could not inspect broker during exit: {error}"),
        }
    }

    if pids.is_empty() {
        return;
    }

    let deadline = Instant::now() + APPIMAGE_BROKER_EXIT_TIMEOUT;
    while pids.iter().any(|pid| daemon_broker_is_running(*pid)) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(75)).await;
    }

    let still_running: Vec<_> = pids
        .into_iter()
        .filter(|pid| daemon_broker_is_running(*pid))
        .collect();
    if !still_running.is_empty() {
        eprintln!(
            "[omp-desktop] OMP broker(s) {still_running:?} did not exit before the AppImage shutdown timeout; leaving them running"
        );
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn wait_for_appimage_brokers(_workspaces: &[String]) {}

#[cfg(target_os = "linux")]
async fn project_broker_for_shutdown(cwd: &str) -> Result<Option<BrokerScope>, String> {
    let binary = omp_transport::sidecar::resolve_omp_binary();
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("ps")
        .arg("--json")
        .arg("--dir")
        .arg(cwd)
        .current_dir(cwd)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(Duration::from_secs(2), command.output())
        .await
        .map_err(|_| "`omp ps` did not answer during shutdown".to_string())?
        .map_err(|error| format!("could not run `omp ps` during shutdown: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "`omp ps` exited with {} during shutdown: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let expected = std::fs::canonicalize(cwd).unwrap_or_else(|_| Path::new(cwd).to_path_buf());
    Ok(decode_scopes(&output.stdout)?.into_iter().find(|scope| {
        scope.kind == "project"
            && std::fs::canonicalize(&scope.project_dir)
                .unwrap_or_else(|_| Path::new(&scope.project_dir).to_path_buf())
                == expected
    }))
}

#[cfg(target_os = "linux")]
fn daemon_broker_is_running(pid: u64) -> bool {
    let command_line = match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(command_line) => command_line,
        Err(_) => return false,
    };
    command_line
        .split(|byte| *byte == 0)
        .any(|argument| argument == b"__omp_worker_daemon_broker")
}

/// Every broker-scoped process the engine knows about, across all projects.
///
/// `omp ps` is the only supported way to see *or stop* broker-owned daemons
/// (`cli/ps-cli.ts`), and `--all` is the flag that asks for every scope rather than this
/// directory's. Listing never starts a broker — measured, and the reason this is safe to
/// call on a panel's open: the action verbs (`stop`, `logs`, `restart`) revive one, the
/// list does not.
///
/// The binary is the app's pinned one rather than whatever `omp` is on `PATH`, for the same
/// reason the sidecars are: a second engine version's daemons are not this app's to manage.
pub async fn broker_scopes(cwd: &str) -> Result<Vec<BrokerScope>, String> {
    let binary = omp_transport::sidecar::resolve_omp_binary();

    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("ps")
        .arg("--json")
        .arg("--all")
        .current_dir(cwd)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(PS_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("`omp ps` did not answer within {PS_TIMEOUT:?}"))?
        .map_err(|error| format!("could not run `omp ps`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`omp ps` exited with {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    decode_scopes(&output.stdout)
}

/// Stop one broker-owned process through the engine (`omp ps stop <name>`).
///
/// The name is checked before it becomes an argument, and the check is about flags rather
/// than paths: `omp ps stop --json` would be a different command entirely, so a name that
/// starts with `-`, or carries whitespace, is refused instead of passed on. Everything else
/// is the engine's own vocabulary — `vite`, `omp.lsp.mux`, `omp.browser.headless` — and
/// validating it further would start rejecting daemons this app has never heard of.
///
/// The engine's own output comes back rather than a code the panel would have to translate:
/// `omp ps` says which daemon it stopped and what it was, and that sentence is the answer.
pub async fn stop_broker(cwd: &str, name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.starts_with('-')
        || name
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!("`{name}` is not a process name"));
    }

    let binary = omp_transport::sidecar::resolve_omp_binary();

    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("ps")
        .arg("stop")
        .arg(name)
        .current_dir(cwd)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(PS_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("`omp ps stop {name}` did not answer within {PS_TIMEOUT:?}"))?
        .map_err(|error| format!("could not run `omp ps stop {name}`: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(if stderr.is_empty() {
            format!(
                "`omp ps stop {name}` exited with {}",
                output.status.code().unwrap_or(-1)
            )
        } else {
            stderr
        });
    }

    Ok(if stdout.is_empty() { stderr } else { stdout })
}

/// Decode `omp ps --json`'s stdout.
///
/// A free function so the shape is asserted against a capture rather than against the
/// engine: the output is a JSON array, and an engine that answered something else (a
/// prefix line, a version banner) must be reported rather than parsed into an empty list
/// that reads as "no processes".
pub fn decode_scopes(stdout: &[u8]) -> Result<Vec<BrokerScope>, String> {
    let scopes: Vec<serde_json::Value> = serde_json::from_slice(stdout).map_err(|error| {
        format!("`omp ps --json` did not print the JSON this app reads: {error}")
    })?;

    Ok(scopes.iter().map(decode_scope).collect())
}

fn decode_scope(raw: &serde_json::Value) -> BrokerScope {
    BrokerScope {
        kind: text(raw, "kind"),
        project_dir: text(raw, "projectDir"),
        runtime_dir: text(raw, "runtimeDir"),
        broker_pid: raw.get("brokerPid").and_then(serde_json::Value::as_u64),
        daemons: raw
            .get("daemons")
            .and_then(serde_json::Value::as_array)
            .map(|daemons| daemons.iter().map(decode_daemon).collect())
            .unwrap_or_default(),
    }
}

fn decode_daemon(raw: &serde_json::Value) -> BrokerDaemon {
    BrokerDaemon {
        name: text(raw, "name"),
        state: text(raw, "state"),
        owner: raw
            .get("owner")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        command: text(raw, "command"),
        cwd: text(raw, "cwd"),
        supervised: raw
            .get("supervised")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        persist: raw
            .get("persist")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        detached: raw
            .get("detached")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        restart_count: raw
            .get("restartCount")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        exit_code: raw.get("exitCode").and_then(serde_json::Value::as_i64),
        output_bytes: raw
            .get("outputBytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        started_at_ms: raw
            .get("startedAt")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        exited_at_ms: raw.get("exitedAt").and_then(serde_json::Value::as_u64),
    }
}

fn text(raw: &serde_json::Value, key: &str) -> String {
    raw.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session path whose artifacts directory the test owns.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("omp-agents-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();

        root
    }

    /// One subagent transcript, with the two entries every session file starts with.
    fn write_transcript(path: &Path, cwd: &str, parent: Option<&str>) {
        fs::create_dir_all(path.parent().expect("a parent directory")).expect("a scratch dir");
        let parent_field = parent
            .map(|parent| format!(",\"parentSession\":\"{parent}\""))
            .unwrap_or_default();
        fs::write(
            path,
            format!(
                "{{\"type\":\"title\",\"title\":\"\"}}\n\
                 {{\"type\":\"session\",\"version\":3,\"id\":\"01a0bfbb-cec7-730d-a4f5-ba88a0e2d14d\",\
                 \"timestamp\":\"2026-09-20T16:52:31.559Z\",\"cwd\":\"{cwd}\"{parent_field}}}\n\
                 {{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"content\":[]}}}}\n"
            ),
        )
        .expect("a scratch transcript");
    }

    #[test]
    fn a_session_with_no_artifacts_directory_has_no_parked_agents() {
        let root = scratch("empty");
        let session = root.join("session.jsonl");
        fs::create_dir_all(&root).expect("a scratch dir");
        fs::write(&session, "").expect("a scratch session");

        let agents = parked(session.to_str().expect("utf-8")).expect("no agents");

        assert!(agents.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    /// The measured layout: `<session>/<id>.jsonl`, plus the files that share that
    /// directory and are *not* transcripts.
    #[test]
    fn parked_lists_transcripts_and_skips_the_adjacent_artifacts() {
        let root = scratch("layout");
        let artifacts = root.join("session");
        fs::create_dir_all(&artifacts).expect("a scratch dir");
        fs::write(root.join("session.jsonl"), "").expect("a scratch session");

        write_transcript(&artifacts.join("Scout.jsonl"), "/tmp/project", None);
        write_transcript(
            &artifacts.join("Reviewer.jsonl"),
            "/tmp/project",
            Some("/tmp/sessions/session.jsonl"),
        );
        // The neighbours a real directory holds, none of which is a transcript.
        fs::write(artifacts.join("Scout.md"), "# yielded output").expect("a scratch file");
        fs::write(artifacts.join("Scout.json"), "{}").expect("a scratch file");
        fs::write(artifacts.join("Scout.patch"), "diff").expect("a scratch file");
        fs::write(artifacts.join("1170.bash.log"), "spilled").expect("a scratch file");
        fs::write(artifacts.join("Scout.jsonl.bak"), "{}").expect("a scratch file");
        fs::write(artifacts.join("__advisor.jsonl"), "").expect("a scratch file");

        let mut agents =
            parked(root.join("session.jsonl").to_str().expect("utf-8")).expect("the parked rows");
        agents.sort_by(|left, right| left.id.cmp(&right.id));

        let ids: Vec<&str> = agents.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(ids, vec!["Reviewer", "Scout", "__advisor"]);

        // The header is the source for where it ran and who it belonged to, and the
        // catalogue's decoder read it — not a second one here.
        let scout = &agents[1];
        assert_eq!(scout.cwd, "/tmp/project");
        assert_eq!(
            scout.parent, None,
            "a top-level subagent's header names no parent, and the panel must not be told one"
        );
        assert_eq!(
            agents[0].parent.as_deref(),
            Some("/tmp/sessions/session.jsonl")
        );
        assert!(scout.bytes > 0);
        assert!(!scout.advisor);

        let advisor = &agents[2];
        assert!(advisor.advisor);
        assert_eq!(advisor.advisor_slug, None);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_nested_child_is_found_beside_its_parent() {
        let root = scratch("nested");
        let artifacts = root.join("session");
        fs::create_dir_all(&artifacts).expect("a scratch dir");
        fs::write(root.join("session.jsonl"), "").expect("a scratch session");

        write_transcript(&artifacts.join("Parent.jsonl"), "/tmp/project", None);
        write_transcript(
            &artifacts.join("Parent").join("Parent.Child.jsonl"),
            "/tmp/project",
            Some("/tmp/sessions/session/Parent.jsonl"),
        );

        let mut agents =
            parked(root.join("session.jsonl").to_str().expect("utf-8")).expect("the parked rows");
        agents.sort_by(|left, right| left.id.cmp(&right.id));

        // The engine's id for a nested child is the whole file stem, not the part after
        // the dot: `entry.name.slice(0, -6)` (`persisted-agents.ts`). The parent's own card
        // names the child that way, so this is the string the panel joins on.
        let ids: Vec<&str> = agents.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(ids, vec!["Parent", "Parent.Child"]);
        // The child's own directory is not itself a transcript, and its header names the
        // parent's file — which is what the panel needs to nest the row.
        assert_eq!(
            agents[1].parent.as_deref(),
            Some("/tmp/sessions/session/Parent.jsonl")
        );

        fs::remove_dir_all(&root).ok();
    }

    /// The shape, asserted against a capture rather than against a guess.
    ///
    /// This is `omp ps --json --all` stdout from this machine, verbatim, with two daemons
    /// kept and the browser's kilobyte-long `command` line dropped (it is the same shape as
    /// `vite`'s). Everything else is the engine's own output — including `owner`, which is
    /// the field that lets the panel attribute a daemon to the session that started it.
    const PS_CAPTURE: &str = r#"[
      {
        "kind": "project",
        "projectDir": "/home/GioViale/Projects/Websites_or_webapp/OhMyPiApp",
        "runtimeDir": "/home/GioViale/.omp/run/daemons/011f02c4f8183799",
        "daemons": [
          {
            "name": "omp.lsp.mux",
            "id": "7cbdd50a-f311-40ba-a78d-65630ecd5c8e",
            "state": "exited",
            "createdAt": 1789909185609,
            "startedAt": 1789909185609,
            "readyAt": 1789909186041,
            "exitedAt": 1789910086197,
            "exitCode": 0,
            "restartCount": 0,
            "outputBytes": 87,
            "readyMatch": "omp lsp mux listening on /home/GioViale/.omp/run/daemons/011f02c4f8183799/lsp-mux.sock",
            "persist": false,
            "detached": false,
            "command": "/home/GioViale/.bun/bin/bun /home/GioViale/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js __omp_worker_lsp_mux",
            "cwd": "/home/GioViale/Projects/Websites_or_webapp/OhMyPiApp",
            "supervised": false
          },
          {
            "name": "vite",
            "id": "a42e33df-5695-4586-abfb-173ed814f32e",
            "state": "exited",
            "createdAt": 1789916421544,
            "startedAt": 1789916421544,
            "readyAt": 1789916422154,
            "exitedAt": 1789929078950,
            "exitCode": 143,
            "restartCount": 0,
            "outputBytes": 7898,
            "owner": "01a0bb7d-7260-757d-b395-39e22d9655d3",
            "readyMatch": "Local:   http",
            "persist": false,
            "detached": false,
            "command": "bun run dev",
            "cwd": "/home/GioViale/Projects/Websites_or_webapp/OhMyPiApp/frontend",
            "supervised": false
          }
        ]
      }
    ]"#;

    #[test]
    fn a_ps_capture_decodes_into_scopes_and_daemons() {
        let scopes = decode_scopes(PS_CAPTURE.as_bytes()).expect("the engine's own output");

        assert_eq!(scopes.len(), 1);
        let scope = &scopes[0];
        assert_eq!(scope.kind, "project");
        assert_eq!(
            scope.project_dir,
            "/home/GioViale/Projects/Websites_or_webapp/OhMyPiApp"
        );
        assert_eq!(
            scope.broker_pid, None,
            "no live broker, and the capture has no pid"
        );

        assert_eq!(scope.daemons.len(), 2);
        let vite = &scope.daemons[1];
        assert_eq!(vite.name, "vite");
        assert_eq!(vite.state, "exited");
        assert_eq!(vite.command, "bun run dev");
        assert_eq!(vite.exit_code, Some(143));
        assert_eq!(vite.output_bytes, 7898);
        assert_eq!(
            vite.owner.as_deref(),
            Some("01a0bb7d-7260-757d-b395-39e22d9655d3"),
            "the session that started it is what lets the panel attribute it"
        );
        assert_eq!(vite.started_at_ms, 1789916421544);
        assert_eq!(vite.exited_at_ms, Some(1789929078950));
        assert!(!vite.supervised);
        assert_eq!(scope.daemons[0].owner, None);
    }

    /// Anything that is not the JSON this reads is reported rather than parsed into an
    /// empty list, which would read as "no processes running".
    #[test]
    fn output_that_is_not_json_is_an_error() {
        assert!(decode_scopes(
            b"omp v18.2.6
"
        )
        .is_err());
        assert!(decode_scopes(b"").is_err());
    }

    #[test]
    fn an_advisor_slug_is_kept() {
        let (advisor, slug) = advisor_of("__advisor.reviewer");

        assert!(advisor);
        assert_eq!(slug.as_deref(), Some("reviewer"));
        assert_eq!(advisor_of("advisor"), (false, None));
    }

    /// An id the session has no transcript for is a sentence, not an empty pane: the
    /// caller asked for something specific and the file is not there.
    #[test]
    fn an_unknown_id_is_reported_by_name() {
        let root = scratch("unknown");
        let artifacts = root.join("session");
        fs::create_dir_all(&artifacts).expect("a scratch dir");
        fs::write(root.join("session.jsonl"), "").expect("a scratch session");
        write_transcript(&artifacts.join("Scout.jsonl"), "/tmp/project", None);

        let session = root.join("session.jsonl").to_string_lossy().to_string();
        let found = parked_by_id(&session, "Scout").expect("the row");
        assert_eq!(found.id, "Scout");

        let error = parked_by_id(&session, "../secrets").expect_err("no such agent");
        assert!(
            error.contains("../secrets"),
            "the id is named back: {error}"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn diff_line_counts_works() {
        let diff =
            "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,4 @@\n context\n-removed\n+added 1\n+added 2\n";
        let (added, removed) = diff_line_counts(diff);
        assert_eq!(added, 2);
        assert_eq!(removed, 1);
    }

    #[test]
    fn scanning_subagent_transcript_extracts_mutating_tool_changes() {
        let root = scratch("subagent_changes");
        let transcript_path = root.join("coder.jsonl");
        fs::create_dir_all(&root).expect("a scratch dir");

        let content = r#"{"type":"session","version":3,"id":"sub-1","agent":"coder","timestamp":"2026-09-20T16:52:31.559Z","cwd":"/tmp"}
{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_1","name":"write","arguments":{"path":"/tmp/created.ts","content":"hello"}}]}}
{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","toolName":"write","content":"written","isError":false,"details":{"resolvedPath":"/tmp/created.ts"}}}
{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_2","name":"edit","arguments":{"path":"/tmp/edited.ts"}}]}}
{"type":"message","message":{"role":"toolResult","toolCallId":"call_2","toolName":"edit","content":"ok","isError":false,"details":{"path":"/tmp/edited.ts","diff":"@@ -1 +1,2 @@\n-old\n+new 1\n+new 2"}}}
"#;
        fs::write(&transcript_path, content).expect("write transcript");

        let changes = scan_subagent_file_changes(&transcript_path, "sub-1", "coder");
        assert_eq!(changes.len(), 2);

        assert_eq!(changes[0].path, "/tmp/created.ts");
        assert_eq!(changes[0].kind, "written");
        assert_eq!(changes[0].touches, 1);
        assert_eq!(changes[0].diff, None);
        assert_eq!(changes[0].lines_added, 0);
        assert_eq!(changes[0].lines_removed, 0);

        assert_eq!(changes[1].path, "/tmp/edited.ts");
        assert_eq!(changes[1].kind, "edited");
        assert_eq!(changes[1].touches, 1);
        assert!(changes[1].diff.is_some());
        assert_eq!(changes[1].lines_added, 2);
        assert_eq!(changes[1].lines_removed, 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn review_counts_applied_changes_not_previews_noops_or_external_scratch_files() {
        let root = scratch("confirmed-changes");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("agent.jsonl");
        let cases = vec![
            (
                "ast_edit",
                serde_json::json!({"path":"/work/app/preview.py"}),
                serde_json::json!({"applied":false,"fileReplacements":[{"path":"/work/app/preview.py","count":1}]}),
            ),
            (
                "edit",
                serde_json::json!({"path":"/work/app/same.py"}),
                serde_json::json!({"path":"/work/app/same.py","oldText":"same","newText":"same","diff":" context"}),
            ),
            (
                "edit",
                serde_json::json!({"path":"/work/app/empty.py"}),
                serde_json::json!({"op":"update","path":"/work/app/empty.py","diff":""}),
            ),
            (
                "write",
                serde_json::json!({"path":"/tmp/review-helper.py"}),
                serde_json::json!({"resolvedPath":"/tmp/review-helper.py"}),
            ),
            (
                "write",
                serde_json::json!({"path":"created.py"}),
                serde_json::json!({"resolvedPath":"/work/app/created.py"}),
            ),
            (
                "edit",
                serde_json::json!({"path":"../website/docs.md"}),
                serde_json::json!({"path":"../website/docs.md","diff":"-old\n+new"}),
            ),
            (
                "resolve",
                serde_json::json!({"action":"apply"}),
                serde_json::json!({"action":"apply","sourceToolName":"ast_edit","sourceResultDetails":{"applied":true,"totalReplacements":1,"fileReplacements":[{"path":"/work/app/applied.py","count":1}]}}),
            ),
            (
                "resolve",
                serde_json::json!({"action":"reject"}),
                serde_json::json!({"action":"reject","sourceToolName":"ast_edit"}),
            ),
        ];
        let mut lines = vec![
            serde_json::json!({"type":"session","version":3,"id":"test","cwd":"/work/app"})
                .to_string(),
        ];
        for (i, (name, args, details)) in cases.into_iter().enumerate() {
            let id = format!("call-{i}");
            lines.push(serde_json::json!({"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":id,"name":name,"arguments":args}]}}).to_string());
            lines.push(serde_json::json!({"type":"message","message":{"role":"toolResult","toolCallId":id,"toolName":name,"content":"ok","isError":false,"details":details}}).to_string());
        }
        fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
        let changes = scan_subagent_file_changes(&path, "coder", "Coder");
        assert_eq!(
            changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
            vec![
                "/work/app/created.py",
                "../website/docs.md",
                "/work/app/applied.py"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_agent_name_prefers_default_id_over_task_tool_type() {
        let root = scratch("resolve_agent_name");
        let transcript_path = root.join("PlaylistDownloadExtraction.jsonl");
        fs::create_dir_all(&root).expect("a scratch dir");

        let content = r#"{"type":"session","version":3,"id":"uuid-123","agent":"task","timestamp":"2026-09-20T16:52:31.559Z","cwd":"/tmp"}
{"type":"message","message":{"role":"assistant","content":[]}}
"#;
        fs::write(&transcript_path, content).expect("write transcript");

        let resolved = resolve_agent_name(&transcript_path, "PlaylistDownloadExtraction");
        assert_eq!(resolved, "PlaylistDownloadExtraction");

        let resolved_task = resolve_agent_name(&transcript_path, "task");
        assert_eq!(resolved_task, "");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn subagent_transcript_cache_reuses_result_and_invalidates_on_file_change() {
        let root = scratch("cache_test");
        let transcript_path = root.join("worker.jsonl");
        fs::create_dir_all(&root).expect("a scratch dir");
        clear_subagent_transcript_cache();

        let initial_content = r#"{"type":"session","version":3,"id":"sub-c","agent":"worker","timestamp":"2026-09-20T16:52:31.559Z","cwd":"/tmp"}
{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"write","arguments":{"path":"/tmp/initial.ts","content":"v1"}}]}}
{"type":"message","message":{"role":"toolResult","toolCallId":"c1","toolName":"write","content":"ok","isError":false,"details":{"resolvedPath":"/tmp/initial.ts"}}}
"#;
        fs::write(&transcript_path, initial_content).expect("initial write");

        let first = scan_subagent_file_changes(&transcript_path, "sub-c", "worker");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].path, "/tmp/initial.ts");

        // Second call should return cached result
        let second = scan_subagent_file_changes(&transcript_path, "sub-c", "worker");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].path, "/tmp/initial.ts");

        // Appending to the transcript should invalidate cache and return new results
        let updated_content = format!(
            "{initial_content}{}\n{}\n",
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c2","name":"write","arguments":{"path":"/tmp/second.ts","content":"v2"}}]}}"#,
            r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c2","toolName":"write","content":"ok","isError":false,"details":{"resolvedPath":"/tmp/second.ts"}}}"#
        );
        fs::write(&transcript_path, updated_content).expect("append write");

        let third = scan_subagent_file_changes(&transcript_path, "sub-c", "worker");
        assert_eq!(third.len(), 2);
        assert_eq!(third[1].path, "/tmp/second.ts");

        fs::remove_dir_all(&root).ok();
    }
}
