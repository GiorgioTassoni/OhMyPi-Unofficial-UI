//! The right panel's data (`docs/12` §8): the Todos tab, the workspace tree, and the
//! engine's spilled artifacts.
//!
//! Two of the panel's three sources are things the engine has no command for, which is
//! why this module exists at all:
//!
//! * **The tree.** The whole `RpcCommand` union is silent on the filesystem — measured at
//!   v18.2.6, nothing file-shaped is in it and the dispatcher's `default` arm refuses what
//!   it does not know — so one directory level is read here.
//! * **The artifacts.** `artifact://<id>` is a plain file beside the session's own `.jsonl`
//!   (the engine's rule: the jsonl path minus that suffix is the artifacts directory, and
//!   the id names `<id>.<sanitizedTool>.log` inside it). Nothing on the wire reads one, so
//!   the host resolves and reads it.
//!
//! Everything here opens a path under exactly two roots — the thread's workspace, and the
//! session's own artifacts directory — and the two rules that enforce that are the two
//! that could otherwise open the user's whole disk:
//!
//! * the tree resolves both the root and the requested path with `canonicalize`, so a
//!   symlink or a `..` segment cannot walk out of the workspace;
//! * an artifact id must be **purely digits**, so a crafted id (`../../etc/passwd`) cannot
//!   name a file outside the session's directory even though the scan is a prefix match.
//!
//! The todos have a rule of their own, and it is the opposite of cleverness: the engine
//! stores what it is given, so the host does **not** re-decide what a plan may contain —
//! with one exception, an empty list, which is refused rather than sent because replacing
//! a real plan with nothing is the destructive accident this command can cause.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{fs, io::Read};

use omp_session::TodoPhase;
use serde_json::{json, Value};

use crate::dto::{
    ArtifactSnapshot, TodoPhaseInput, TodoPhaseSnapshot, TodoTaskSnapshot, WorkspaceEntry,
};

/// The most entries one level of the tree answers with.
///
/// A cap rather than a threshold: the walk stops once it has this many entries instead of
/// reading a directory into memory to then show a slice of it, because the cost that has
/// to be bounded is the *read*, and a directory with a million names in it is exactly the
/// case a window must not stall on. What the cap therefore fixes is the set shown — the
/// first entries the filesystem hands over — and the panel is a browser, so a level of
/// 2 000 rows is a level nobody reads past either way.
pub const MAX_ENTRIES: usize = 2_000;

/// The most of an artifact a read hands to the window.
///
/// The spilled file is the *complete* result, and a complete result can be megabytes (the
/// engine spills exactly when the card was cut short). 256 KiB is enough to read the head
/// of any log and small enough that one click cannot turn into a 20 MB IPC payload.
pub const MAX_ARTIFACT_BYTES: u64 = 256 * 1024;

/// The panel's own shape for a plan, from the engine's decoded one.
pub fn phases(phases: &[TodoPhase]) -> Vec<TodoPhaseSnapshot> {
    phases
        .iter()
        .map(|phase| TodoPhaseSnapshot {
            name: phase.name.clone(),
            tasks: phase
                .tasks
                .iter()
                .map(|task| TodoTaskSnapshot {
                    content: task.content.clone(),
                    status: task.status.clone(),
                    // A blank blocker is drawn as absent: the engine keeps an empty
                    // string if it is given one, and a "waiting for" line with nothing on
                    // it is a line the panel should not draw.
                    blocker: task
                        .blocker
                        .clone()
                        .filter(|blocker| !blocker.trim().is_empty()),
                })
                .collect(),
        })
        .collect()
}

