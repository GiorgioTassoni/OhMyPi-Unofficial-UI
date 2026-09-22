//! The record behind "always allow <tool>": `tools.approval.<tool> = allow`.
//!
//! The write goes through the engine's own config CLI rather than through the
//! file, because the file is the engine's: it is one layer of a merge, and it is
//! not ours to serialize. Measured at v18.2.6:
//!
//! * `omp config get tools.approval --json` prints `{"key": …, "value": {…},
//!   "type": "record", …}` — the record to merge is `value`.
//! * `omp config set tools.approval '{"bash":"allow"}'` persists it into the global
//!   config as valid nested YAML. The per-tool subkey form
//!   (`omp config set tools.approval.bash allow`) is rejected as an unknown
//!   setting, so the whole record has to be read, merged and written back.
//! * There is no clean removal — `omp config reset tools.approval` leaves
//!   `tools.approval: {}` behind — so a write here is always a merge, never a
//!   delete.
//!
//! # The write is not live
//!
//! Measured both directions with a real agent: recording `tools.approval.bash:
//! allow` **mid-session** does not stop the next approval prompt in that session,
//! while a session started afterwards does not prompt at all. The engine reads the
//! record when it constructs the session, so the click takes effect from the
//! *next* session onwards — which is what the dialog has to tell the user, and why
//! [`PolicyOutcome`] says what was written rather than claiming the prompt is gone.
//! (A debounce was ruled out: an 8-second window behaves the same.)
//!
//! A project-level `<workspace>/.omp/config.yml` carrying the same record *is*
//! honoured at session start, which is the cheap way to exercise this path without
//! writing the user's real config — `tests/approval.rs` proves the suppression that
//! way, and proves this module's write against the global config.

use std::process::Stdio;

use omp_transport::resolve_omp_binary;
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::process::Command;

/// The engine setting holding the per-tool policies.
const KEY: &str = "tools.approval";

/// The engine setting holding the session's approval mode (`docs/12` §7.2).
const MODE_KEY: &str = "tools.approvalMode";

/// The ladder, exactly as the engine's schema spells it.
pub const APPROVAL_MODES: [&str; 3] = ["always-ask", "write", "yolo"];

/// The one policy this module records. `deny` and `prompt` exist, but nothing a
/// user clicks in the approval dialog asks for them.
const ALLOW: &str = "allow";

/// Longest tool name accepted, in characters.
///
/// The name becomes a YAML key in a file the engine parses at startup, so it is
/// bounded rather than trusted: a hostile or broken name should be refused here,
/// where the user gets a message, instead of being written and read back later.
const MAX_TOOL_NAME_CHARS: usize = 128;

/// How many of the engine's stderr lines an error message carries.
const STDERR_TAIL_LINES: usize = 4;

/// What one "always allow" click did to the engine's config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyOutcome {
    /// The tool the policy was written for.
    pub tool: String,
    /// The policy now recorded for it — always `"allow"`.
    pub policy: String,
    /// The policy that was recorded before, when there was one.
    pub previous: Option<String>,
}

/// Refuse a tool name that cannot safely become a config key.
///
/// The allowed set is what a tool name can be — `read`, `mcp__server.read`,
/// `web:fetch` — and nothing else, so that a name can never terminate the key, add
/// a sibling entry, or reach into another part of the document.
pub fn validate_tool_name(tool: &str) -> Result<(), String> {
    if tool.is_empty() {
        return Err("the tool name is empty".to_string());
    }

    let length = tool.chars().count();
    if length > MAX_TOOL_NAME_CHARS {
        return Err(format!(
            "the tool name is too long ({length} characters; the limit is {MAX_TOOL_NAME_CHARS})"
        ));
    }

    if let Some(offender) = tool
        .chars()
        .find(|character| !is_name_character(*character))
    {
        return Err(format!(
            "the tool name contains {offender:?}, which cannot be a config key — letters, digits, `.`, `_`, `:` and `-` only"
        ));
    }

    Ok(())
}

/// Whether a character may appear in a policy key.
fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
}

/// The record with `tool` set to `policy`, plus whatever that tool held before.
///
/// Merging rather than replacing is forced by the CLI: it refuses per-tool subkeys,
/// so the write carries the whole record and every other tool's policy goes with
/// it. A value that is not a string — the engine would not produce one, but a
/// hand-edited config could — is replaced and reported as *no* previous value
/// rather than as a string it never was.
pub fn merge_record(
    record: &Map<String, Value>,
    tool: &str,
    policy: &str,
) -> (Map<String, Value>, Option<String>) {
    let previous = record.get(tool).and_then(Value::as_str).map(str::to_string);

    let mut merged = record.clone();
    merged.insert(tool.to_string(), Value::String(policy.to_string()));

    (merged, previous)
}

