//! The engine's config CLI, which is the app's only writer.
//!
//! # Why the app does not write the file itself
//!
//! `docs/13` protection 3 asks for an atomic, locked, merge-not-clobber write, and the
//! obvious reading is to implement it. Measured at v18.2.6, that write is not ours to
//! make: the engine serialises it with a **native** cross-process lock
//! (`@oh-my-pi/pi-utils/file-lock` → `pi-natives`: an abstract Unix socket on Linux, a
//! named mutex on Windows, `flock(2)` elsewhere, keyed off the resolved path), it resolves
//! symlinked config paths to their *physical* target before renaming onto them, it applies
//! migrations, and it quarantines a file it cannot parse. A second writer that got any of
//! those wrong would lose a user's config quietly, which is the one failure this screen
//! must never have.
//!
//! So every write goes through `omp config set` / `omp config reset`: the engine's own
//! entry point, which holds that lock, validates against its own schema (including the
//! per-key validators we cannot see), and reports what it did. Measured costs: 0.24 s per
//! write, and the file it leaves is a *sparse* patch — only what differs from the defaults,
//! mode `0600` — which is why "is this key set on disk" is a question for the file, not for
//! the value the engine reports.
//!
//! # Values on the command line
//!
//! Scalar values go as bare text and structured ones as JSON (`set enabledModels '["a"]'`,
//! `set modelRoles '{"default":"x/y"}'` — both measured). The value is placed after `--`:
//! the CLI's own parser refuses a positional that begins with a dash (`Unknown option '-5'`
//! with a hint to use `--`), and measured, `--` is transparent for ordinary values too, so
//! there is one path rather than a conditional one.

use std::collections::BTreeMap;
use std::process::Stdio;

use omp_transport::resolve_omp_binary;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use crate::catalog::SettingType;

/// How many of the engine's stderr lines an error message carries.
const STDERR_TAIL_LINES: usize = 4;

/// One key as the engine reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// The effective value. Absent when the key is unset *and* has no default, and — for a
    /// credential that is set — replaced by `redacted`, because the engine's `list` never
    /// prints a stored secret.
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(rename = "type")]
    pub kind: SettingType,
    /// The schema's own description; empty for keys that carry no `ui` metadata.
    #[serde(default)]
    pub description: String,
    /// The engine's way of saying "this key holds a value you may not see".
    #[serde(default)]
    pub redacted: bool,
}

/// The engine's whole settings list, in key order.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub entries: BTreeMap<String, Entry>,
}

impl Listing {
    /// One key's entry.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// The effective value of a key, for the condition predicates.
    pub fn value_of(&self, key: &str) -> Option<Value> {
        self.entries.get(key).and_then(|entry| entry.value.clone())
    }
}

/// The engine's config CLI, run against the profile the app uses.
///
/// `profile` exists because the engine has profiles (`docs/13` §"What it edits": a named
/// profile keeps its own `agent/config.yml`), and because a test must never touch the
/// user's real config: a throwaway profile is how the write path is exercised for real.
#[derive(Debug, Clone, Default)]
pub struct Cli {
    profile: Option<String>,
}

impl Cli {
    /// The CLI as the app runs it: the default profile, the one its sessions use.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same CLI against a named profile.
    pub fn with_profile(profile: impl Into<String>) -> Self {
        Self {
            profile: Some(profile.into()),
        }
    }

    /// Every setting the engine knows, with its effective value.
    pub async fn list(&self) -> Result<Listing, String> {
        let output = self
            .run(
                "could not read the settings",
                &["list".into(), "--json".into()],
            )
            .await?;

        let entries: BTreeMap<String, Entry> = serde_json::from_str(&output).map_err(|error| {
            format!("could not read the settings: the engine's list was not the expected JSON ({error})")
        })?;

        Ok(Listing { entries })
    }

    /// Record a value for a key.
    pub async fn set(&self, key: &str, value: &Value, kind: SettingType) -> Result<(), String> {
        let text = value_text(value, kind);
        self.run(
            "could not write the setting",
            &["set".into(), key.to_string(), "--".into(), text],
        )
        .await
        .map(|_| ())
    }

    /// Return a key to the engine's default.
    pub async fn reset(&self, key: &str) -> Result<(), String> {
        self.run(
            "could not clear the setting",
            &["reset".into(), key.to_string()],
        )
        .await
        .map(|_| ())
    }