/// The plan to send, or the reason an empty one is refused.
///
/// The refusal is the one place this module second-guesses a caller, and it is deliberate:
/// `set_todos` is a *replace*, the engine persists nothing, and the plan that is replaced
/// cannot be recovered from the wire (the durable copy is the `todo` tool calls in the
/// transcript). An accidental empty send therefore destroys a real plan until the thread is
/// resumed, so the panel's own affordance has to mean it — one empty phase is how it says
/// so.
pub fn outgoing(phases: &[TodoPhaseInput]) -> Result<Value, String> {
    if phases.is_empty() {
        return Err(
            "refusing to replace the plan with nothing: send one empty phase to clear it"
                .to_string(),
        );
    }

    Ok(Value::Array(
        phases
            .iter()
            .map(|phase| {
                json!({
                    "name": phase.name,
                    "tasks": phase
                        .tasks
                        .iter()
                        .map(|task| {
                            // The engine's own projection keeps `blocker` only when it has
                            // one (`todo-tracker.ts`'s `#clonePhases`), so an absent field
                            // is what it sends and what it should receive: a `null` here
                            // would come back as a blocker the engine holds, and a task
                            // that reads as "waiting for nothing" in the panel.
                            let mut wire = json!({
                                "content": task.content,
                                "status": task.status,
                            });
                            if let Some(blocker) = &task.blocker {
                                wire["blocker"] = Value::String(blocker.clone());
                            }
                            wire
                        })
                        .collect::<Vec<Value>>(),
                })
            })
            .collect(),
    ))
}

/// One directory level, as the Tree view draws it.
///
/// `path` is absolute (and inside `cwd`), or relative to it, or absent for `cwd` itself.
/// The check is made on resolved paths rather than on the strings that arrived: `..`
/// segments and symlinks are precisely how a path that reads as inside reaches somewhere
/// else, and the panel passes back the paths this function returned, so the two agree on
/// what "inside" means.
pub fn tree(cwd: &str, path: Option<&str>) -> Result<Vec<WorkspaceEntry>, String> {
    let root = fs::canonicalize(cwd)
        .map_err(|error| format!("the thread's workspace `{cwd}` cannot be read: {error}"))?;

    let directory = match path {
        None => root.clone(),
        Some(given) => {
            let requested = Path::new(given);
            let requested = if requested.is_absolute() {
                requested.to_path_buf()
            } else {
                root.join(requested)
            };

            let resolved = fs::canonicalize(&requested)
                .map_err(|error| format!("`{}` cannot be read: {error}", requested.display()))?;

            if !resolved.starts_with(&root) {
                return Err(format!(
                    "`{}` is outside the thread's workspace, so it will not be listed",
                    resolved.display()
                ));
            }

            resolved
        }
    };

    let listing = fs::read_dir(&directory)
        .map_err(|error| format!("`{}` cannot be listed: {error}", directory.display()))?;

    let mut entries: Vec<WorkspaceEntry> = Vec::new();
    for entry in listing {
        if entries.len() >= MAX_ENTRIES {
            break;
        }

        // An entry the filesystem will not describe is skipped rather than fatal: a broken
        // symlink is an ordinary thing to have in a workspace, and a browser that refused
        // the whole level because of one would be refusing a directory the user can see.
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };

        entries.push(WorkspaceEntry {
            // Lossy because the wire is a string. A name that is not UTF-8 is still
            // listed; the panel simply cannot pass that one path back for another level.
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.to_string_lossy().into_owned(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified_at: modified_at(&metadata),
        });
    }

    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            // Case-insensitively equal names need a tiebreak, or the order would depend on
            // the order `read_dir` happened to hand them over in.
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(entries)
}

/// One spilled tool result, read back.
///
/// `directory` is the session's artifacts directory; `id` is the number the tool card
/// named. A missing file — or a missing directory, which is the same answer — is an `Err`
/// with a sentence fit to show, because an artifact genuinely disappears: `omp gc`
/// archives a cold session's artifacts, and 30 days later a card's chip points at nothing.
pub fn artifact(directory: &Path, id: &str) -> Result<ArtifactSnapshot, String> {
    let path = artifact_file(directory, id)?;

    // The size comes from the filesystem rather than from what was read, because it is
    // half the answer: the card showed a truncated result and this is how much the engine
    // actually kept.
    let bytes = fs::metadata(&path)
        .map_err(|error| format!("artifact `{id}` cannot be measured: {error}"))?
        .len();

    let file = fs::File::open(&path)
        .map_err(|error| format!("artifact `{id}` cannot be opened: {error}"))?;

    let mut buffer = Vec::with_capacity(bytes.min(MAX_ARTIFACT_BYTES) as usize);
    file.take(MAX_ARTIFACT_BYTES)
        .read_to_end(&mut buffer)
        .map_err(|error| format!("artifact `{id}` cannot be read: {error}"))?;

    Ok(ArtifactSnapshot {
        id: id.to_string(),
        path: path.to_string_lossy().into_owned(),
        bytes,
        // Read against the file's own size, so the flag is "there is more" and not an
        // arithmetic on the cap: a file that is exactly the cap is complete.
        truncated: bytes > buffer.len() as u64,
        // Lossy on purpose: a spilled log is bytes, not text, and a tool that emitted one
        // invalid sequence must not make its artifact unreadable.
        text: String::from_utf8_lossy(&buffer).into_owned(),
    })
}