/// Record `tools.approval.<tool> = allow` in the engine's config.
///
/// Takes effect the next time a session starts, not in the one that is asking —
/// see the module docs. The outcome reports the write, not a promise about the
/// running agent.
pub async fn allow_tool(tool: &str) -> Result<PolicyOutcome, String> {
    validate_tool_name(tool)?;

    let record = read_policies().await?;
    let (merged, previous) = merge_record(&record, tool, ALLOW);
    write_policies(&Value::Object(merged).to_string()).await?;

    Ok(PolicyOutcome {
        tool: tool.to_string(),
        policy: ALLOW.to_string(),
        previous,
    })
}

/// Record the approval mode for the sessions that follow (`docs/12` §7.2).
///
/// A scalar key, so unlike `tools.approval` the CLI accepts the value directly —
/// measured: `omp config set tools.approvalMode always-ask` persists it, and
/// `omp config get` answers with it. It is read when a session is constructed, so this
/// is the half of the mode switch that outlives the process; the running session needs
/// the restart that goes with it.
pub async fn record_mode(mode: &str) -> Result<(), String> {
    validate_mode(mode)?;
    config(
        "could not record the approval mode",
        &["set", MODE_KEY, mode],
    )
    .await
    .map(|_| ())
}

/// Refuse a mode the engine does not have.
///
/// Checked here rather than trusted to the CLI: the value travels from a click, and a
/// typo would otherwise be written to the config and only surface at the next session,
/// as an engine that silently kept its old ladder.
pub fn validate_mode(mode: &str) -> Result<(), String> {
    if APPROVAL_MODES.contains(&mode) {
        return Ok(());
    }
    Err(format!(
        "`{mode}` is not an approval mode — one of {}",
        APPROVAL_MODES.join(", ")
    ))
}

/// The policies the engine currently holds.
async fn read_policies() -> Result<Map<String, Value>, String> {
    let output = config("could not read the tool policies", &["get", KEY, "--json"]).await?;

    let parsed: Value = serde_json::from_str(&output).map_err(|error| {
        format!(
            "could not read the tool policies: the engine's output was not the expected JSON ({error})\n  it printed: {output}"
        )
    })?;

    match parsed.get("value") {
        Some(Value::Object(record)) => Ok(record.clone()),
        Some(other) => Err(format!(
            "could not read the tool policies: expected a record of tool to policy, got {other}"
        )),
        None => Err(format!(
            "could not read the tool policies: the engine's JSON carried no `value`: {parsed}"
        )),
    }
}

/// Write the whole record back, which is the only form the CLI accepts.
async fn write_policies(payload: &str) -> Result<(), String> {
    config("could not record the policy", &["set", KEY, payload])
        .await
        .map(|_| ())
}