    /// The directory holding this profile's config, sessions and credentials.
    pub async fn agent_dir(&self) -> Result<String, String> {
        self.run(
            "could not find the engine's config directory",
            &["path".into()],
        )
        .await
        .map(|output| output.trim().to_string())
    }

    /// Run one `omp config …` subcommand and return its stdout.
    ///
    /// `intent` is the half of the message the user will read, so each caller names what it
    /// was doing; the failure detail is appended here rather than invented per call site.
    /// This is the same discipline `src-tauri/src/policy.rs` established for the one write
    /// it makes, and that module now calls through here rather than spawning its own.
    async fn run(&self, intent: &str, args: &[String]) -> Result<String, String> {
        let program = resolve_omp_binary();
        // Captured before spawning: when the binary cannot be found, the path that was
        // tried is the whole diagnosis.
        let tried = program.display().to_string();

        let mut command = Command::new(&program);
        if let Some(profile) = &self.profile {
            command.arg("--profile").arg(profile);
        }
        let output = command
            .arg("config")
            .args(args)
            // The CLI is not being asked anything; leaving stdin attached would let a prompt
            // it should never make hang the command.
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| format!("{intent}: could not run `{tried}` ({error})"))?;

        if !output.status.success() {
            // The engine refuses unknown keys and values outside an enum's domain with a
            // sentence that already names both — measured — so its own words are the ones
            // the user should read.
            return Err(format!(
                "{intent}: `omp config {}` {} — {}",
                args.join(" "),
                output.status,
                stderr_tail(&output.stderr)
            ));
        }

        String::from_utf8(output.stdout)
            .map_err(|error| format!("{intent}: the engine's output was not text ({error})"))
    }
}

/// A value as the CLI takes it: bare text for scalars, JSON for anything structured.
fn value_text(value: &Value, kind: SettingType) -> String {
    match kind {
        SettingType::String | SettingType::Enum => value
            .as_str()
            .map(str::to_string)
            // Unreachable through [`crate::validate`], which runs first: a value of the
            // wrong shape never reaches the CLI. JSON is the honest fallback rather than a
            // panic in a command handler.
            .unwrap_or_else(|| value.to_string()),
        SettingType::Boolean | SettingType::Number => value.to_string(),
        SettingType::Array | SettingType::Record => value.to_string(),
    }
}

/// The engine's own explanation of a failure, for a message the user reads.
///
/// The tail rather than the whole stream: a failure prints its reason last, and the CLI is
/// free to be chatty above it.
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
    lines[start..].join("\n  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_scalar_goes_as_text_and_a_record_as_json() {
        assert_eq!(
            value_text(&json!("always-ask"), SettingType::Enum),
            "always-ask"
        );
        assert_eq!(
            value_text(&json!("titanium"), SettingType::String),
            "titanium"
        );
        assert_eq!(value_text(&json!(true), SettingType::Boolean), "true");
        assert_eq!(value_text(&json!(45), SettingType::Number), "45");
        assert_eq!(value_text(&json!(-5), SettingType::Number), "-5");
        assert_eq!(
            value_text(&json!(["a", "b"]), SettingType::Array),
            r#"["a","b"]"#
        );
        assert_eq!(
            value_text(&json!({"default": "x/y"}), SettingType::Record),
            r#"{"default":"x/y"}"#
        );
    }

    /// A string that reads as JSON must stay a string: the CLI would otherwise store a list
    /// where the schema says text.
    #[test]
    fn a_string_that_looks_structured_is_still_text() {
        assert_eq!(
            value_text(&json!("[1,2]"), SettingType::String),
            "[1,2]",
            "no JSON re-encoding for a scalar"
        );
    }

    #[test]
    fn the_stderr_tail_keeps_the_last_lines() {
        // Fewer lines than the tail keeps all of them; more, and the tail is the last few.
        let short = b"noise\nnoise\nthe real reason\n";
        assert_eq!(stderr_tail(short), "noise\n  noise\n  the real reason");

        let many = b"1\n2\n3\n4\n5\n6\n";
        assert_eq!(stderr_tail(many), "3\n  4\n  5\n  6");
        assert_eq!(stderr_tail(b""), "it printed no error output");
        assert_eq!(stderr_tail(b"\n\n"), "it printed no error output");
    }
}