/// Whether an id is one the engine could have minted: digits, and nothing else.
///
/// This is the whole of the traversal defence, and it is a gate rather than a filter
/// because the scan below is a *prefix* match: `../secret` would match nothing by luck,
/// but `0.` inside a crafted name would, and a rule about what an id may contain is
/// cheaper to be sure of than a rule about where a path ends up.
pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "`{id}` is not an artifact id: an id is the number the session's tool card named"
        ));
    }

    Ok(())
}

/// The artifacts directory a session's file implies, by the engine's own rule.
///
/// Transcribed from `session-manager.ts`'s `artifactsDirectoryFor`: the session file's
/// path with `.jsonl` removed is a *directory*, and a file that does not end in `.jsonl`
/// has no artifacts directory at all.
pub fn artifacts_dir(session_file: &str) -> Result<PathBuf, String> {
    let directory = session_file.strip_suffix(".jsonl").ok_or_else(|| {
        format!("`{session_file}` is not a session file, so it has no artifacts directory")
    })?;

    if directory.is_empty() {
        return Err(format!(
            "`{session_file}` is not a session file, so it has no artifacts directory"
        ));
    }

    Ok(PathBuf::from(directory))
}

/// The file an id names inside the artifacts directory.
///
/// The same scan the engine does (`artifacts.ts`'s `getPath`): the first directory entry
/// whose name starts with `<id>.`, taken in sorted order so two ids that share a prefix
/// (`1.` and `10.`) cannot resolve differently between two reads.
fn artifact_file(directory: &Path, id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;

    let Ok(listing) = fs::read_dir(directory) else {
        // A session that never spilled anything has no artifacts directory, and that is
        // the same answer as one whose artifacts `omp gc` archived: there is no file.
        return Err(gone(id));
    };

    let prefix = format!("{id}.");
    let mut matches: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
        })
        .collect();
    matches.sort();

    matches.into_iter().next().ok_or_else(|| gone(id))
}

/// What a missing artifact is, as the panel says it.
fn gone(id: &str) -> String {
    format!("artifact `{id}` is gone: the engine archives a cold session's artifacts (`omp gc`)")
}