/// Run one `omp config …` subcommand and return its stdout.
///
/// `intent` is the half of the message the user will read, so the two callers name
/// what they were doing; the failure detail is appended here rather than invented
/// per call site.
async fn config(intent: &str, args: &[&str]) -> Result<String, String> {
    let program = resolve_omp_binary();
    // Captured before spawning: when the binary cannot be found, the path that was
    // tried is the whole diagnosis.
    let tried = program.display().to_string();

    let output = Command::new(&program)
        .arg("config")
        .args(args)
        // The CLI is not being asked anything; leaving stdin attached would let a
        // prompt it should never make hang the command.
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("{intent}: could not run `{tried}` ({error})"))?;

    if !output.status.success() {
        return Err(format!(
            "{intent}: `omp config {}` {} — {}",
            args.join(" "),
            output.status,
            stderr_tail(&output.stderr),
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("{intent}: the engine's output was not text ({error})"))
}

/// The engine's own explanation of a failure, for a message the user reads.
///
/// The tail rather than the whole stream: a failure prints its reason last, and the
/// CLI is free to be chatty above it.
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return "it printed no error output".to_string();
    }

    let start = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines[start..].join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record of tool to policy, the shape `tools.approval` has on the wire.
    fn record(entries: &[(&str, &str)]) -> Map<String, Value> {
        entries
            .iter()
            .map(|(tool, policy)| (tool.to_string(), Value::String(policy.to_string())))
            .collect()
    }

    #[test]
    fn only_the_engines_three_modes_are_accepted() {
        // The ladder is the engine's, so a value outside it is a bug upstream of the
        // write, not something to store and find out about at the next session.
        for mode in APPROVAL_MODES {
            assert!(validate_mode(mode).is_ok(), "{mode} must be accepted");
        }
        for mode in ["", "auto", "default", "YOLO", "write ", "always_ask"] {
            assert!(
                validate_mode(mode).is_err(),
                "{mode:?} must be refused with a message naming the ladder"
            );
        }
        let error = validate_mode("plan").expect_err("refused");
        assert!(error.contains("always-ask"), "the message lists the modes");
    }

    #[test]
    fn a_name_that_can_be_a_config_key_passes() {
        // The names the engine actually uses, `mcp` namespacing included.
        for name in [
            "bash",
            "write",
            "mcp__server.read",
            "web:fetch",
            "my-tool",
            "a.b.c",
        ] {
            assert!(
                validate_tool_name(name).is_ok(),
                "{name:?} must be accepted"
            );
        }
    }

    #[test]
    fn a_name_carrying_a_character_outside_the_key_alphabet_is_refused() {
        // Every one of these would change what the record *means* if it reached the
        // file: a space or quote breaks the key, a newline adds a sibling entry, a
        // slash or a backslash walks into another part of the document.
        for name in [
            " ",
            "read\n",
            "a\tb",
            "read/write",
            "read\"",
            "it's",
            "read\\write",
            "é",
        ] {
            let error = validate_tool_name(name).expect_err(&format!("{name:?} must be refused"));
            assert!(
                error.contains("letters, digits"),
                "a refusal should say what is allowed, got {error:?} for {name:?}"
            );
        }

        // The offending character itself is named, so the user can see it.
        let error = validate_tool_name("read\n").expect_err("must refuse");
        assert!(error.contains(r"'\n'"), "got {error}");
        let error = validate_tool_name("read/write").expect_err("must refuse");
        assert!(error.contains("'/'"), "got {error}");
    }

    #[test]
    fn an_empty_name_is_refused_before_anything_else() {
        let error = validate_tool_name("").expect_err("must refuse");
        assert!(error.contains("empty"), "got {error}");
    }

    #[test]
    fn a_name_longer_than_the_limit_is_refused() {
        let at_the_limit = "a".repeat(MAX_TOOL_NAME_CHARS);
        assert!(
            validate_tool_name(&at_the_limit).is_ok(),
            "the limit itself is usable"
        );

        let past_it = "a".repeat(MAX_TOOL_NAME_CHARS + 1);
        let error = validate_tool_name(&past_it).expect_err("must refuse");
        assert!(error.contains("too long"), "got {error}");
        assert!(
            error.contains(&(MAX_TOOL_NAME_CHARS + 1).to_string()),
            "got {error}"
        );
    }

    #[test]
    fn a_first_policy_lands_in_an_empty_record() {
        let (merged, previous) = merge_record(&record(&[]), "bash", ALLOW);

        assert_eq!(previous, None, "nothing was recorded for bash before");
        assert_eq!(merged.get("bash"), Some(&Value::String("allow".into())));
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn another_tools_policy_survives_the_merge() {
        // The whole reason `merge_record` exists: the CLI rejects per-tool subkeys,
        // so this write carries the entire record and would drop this entry if it
        // simply replaced it.
        let (merged, _) = merge_record(&record(&[("write", "deny")]), "bash", ALLOW);

        assert_eq!(
            merged.get("write"),
            Some(&Value::String("deny".into())),
            "another tool's policy must come back out of the merge"
        );
        assert_eq!(merged.get("bash"), Some(&Value::String("allow".into())));
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn overwriting_a_policy_reports_the_one_it_replaced() {
        for before in ["deny", "prompt"] {
            let (merged, previous) = merge_record(&record(&[("bash", before)]), "bash", ALLOW);

            assert_eq!(
                previous.as_deref(),
                Some(before),
                "the dialog shows what the write took away"
            );
            assert_eq!(merged.get("bash"), Some(&Value::String("allow".into())));
        }
    }

    #[test]
    fn rewriting_an_already_allowed_tool_reports_what_was_there() {
        let (merged, previous) = merge_record(&record(&[("bash", "allow")]), "bash", ALLOW);

        assert_eq!(
            previous.as_deref(),
            Some("allow"),
            "`allow` is what was recorded, and saying so keeps the UI honest"
        );
        assert_eq!(merged.get("bash"), Some(&Value::String("allow".into())));
    }

    #[test]
    fn a_value_that_is_not_a_policy_is_replaced_without_being_reported() {
        // A hand-edited config can hold a nested record where a policy belongs.
        let mut odd = Map::new();
        odd.insert("bash".to_string(), serde_json::json!({"nested": true}));

        let (merged, previous) = merge_record(&odd, "bash", ALLOW);

        assert_eq!(previous, None, "a string it never was must not be reported");
        assert_eq!(merged.get("bash"), Some(&Value::String("allow".into())));
    }
}
