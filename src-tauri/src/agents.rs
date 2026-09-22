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

use std::fs;
use std::path::{Path, PathBuf};

use omp_store::listing;

use crate::dto::{BrokerDaemon, BrokerScope, ParkedAgent};
use crate::panel;

/// How many entries one level of the scan will look at.
///
/// The same bound the workspace tree uses, and for the same reason: this runs on a
/// blocking worker at a panel's request, and a directory with a million files in it must
/// not turn into a window that will not open.
const MAX_ENTRIES: usize = 2000;

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
}