/// A modification time in epoch milliseconds, or 0.
///
/// 0 rather than an error for the platforms that will not say (and for a file whose
/// timestamp predates the epoch): the tree draws a date only when there is one, and a
/// listing that failed over a timestamp would be a worse trade than a blank column.
fn modified_at(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::TodoTaskInput;

    /// A workspace of this test's own, removed when the test ends.
    fn workspace(label: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("omp-desktop-panel-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a test workspace");

        path
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused_and_one_inside_is_not() {
        let root = workspace("containment");
        let inside = root.join("src");
        let outside =
            std::env::temp_dir().join(format!("omp-desktop-panel-outside-{}", std::process::id()));
        fs::create_dir_all(&inside).expect("a directory inside");
        fs::create_dir_all(&outside).expect("a directory outside");
        fs::write(inside.join("main.rs"), "fn main() {}").expect("a file inside");

        // Inside, asked for by absolute path.
        let listing = tree(&root.to_string_lossy(), Some(&inside.to_string_lossy()))
            .expect("a directory inside the workspace is listed");
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "main.rs");
        assert!(!listing[0].is_dir);

        // Inside, asked for relatively: the panel opens the root with `null` and then
        // passes back absolute paths, but a relative path means the same thing.
        let listing = tree(&root.to_string_lossy(), Some("src")).expect("a relative path resolves");
        assert_eq!(listing[0].name, "main.rs");

        // Out, by absolute path.
        let refused = tree(&root.to_string_lossy(), Some(&outside.to_string_lossy()))
            .expect_err("a directory outside the workspace is refused");
        assert!(
            refused.contains("outside the thread's workspace"),
            "{refused}"
        );

        // Out, through `..` spelled from inside: the same refusal, because the check is
        // made on the resolved path rather than on what was sent.
        let escaped = format!("{}/../..", inside.display());
        let refused = tree(&root.to_string_lossy(), Some(&escaped))
            .expect_err("a path that climbs out is refused");
        assert!(
            refused.contains("outside the thread's workspace"),
            "{refused}"
        );

        // Out, through a symlink: `canonicalize` is what makes this one fail, and it is
        // the reason the check cannot be a string prefix test.
        #[cfg(unix)]
        {
            let link = root.join("escape");
            std::os::unix::fs::symlink(&outside, &link).expect("a symlink out of the workspace");
            let refused = tree(&root.to_string_lossy(), Some(&link.to_string_lossy()))
                .expect_err("a symlink out of the workspace is refused");
            assert!(
                refused.contains("outside the thread's workspace"),
                "{refused}"
            );
        }

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn one_level_is_sorted_directories_first_and_nothing_is_filtered() {
        let root = workspace("level");
        fs::create_dir_all(root.join("zeta")).expect("a directory");
        fs::create_dir_all(root.join("Alpha")).expect("a directory");
        fs::create_dir_all(root.join(".git")).expect("a hidden directory");
        fs::write(root.join("beta.txt"), "b").expect("a file");
        fs::write(root.join("Beta.txt"), "B").expect("a file");
        fs::write(root.join(".env"), "secret").expect("a hidden file");

        let names: Vec<String> = tree(&root.to_string_lossy(), None)
            .expect("the root is listed")
            .into_iter()
            .map(|entry| entry.name)
            .collect();

        assert_eq!(
            names,
            // Directories first, then case-insensitively by name, and `.git` and `.env`
            // are entries like any other: hiding them is the panel's decision.
            // (`.env` sorts before `Beta.txt` on the case-insensitive comparison, `.`
            // being below any letter.)
            vec![".git", "Alpha", "zeta", ".env", "Beta.txt", "beta.txt"],
        );

        let listing = tree(&root.to_string_lossy(), None).expect("the root is listed");
        let directory = listing
            .iter()
            .find(|entry| entry.name == "zeta")
            .expect("zeta");
        assert!(directory.is_dir);
        assert_eq!(directory.size, 0, "a directory has no size to draw");

        let file = listing
            .iter()
            .find(|entry| entry.name == "beta.txt")
            .expect("beta");
        assert!(!file.is_dir);
        assert_eq!(file.size, 1);
        assert!(file.modified_at > 0, "a real file has a timestamp");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_level_stops_at_the_cap_rather_than_reading_a_whole_directory() {
        let root = workspace("cap-level");
        // One past the cap, so the boundary is the thing asserted.
        for index in 0..=MAX_ENTRIES {
            fs::write(root.join(format!("{index:04}.txt")), "").expect("a file");
        }

        let listing = tree(&root.to_string_lossy(), None).expect("the level lists");
        assert_eq!(
            listing.len(),
            MAX_ENTRIES,
            "a level is capped, or a pathological directory stalls the window"
        );
        // Sorted within what was read, so the panel draws an ordered level whatever the
        // filesystem handed over.
        let mut sorted = listing.clone();
        sorted.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(listing, sorted);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_symlink_is_skipped_rather_than_failing_the_level() {
        let root = workspace("broken");
        fs::write(root.join("real.txt"), "hi").expect("a file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling"))
            .expect("a broken symlink");

        let names: Vec<String> = tree(&root.to_string_lossy(), None)
            .expect("a broken symlink does not fail the listing")
            .into_iter()
            .map(|entry| entry.name)
            .collect();

        assert_eq!(names, vec!["real.txt"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_artifact_id_must_be_digits_and_nothing_else() {
        // The ids the engine mints.
        assert!(validate_id("0").is_ok());
        assert!(validate_id("143").is_ok());

        // Everything a crafted id could be. The traversal attempt is the one that matters:
        // the scan below is a prefix match inside a directory, so an id that can carry a
        // separator is an id that can name another directory's file.
        for refused in [
            "",
            "..",
            "../etc/passwd",
            "../../../../etc/passwd",
            "1/../../etc/passwd",
            "1.bash",
            "1.",
            "/etc/passwd",
            "-1",
            "1e3",
            " 1",
        ] {
            assert!(
                validate_id(refused).is_err(),
                "`{refused}` is not an artifact id"
            );
        }
    }

    #[test]
    fn an_artifact_is_read_from_beside_the_session_file() {
        let root = workspace("artifact");
        let session_file = root.join("2026-09-20_abc.jsonl");
        let directory = artifacts_dir(&session_file.to_string_lossy()).expect("the engine's rule");
        assert_eq!(directory, root.join("2026-09-20_abc"));

        fs::create_dir_all(&directory).expect("the artifacts directory");
        fs::write(directory.join("7.bash.log"), "one\ntwo\n").expect("an artifact");

        let read = artifact(&directory, "7").expect("the artifact is read");
        assert_eq!(read.id, "7");
        assert_eq!(read.text, "one\ntwo\n");
        assert_eq!(read.bytes, 8);
        assert!(!read.truncated);
        assert!(read.path.ends_with("7.bash.log"));

        // A file that is not a session file has no artifacts directory at all.
        assert!(artifacts_dir("/tmp/notes.txt").is_err());

        // The id that would escape is refused before anything is listed.
        let refused = artifact(&directory, "../../../etc/passwd")
            .expect_err("a traversal attempt is refused");
        assert!(refused.contains("is not an artifact id"), "{refused}");

        // And an id with nothing behind it is the *gone* answer, not an error about the
        // read: the panel has a place to put a sentence, and a deleted artifact is normal.
        let missing = artifact(&directory, "8").expect_err("no such artifact");
        assert!(missing.contains("omp gc"), "{missing}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_artifact_over_the_cap_reports_its_real_size() {
        let root = workspace("cap");
        fs::create_dir_all(&root).expect("a directory");
        let content = "x".repeat(MAX_ARTIFACT_BYTES as usize + 16);
        fs::write(root.join("3.bash.log"), &content).expect("a spilled artifact");

        let read = artifact(&root, "3").expect("the artifact is read");
        assert!(read.truncated, "a file past the cap must say so");
        assert_eq!(read.bytes, content.len() as u64);
        assert_eq!(read.text.len(), MAX_ARTIFACT_BYTES as usize);

        // Exactly the cap is complete, not truncated.
        fs::write(
            root.join("4.bash.log"),
            "y".repeat(MAX_ARTIFACT_BYTES as usize),
        )
        .expect("a spilled artifact");
        let read = artifact(&root, "4").expect("the artifact is read");
        assert!(!read.truncated);
        assert_eq!(read.text.len(), MAX_ARTIFACT_BYTES as usize);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_plan_is_refused_and_a_plan_is_sent_without_an_absent_blocker() {
        assert!(outgoing(&[]).is_err());

        let sent = outgoing(&[TodoPhaseInput {
            name: "Implementation".to_string(),
            tasks: vec![
                TodoTaskInput {
                    content: "write it".to_string(),
                    status: "pending".to_string(),
                    blocker: None,
                },
                TodoTaskInput {
                    content: "ship it".to_string(),
                    status: "blocked".to_string(),
                    blocker: Some("the review".to_string()),
                },
            ],
        }])
        .expect("a plan with phases in it is sent");

        assert_eq!(
            sent,
            json!([{
                "name": "Implementation",
                "tasks": [
                    { "content": "write it", "status": "pending" },
                    { "content": "ship it", "status": "blocked", "blocker": "the review" },
                ],
            }]),
            "a task without a blocker must not carry `blocker: null`: the engine's own \
             projection keeps whatever it is given, so an absent field is what it sends too"
        );
    }

    #[test]
    fn the_panels_plan_shape_is_the_engines() {
        let plan = omp_session::decode_phases(&json!([{
            "name": "Implementation",
            "tasks": [
                { "content": "write it", "status": "in_progress" },
                { "content": "ship it", "status": "blocked", "blocker": "the review" },
            ],
        }]));

        let shaped = phases(&plan);
        assert_eq!(shaped.len(), 1);
        assert_eq!(shaped[0].name, "Implementation");
        assert_eq!(shaped[0].tasks[0].status, "in_progress");
        assert_eq!(shaped[0].tasks[1].blocker.as_deref(), Some("the review"));

        // A blank blocker is the same as none: an empty "waiting for" line is a line the
        // panel should not draw.
        let blank = omp_session::decode_phases(&json!([{
            "name": "Implementation",
            "tasks": [{ "content": "write it", "status": "blocked", "blocker": "   " }],
        }]));
        assert_eq!(phases(&blank)[0].tasks[0].blocker, None);
    }
}
