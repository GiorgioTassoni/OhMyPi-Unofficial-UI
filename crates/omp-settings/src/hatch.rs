//! The raw config escape hatch: read what a config file actually sets, and turn a hand-edit
//! into a plan the engine can apply.
//!
//! # Why the file is parsed here at all
//!
//! Two questions need it, and neither is answerable from the engine:
//!
//! * **Where does this value come from?** `omp config list` reports *effective* values — the
//!   file, the project file, an overlay and the defaults all merged — so "set on disk" is a
//!   question only the file answers. That is what a row's provenance shows.
//! * **What did this edit change?** The hatch is a text editor over a file the engine owns,
//!   and the only safe way to apply it through `config set` is to know which *keys* moved.
//!
//! The parse is deliberately read-only and deliberately partial: a file that cannot be parsed,
//! or that says something this build does not understand, produces refusals rather than a
//! guessed write.
//!
//! # The flattening rule
//!
//! The file is a *sparse patch* — the engine writes only what differs from the defaults — and
//! it nests dotted keys (`gc:\n  coldArchiveAfterDays: 45`). So a mapping is descended into by
//! its key path, **except** where the path itself is a known setting: `tools.approval` is a
//! record, and its children are its value, not settings of their own. The deepest known key
//! wins, and anything left over is reported as a key this build does not know rather than
//! silently dropped.
//!
//! Scalar resolution matters more than it looks: YAML 1.1 would read `off` as a boolean, and a
//! user writing `sleepPrevention: off` would then be told their edit is invalid. `serde_yaml`
//! resolves the YAML 1.2 core schema — measured: `off` and `yes` stay strings, `45` is a
//! number, and an alias resolves — so the text a human typed is what gets validated.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::catalog::{Catalog, Restart};
use crate::validate;

/// One key a config file sets.
#[derive(Debug, Clone, PartialEq)]
pub struct SetKey {
    pub key: String,
    pub value: Value,
}

/// Something in the file the app will not act on, with the reason it can show.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The key path, as the file spells it when it can be worked out.
    pub key: String,
    pub reason: String,
}

/// A file, flattened into the keys it sets.
#[derive(Debug, Clone, Default)]
pub struct Flattened {
    pub keys: Vec<SetKey>,
    /// Parts of the file that could not be read as settings. A file with refusals is still
    /// *readable* — the rest of its keys are real — but its provenance is incomplete, and a
    /// hand-edit must not be applied from it.
    pub refusals: Vec<Refusal>,
}

impl Flattened {
    /// Whether the file holds a value for a key.
    pub fn sets(&self, key: &str) -> bool {
        self.keys.iter().any(|entry| entry.key == key)
    }

    /// Everything, as a map (last writer wins, and YAML forbids duplicates anyway).
    pub fn map(&self) -> BTreeMap<String, Value> {
        self.keys
            .iter()
            .map(|entry| (entry.key.clone(), entry.value.clone()))
            .collect()
    }
}

/// Why a config file could not be read at all.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxError {
    pub message: String,
    /// 1-based, as an editor counts.
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl SyntaxError {
    /// The sentence a banner shows.
    pub fn sentence(&self) -> String {
        match (self.line, self.column) {
            (Some(line), Some(column)) => format!("line {line}, column {column}: {}", self.message),
            (Some(line), None) => format!("line {line}: {}", self.message),
            _ => self.message.clone(),
        }
    }
}

/// Flatten a config file against the catalog.
///
/// A syntax error is the only failure: everything else becomes a refusal, because a file the
/// app cannot fully understand is still a file the user can edit by hand.
pub fn flatten(text: &str, catalog: &Catalog) -> Result<Flattened, SyntaxError> {
    let document: serde_yaml::Value = serde_yaml::from_str(text).map_err(|error| {
        let location = error.location();
        SyntaxError {
            message: error
                .to_string()
                .split(" at line")
                .next()
                .unwrap_or_default()
                .to_string(),
            line: location.as_ref().map(|location| location.line()),
            column: location.as_ref().map(|location| location.column()),
        }
    })?;

    let mut flattened = Flattened::default();
    match document {
        serde_yaml::Value::Null => {}
        serde_yaml::Value::Mapping(mapping) => {
            descend(&mapping, "", catalog, &mut flattened);
        }
        // A document that is not a mapping cannot be a settings file. `omp` would quarantine
        // it; the app says so rather than pretending it has no settings.
        _ => flattened.refusals.push(Refusal {
            key: String::new(),
            reason: "the file's top level is not a mapping of settings".to_string(),
        }),
    }

    Ok(flattened)
}

/// Walk a mapping, flattening it into dotted key paths.
fn descend(mapping: &serde_yaml::Mapping, prefix: &str, catalog: &Catalog, out: &mut Flattened) {
    for (key, value) in mapping {
        let Some(segment) = key.as_str() else {
            out.refusals.push(Refusal {
                key: prefix.to_string(),
                reason: format!(
                    "a mapping key that is not text ({}) cannot be a settings key",
                    describe_scalar(key)
                ),
            });
            continue;
        };

        let path = if prefix.is_empty() {
            segment.to_string()
        } else {
            format!("{prefix}.{segment}")
        };

        // A known key is a leaf, even when its value is a mapping: records are settings, and
        // their children belong to them.
        if catalog.knows(&path) {
            out.keys.push(SetKey {
                key: path,
                value: to_json(value),
            });
            continue;
        }

        match value {
            serde_yaml::Value::Mapping(inner) => descend(inner, &path, catalog, out),
            other => out.keys.push(SetKey {
                key: path,
                value: to_json(other),
            }),
        }
    }
}

/// A YAML scalar, as the sentence that refuses it.
fn describe_scalar(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(value) => value.to_string(),
        serde_yaml::Value::Number(value) => value.to_string(),
        serde_yaml::Value::String(value) => format!("{value:?}"),
        serde_yaml::Value::Sequence(_) => "a list".to_string(),
        serde_yaml::Value::Mapping(_) => "a mapping".to_string(),
        serde_yaml::Value::Tagged(tagged) => format!("a tagged value ({})", tagged.tag),
    }
}

/// A YAML node as the JSON the engine's own JSON output would use.
///
/// Measured against the engine's `config list --json` for the same file: strings, numbers,
/// booleans, lists and records all round-trip, which is what makes a diff against the engine's
/// answer meaningful.
fn to_json(value: &serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(value) => Value::Bool(*value),
        serde_yaml::Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Value::Number(integer.into())
            } else if let Some(unsigned) = value.as_u64() {
                Value::Number(unsigned.into())
            } else if let Some(float) = value.as_f64() {
                serde_json::Number::from_f64(float).map_or(Value::Null, Value::Number)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(value) => Value::String(value.clone()),
        serde_yaml::Value::Sequence(items) => Value::Array(items.iter().map(to_json).collect()),
        serde_yaml::Value::Mapping(mapping) => Value::Object(
            mapping
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        serde_yaml::Value::String(text) => text.clone(),
                        serde_yaml::Value::Number(number) => number.to_string(),
                        serde_yaml::Value::Bool(flag) => flag.to_string(),
                        _ => describe_scalar(key),
                    };
                    (key, to_json(value))
                })
                .collect(),
        ),
        // An alias is resolved by the parser, so this is unreachable in practice; a tag the
        // app does not know is carried as its inner value rather than dropped.
        serde_yaml::Value::Tagged(tagged) => to_json(&tagged.value),
    }
}

/// What a hand-edit does to one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// The key gains a value, or changes to another one.
    Set,
    /// The key is gone from the file, so the engine returns it to its default.
    Reset,
}

/// One edit, as the plan describes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub key: String,
    pub action: Action,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub restart: Restart,
}

/// A hand-edit, checked before anything is written.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The edits that will be applied, in file order.
    pub changes: Vec<Change>,
    /// Everything the plan will not do, with the reason. A plan with refusals applies
    /// nothing: a half-applied edit is a file that disagrees with what the user sees.
    pub refusals: Vec<Refusal>,
}

impl Plan {
    /// Whether the plan would change anything.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.refusals.is_empty()
    }

    /// Whether the plan is applicable (nothing refused).
    pub fn is_applicable(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// Read what a config file sets, and refuse an unusable one.
pub fn read(text: &str, catalog: &Catalog) -> Result<Flattened, SyntaxError> {
    flatten(text, catalog)
}

/// Compare what a file said with what the edited text says, and validate every change.
///
/// `before` is the text as it was when the hatch was opened — the app's own last known-good
/// copy — not the file on disk right now: the engine and the user's editor both write this
/// file, and a diff against a moving target would report changes nobody made.
pub fn plan(before: &Flattened, edited_text: &str, catalog: &Catalog) -> Plan {
    let Ok(after) = flatten(edited_text, catalog) else {
        // A syntax error is reported by the caller, which has the text and can show the line.
        // Here it means the edit cannot be turned into changes at all.
        return Plan::default();
    };

    let mut plan = Plan::default();
    plan.refusals.extend(after.refusals.iter().cloned());

    let before_map = before.map();
    let after_map = after.map();

    // Keys the file dropped: the engine returns each to its default.
    for (key, value) in &before_map {
        if after_map.contains_key(key) {
            continue;
        }
        match catalog.spec(key) {
            Some(spec) => plan.changes.push(Change {
                key: key.clone(),
                action: Action::Reset,
                before: Some(value.clone()),
                after: None,
                restart: spec.restart,
            }),
            None => plan.refusals.push(Refusal {
                key: key.clone(),
                reason: refuses_unknown(key),
            }),
        }
    }

    // Keys the edit sets or changes.
    for (key, value) in &after_map {
        if before_map.get(key) == Some(value) {
            continue;
        }
        let Some(spec) = catalog.spec(key) else {
            plan.refusals.push(Refusal {
                key: key.clone(),
                reason: refuses_unknown(key),
            });
            continue;
        };

        if let Err(reason) = validate::value(spec, value) {
            plan.refusals.push(Refusal {
                key: key.clone(),
                reason,
            });
            continue;
        }

        if spec.is_secret() {
            plan.refusals.push(Refusal {
                key: key.clone(),
                reason: refuses_secret(&spec.key),
            });
            continue;
        }

        plan.changes.push(Change {
            key: key.clone(),
            action: Action::Set,
            before: before_map.get(key).cloned(),
            after: Some(value.clone()),
            restart: spec.restart,
        });
    }

    plan
}

/// The sentence for a key this build cannot write.
fn refuses_unknown(key: &str) -> String {
    format!(
        "`{key}` is not in this build's settings catalog, so the app will not write it — \
         edit the file in your own editor, or regenerate the catalog"
    )
}

/// The sentence for a credential the hatch will not handle.
///
/// The rules in `docs/13` §Secrets are replace-only and never display a stored value; a text
/// editor that can write a key the app cannot read back would break that in one paste. The
/// value is masked out of the text *before* it is ever sent to the frontend (see [`redact`]),
/// so the pane can show the file without showing a secret.
fn refuses_secret(key: &str) -> String {
    format!(
        "`{key}` holds a credential: this editor does not write secrets — set it from its own row, \
         or in your own editor"
    )
}

/// Shorter than this, a secret is not masked as a substring: masking a 2-character value
/// everywhere would mangle the rest of the file, and the honest answer for a value that short
/// is to refuse the pane's text rather than guess at it.
const MIN_MASKABLE_SECRET: usize = 4;

/// What a masked secret looks like in the pane.
pub const MASK: &str = "••••••••";

/// The file's text with every secret value masked out.
///
/// Masking is by *value*, not by position: the app knows which keys are credentials and what
/// their values are (it had to read the file to answer "is this key set"), and a plain
/// substitution of each value is exact, needs no line arithmetic, and over-masks rather than
/// under-masks when a value appears somewhere unexpected. A value too short to substitute
/// safely masks its whole line instead.
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut redacted = text.to_string();

    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        if secret.chars().count() >= MIN_MASKABLE_SECRET {
            redacted = redacted.replace(secret.as_str(), MASK);
            continue;
        }

        // Too short to substitute: take out the lines that carry it.
        redacted = redacted
            .lines()
            .map(|line| {
                if line.contains(secret.as_str()) {
                    mask_line(line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    redacted
}

/// A line with everything after its colon replaced — the shape a masked key keeps.
fn mask_line(line: &str) -> String {
    match line.split_once(':') {
        Some((key, _)) => format!("{key}: {MASK}"),
        None => MASK.to_string(),
    }
}

/// How the app names its own backups, so they are never confused with the engine's
/// `.broken-*` quarantine files.
const BACKUP_SUFFIX: &str = ".app-backup-";

/// Copy a config file aside, and return the copy's path.
///
/// Taken before the first write of a session and kept for it: the escape hatch is the one
/// place a user edits many keys at once, and "revert" has to be possible without trusting a
/// diff to be reversible. The copy is a *file* copy and not a write to the config — the config
/// still only ever changes through the engine.
pub fn backup(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if !path.exists() {
        return Err(format!(
            "there is nothing to back up: {} does not exist yet",
            path.display()
        ));
    }

    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "config.yml".to_string());

    let mut candidate = path.with_file_name(format!("{name}{BACKUP_SUFFIX}{seconds}"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = path.with_file_name(format!("{name}{BACKUP_SUFFIX}{seconds}-{counter}"));
        counter += 1;
    }

    std::fs::copy(path, &candidate)
        .map_err(|error| format!("could not back up {}: {error}", path.display()))?;

    Ok(candidate)
}

/// The app's own backups of a file, newest first.
pub fn backups(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Some(directory) = path.parent() else {
        return Vec::new();
    };
    let Some(name) = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        return Vec::new();
    };
    let prefix = format!("{name}{BACKUP_SUFFIX}");

    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut found: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|entry| {
            entry
                .file_name()
                .map(|name| name.to_string_lossy().starts_with(&prefix))
                .unwrap_or(false)
        })
        .collect();

    // Newest first: the name ends in the timestamp, and the counter after it only exists to
    // break a tie inside one second, so a plain name sort is newest-first with a stable tail.
    found.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    found
}

/// When a backup was taken, in seconds since the epoch, from its name.
pub fn backup_taken_at(path: &std::path::Path) -> Option<u64> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let (_, stamp) = name.split_once(BACKUP_SUFFIX)?;
    let seconds = stamp.split('-').next()?;
    seconds.parse().ok()
}

/// How many backups the hatch carries to the frontend, newest first.
const BACKUPS_SHOWN: usize = 3;

/// One backup of the config file, as the hatch offers it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub path: String,
    /// Seconds since the epoch, from the name — `None` when the name was not the app's own.
    pub at: Option<u64>,
    /// The backup's text, redacted exactly like the live file's.
    pub text: String,
}

/// What the escape hatch needs to draw itself.
///
/// `text` is the file **after redaction**: a config file holds credentials in plain text, and
/// the app's rule for them is that they are never displayed. The mask is applied here rather
/// than in the frontend so the plaintext never crosses the IPC boundary at all — and the
/// *baseline* a hand-edit is diffed against is the redacted text, which is why an untouched
/// credential line does not read as a change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub path: String,
    /// Whether the file exists yet (a fresh install has none until the first write).
    pub exists: bool,
    pub text: String,
    /// Why the file could not be read, when it could not — a syntax error's own words.
    pub error: Option<String>,
    /// The keys the file sets, sorted, so the screen can say how much is in there.
    pub keys: Vec<String>,
    /// The parts of the file the app will not act on.
    pub refusals: Vec<Refusal>,
    pub backups: Vec<Backup>,
}

/// Whether a key's *name* looks like it holds a credential.
///
/// The catalog's 8 credentials are the known ones; this is the belt to that braces. A key the
/// catalog has never seen — after an engine bump, or one a user invented — would otherwise be
/// displayed in full, and a leak is not something a later test can undo.
fn looks_like_a_credential(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    ["key", "token", "password", "passwd", "secret", "credential"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

/// The file's text with every credential value masked.
///
/// Fails when the file cannot be parsed, because a file the app cannot parse is one whose
/// secrets it cannot find — and showing it unmasked to be helpful is the one failure this must
/// not have.
pub fn redact_file(text: &str, catalog: &Catalog) -> Result<String, SyntaxError> {
    let flattened = flatten(text, catalog)?;

    let secrets: Vec<String> = flattened
        .keys
        .iter()
        .filter(|entry| {
            catalog
                .spec(&entry.key)
                .is_some_and(|spec| spec.is_secret())
                || looks_like_a_credential(&entry.key)
        })
        .filter_map(|entry| scalar_text(&entry.value))
        .collect();

    Ok(redact(text, &secrets))
}

/// A value worth masking, when it is a non-empty scalar.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Read a config file the way the hatch draws it.
///
/// One call rather than four, so the host cannot accidentally send the unredacted text: the
/// order (read → parse → redact → list backups) lives here, with the tests.
pub fn open(path: &std::path::Path, catalog: &Catalog) -> Result<Payload, String> {
    let mut payload = Payload {
        path: path.display().to_string(),
        exists: false,
        text: String::new(),
        error: None,
        keys: Vec::new(),
        refusals: Vec::new(),
        backups: Vec::new(),
    };

    for backup in backups(path).into_iter().take(BACKUPS_SHOWN) {
        let Ok(text) = std::fs::read_to_string(&backup) else {
            continue;
        };
        payload.backups.push(Backup {
            path: backup.display().to_string(),
            at: backup_taken_at(&backup),
            // A backup is redacted against its *own* content: a credential may have been
            // cleared since, and the mask has to follow the value, not the current file.
            text: redact_file(&text, catalog).unwrap_or_else(|_| String::new()),
        });
    }

    if !path.exists() {
        return Ok(payload);
    }
    payload.exists = true;

    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;

    match redact_file(&text, catalog) {
        Ok(redacted) => match flatten(&redacted, catalog) {
            Ok(flattened) => {
                payload.keys = flattened
                    .keys
                    .iter()
                    .map(|entry| entry.key.clone())
                    .collect();
                payload.keys.sort();
                payload.refusals = flattened.refusals;
                payload.text = redacted;
            }
            Err(error) => payload.error = Some(error.sentence()),
        },
        Err(error) => payload.error = Some(error.sentence()),
    }

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use serde_json::json;

    /// A catalog with the shapes that make flattening interesting: a nested path, a record
    /// whose children are not settings, a boolean, an array and a credential.
    const CATALOG: &str = r#"{
      "engineVersion": "18.2.6",
      "schemaSha256": "00",
      "sections": [ { "id": "general", "title": "General", "blurb": "b", "navGroup": "app", "icon": "sliders", "order": 1 } ],
      "keys": [
        { "key": "gc.coldArchiveAfterDays", "label": "Cold archive after days", "type": "number", "description": "",
          "values": [], "credential": false, "tab": "interaction", "group": "Housekeeping", "section": "general",
          "disposition": "curated", "control": "number", "restart": "sidecar" },
        { "key": "power.sleepPrevention", "label": "Sleep prevention", "type": "enum", "description": "",
          "values": ["off", "idle", "display", "system"], "credential": false, "tab": "interaction",
          "group": "Power", "section": "general", "disposition": "curated", "control": "select", "restart": "sidecar" },
        { "key": "magicKeywords.enabled", "label": "Magic keywords", "type": "boolean", "description": "",
          "values": [], "credential": false, "tab": "interaction", "group": "Magic Keywords", "section": "general",
          "disposition": "curated", "control": "toggle", "restart": "sidecar" },
        { "key": "enabledModels", "label": "Enabled models", "type": "array", "description": "",
          "values": [], "credential": false, "tab": "model", "group": "Roles & Selection", "section": "general",
          "disposition": "curated", "control": "list", "restart": "app" },
        { "key": "modelRoles", "label": "Model roles", "type": "record", "description": "",
          "values": [], "credential": false, "tab": "model", "group": "Roles & Selection", "section": "general",
          "disposition": "curated", "control": "record", "restart": "sidecar" },
        { "key": "tools.approval", "label": "Tool approval", "type": "record", "description": "",
          "values": [], "credential": true, "tab": "tools", "group": "Approval", "section": "general",
          "disposition": "hidden", "control": "record", "restart": "sidecar" },
        { "key": "steeringMode", "label": "Steering mode", "type": "enum", "description": "",
          "values": ["one-at-a-time", "all"], "credential": false, "tab": "interaction", "group": "Input",
          "section": "general", "disposition": "curated", "control": "select", "restart": "live" },
        { "key": "mnemopi.embeddingApiKey", "label": "Embedding API key", "type": "string", "description": "",
          "values": [], "credential": true, "tab": "memory", "group": "Mnemosyne", "section": "general",
          "disposition": "curated", "control": "secret", "restart": "sidecar" }
      ],
      "danger": []
    }"#;

    fn catalog() -> Catalog {
        Catalog::parse(CATALOG).expect("parse")
    }

    /// The shape the engine actually writes: nested by dotted key, sparse, and `0600`.
    const FILE: &str = "\
gc:
  coldArchiveAfterDays: 45
power:
  sleepPrevention: display
enabledModels:
  []
modelRoles:
  default: x/y
";

    #[test]
    fn a_sparse_file_flattens_by_its_dotted_keys() {
        let flattened = read(FILE, &catalog()).expect("parse");

        assert_eq!(
            flattened.map(),
            BTreeMap::from([
                ("gc.coldArchiveAfterDays".to_string(), json!(45)),
                ("power.sleepPrevention".to_string(), json!("display")),
                ("enabledModels".to_string(), json!([])),
                ("modelRoles".to_string(), json!({"default": "x/y"})),
            ]),
            "the file's nesting becomes the schema's dotted keys, and a record stays whole"
        );
        assert!(flattened.refusals.is_empty());
        assert!(flattened.sets("power.sleepPrevention"));
        assert!(!flattened.sets("magicKeywords.enabled"), "not in this file");
    }

    #[test]
    fn a_record_is_a_leaf_even_though_it_nests() {
        let flattened = read("tools:\n  approval:\n    bash: allow\n", &catalog()).expect("parse");
        assert_eq!(
            flattened.map(),
            BTreeMap::from([("tools.approval".to_string(), json!({"bash": "allow"}))]),
            "`tools.approval` is the setting; `bash` is part of its value"
        );
    }

    /// The trap this parser choice exists to avoid: YAML 1.1 reads `off` as a boolean, and the
    /// edit would then be refused as "not one of off, idle, display, system".
    #[test]
    fn a_word_that_yaml_11_would_read_as_a_boolean_stays_text() {
        let flattened = read("power:\n  sleepPrevention: off\n", &catalog()).expect("parse");
        assert_eq!(
            flattened.map().get("power.sleepPrevention"),
            Some(&json!("off")),
            "`off` is the string the user typed"
        );

        let plan = plan(
            &Flattened::default(),
            "power:\n  sleepPrevention: off\n",
            &catalog(),
        );
        assert!(plan.is_applicable(), "{:?}", plan.refusals);
        assert_eq!(plan.changes[0].after, Some(json!("off")));
    }

    #[test]
    fn an_alias_resolves_instead_of_becoming_a_refusal() {
        let flattened =
            read("base: &b 45\ngc:\n  coldArchiveAfterDays: *b\n", &catalog()).expect("parse");
        assert_eq!(
            flattened.map().get("gc.coldArchiveAfterDays"),
            Some(&json!(45)),
            "an anchored value reads as its value"
        );
        // `base` is not a setting, and the plan is where that is refused — not the reader.
        assert!(flattened.sets("base"));
    }

    #[test]
    fn a_broken_file_reports_where_it_broke() {
        let error =
            read("gc:\n coldArchiveAfterDays: 45\n  bad: 1\n", &catalog()).expect_err("broken");
        assert!(error.line.is_some(), "the parser's own position: {error:?}");
        assert!(
            error.sentence().starts_with("line "),
            "{}",
            error.sentence()
        );
    }

    #[test]
    fn a_file_that_is_not_a_mapping_is_refused_rather_than_ignored() {
        let flattened = read("- a\n- b\n", &catalog()).expect("parses");
        assert!(flattened.keys.is_empty());
        assert_eq!(flattened.refusals.len(), 1);
        assert!(flattened.refusals[0].reason.contains("not a mapping"));
    }

    #[test]
    fn a_mapping_key_that_is_not_text_is_refused_and_the_rest_still_reads() {
        let flattened =
            read("1: one\ngc:\n  coldArchiveAfterDays: 45\n", &catalog()).expect("parse");
        assert!(
            flattened.sets("gc.coldArchiveAfterDays"),
            "the readable half survives"
        );
        assert_eq!(flattened.refusals.len(), 1);
        assert!(flattened.refusals[0].reason.contains("not text"));
    }

    #[test]
    fn a_plan_sets_what_changed_and_resets_what_left() {
        let before = read(FILE, &catalog()).expect("parse");
        let edited = "\
gc:
  coldArchiveAfterDays: 60
power:
  sleepPrevention: display
modelRoles:
  default: x/y
";

        let plan = plan(&before, edited, &catalog());
        assert!(plan.is_applicable(), "{:?}", plan.refusals);
        assert_eq!(plan.changes.len(), 2);

        let changed = plan
            .changes
            .iter()
            .find(|c| c.key == "gc.coldArchiveAfterDays")
            .expect("changed");
        assert_eq!(changed.action, Action::Set);
        assert_eq!(changed.before, Some(json!(45)));
        assert_eq!(changed.after, Some(json!(60)));

        let dropped = plan
            .changes
            .iter()
            .find(|c| c.key == "enabledModels")
            .expect("dropped");
        assert_eq!(dropped.action, Action::Reset);
        assert_eq!(dropped.after, None);

        // A key that did not move is not a change at all.
        assert!(!plan
            .changes
            .iter()
            .any(|c| c.key == "power.sleepPrevention"));
    }

    #[test]
    fn a_plan_that_changes_nothing_is_empty() {
        let before = read(FILE, &catalog()).expect("parse");
        assert!(plan(&before, FILE, &catalog()).is_empty());
    }

    #[test]
    fn an_edit_is_refused_for_an_unknown_key() {
        let before = read(FILE, &catalog()).expect("parse");
        let plan = plan(&before, "something.new: 1\n", &catalog());

        assert!(!plan.is_applicable());
        assert_eq!(
            plan.changes.len(),
            4,
            "every key the file used to set is reset, even though the edit applies nothing"
        );
        let refusal = plan
            .refusals
            .iter()
            .find(|r| r.key == "something.new")
            .expect("refused");
        assert!(
            refusal.reason.contains("not in this build"),
            "{}",
            refusal.reason
        );
    }

    #[test]
    fn an_edit_is_refused_for_a_value_outside_the_enum() {
        let before = Flattened::default();
        let plan = plan(
            &before,
            "power:\n  sleepPrevention: sometimes\n",
            &catalog(),
        );

        assert!(!plan.is_applicable());
        let refusal = &plan.refusals[0];
        assert_eq!(refusal.key, "power.sleepPrevention");
        assert!(
            refusal.reason.contains("off, idle, display, system"),
            "{}",
            refusal.reason
        );
    }

    #[test]
    fn an_edit_is_refused_for_a_value_of_the_wrong_shape() {
        let before = Flattened::default();
        let plan = plan(&before, "gc:\n  coldArchiveAfterDays: soon\n", &catalog());

        assert!(!plan.is_applicable());
        assert!(
            plan.refusals[0].reason.contains("takes a number"),
            "{:?}",
            plan.refusals
        );
    }

    /// A secret cannot be written by this editor: the rules that keep stored credentials out of
    /// the app's reach would not survive a text pane that writes them back.
    #[test]
    fn an_edit_is_refused_for_a_credential() {
        let before = Flattened::default();
        let plan = plan(
            &before,
            "tools:\n  approval:\n    bash: allow\n",
            &catalog(),
        );

        assert!(!plan.is_applicable());
        assert!(
            plan.refusals[0].reason.contains("credential"),
            "{:?}",
            plan.refusals
        );
    }

    #[test]
    fn every_change_carries_what_it_will_cost() {
        let before = Flattened::default();
        let plan = plan(
            &before,
            "steeringMode: all\ngc:\n  coldArchiveAfterDays: 45\n",
            &catalog(),
        );

        assert_eq!(
            plan.changes
                .iter()
                .find(|c| c.key == "steeringMode")
                .map(|c| c.restart),
            Some(Restart::Live)
        );
        assert_eq!(
            plan.changes
                .iter()
                .find(|c| c.key == "gc.coldArchiveAfterDays")
                .map(|c| c.restart),
            Some(Restart::Sidecar)
        );
    }

    /// The rule the hatch exists to keep: a credential is in the file in plain text, and the
    /// pane must never be the thing that shows it.
    #[test]
    fn a_credential_value_is_masked_out_of_the_text() {
        let text = "mnemopi:\n  embeddingApiKey: sk-live-abcdef123456\n";
        let redacted = redact_file(text, &catalog()).expect("parses");

        assert!(
            !redacted.contains("sk-live-abcdef123456"),
            "the value is gone: {redacted}"
        );
        assert!(redacted.contains(MASK), "{redacted}");
        assert!(
            redacted.contains("embeddingApiKey"),
            "the key stays: {redacted}"
        );
    }

    /// A key the catalog has never seen — after an engine bump, or one a user invented — is
    /// masked by its name, because a leak is not something a later fix can undo.
    #[test]
    fn an_unknown_key_that_reads_like_a_credential_is_masked_too() {
        let text = "hindsight:\n  apiToken: tok_abcdefgh12345\n";
        let redacted = redact_file(text, &catalog()).expect("parses");
        assert!(!redacted.contains("tok_abcdefgh12345"), "{redacted}");
    }

    /// A value too short to substitute safely takes its line with it rather than being
    /// replaced everywhere in the file.
    #[test]
    fn a_short_secret_masks_its_whole_line() {
        let redacted = redact("a: abc\nother: abc\n", &["abc".to_string()]);
        assert_eq!(redacted, format!("a: {MASK}\nother: {MASK}"));
        assert!(!redacted.contains("abc"));
    }

    /// And the mask is the *baseline*, so an untouched credential line is not a change.
    #[test]
    fn an_untouched_credential_is_not_a_change() {
        let text = "mnemopi:\n  embeddingApiKey: sk-live-abcdef123456\n";
        let redacted = redact_file(text, &catalog()).expect("parses");
        let before = read(&redacted, &catalog()).expect("parses");

        let untouched = plan(&before, &redacted, &catalog());
        assert!(untouched.is_empty(), "{:?}", untouched.refusals);

        // But an *edit* to it is refused: this editor does not write secrets.
        let edited = "mnemopi:\n  embeddingApiKey: sk-live-somethingelse\n".to_string();
        let changed = plan(&before, &edited, &catalog());
        assert!(!changed.is_applicable());
        assert!(
            changed
                .refusals
                .iter()
                .any(|refusal| refusal.reason.contains("credential")),
            "{:?}",
            changed.refusals
        );
    }

    #[test]
    fn opening_a_file_that_does_not_exist_is_not_an_error() {
        let path =
            std::env::temp_dir().join(format!("omp-settings-absent-{}.yml", std::process::id()));
        let payload = open(&path, &catalog()).expect("open");

        assert!(!payload.exists);
        assert!(payload.text.is_empty());
        assert!(payload.error.is_none());
        assert!(payload.keys.is_empty());
    }

    /// A file the app cannot parse is one whose secrets it cannot find, so it is not shown at
    /// all: the error is what the pane has to offer.
    #[test]
    fn opening_a_file_it_cannot_parse_shows_the_error_and_no_text() {
        let dir = std::env::temp_dir().join(format!("omp-settings-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("config.yml");
        std::fs::write(&path, "gc:\n coldArchiveAfterDays: 45\n  bad: 1\n").expect("write");

        let payload = open(&path, &catalog()).expect("open");
        assert!(payload.exists);
        assert!(payload.text.is_empty(), "nothing is shown until it parses");
        assert!(payload
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("line ")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_a_file_carries_its_backups_newest_first_with_their_own_redaction() {
        let dir = std::env::temp_dir().join(format!("omp-settings-backups-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let path = dir.join("config.yml");
        std::fs::write(&path, "gc:\n  coldArchiveAfterDays: 45\n").expect("write");
        backup(&path).expect("first");
        std::fs::write(&path, "mnemopi:\n  embeddingApiKey: sk-live-abcdef123456\n")
            .expect("write");
        let newest = backup(&path).expect("second");

        let payload = open(&path, &catalog()).expect("open");
        assert_eq!(payload.backups.len(), 2);
        assert_eq!(
            payload.backups[0].path,
            newest.display().to_string(),
            "newest first"
        );
        assert!(
            payload.backups[0].at.is_some(),
            "the name carries when it was taken"
        );
        assert!(
            !payload.backups[0].text.contains("sk-live-abcdef123456"),
            "a backup's own secrets are masked in it too"
        );

        // The live file's own keys are listed, and a backup is not mistaken for the file.
        assert!(payload
            .keys
            .contains(&"mnemopi.embeddingApiKey".to_string()));
        assert!(
            payload.text.contains("embeddingApiKey") && !payload.text.contains("sk-live"),
            "the pane shows the live file, not a backup — and not its secret: {}",
            payload.text
        );
        assert!(
            payload.backups[1].text.contains("coldArchiveAfterDays"),
            "the older backup is still the older file"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `docs/13` protection 3, tested without an engine: the engine wrote to the file between
    /// the preview the user is looking at and the apply.
    #[tokio::test]
    async fn an_apply_refuses_a_change_whose_key_moved_under_it() {
        let catalog = catalog();
        let cli = crate::engine::Cli::new();

        // The user saw `45` and edited it to `60`.
        let before = read("gc:\n  coldArchiveAfterDays: 45\n", &catalog).expect("parse");
        let plan = plan(&before, "gc:\n  coldArchiveAfterDays: 60\n", &catalog);
        assert_eq!(plan.changes.len(), 1);

        // But something else — the engine, recording an approval — rewrote the file first.
        let current = read("gc:\n  coldArchiveAfterDays: 30\n", &catalog).expect("parse");

        let report = crate::apply::apply_plan(&cli, &catalog, &plan, &current, None)
            .await
            .expect("a refusal is not a failure");

        assert!(report.changes.is_empty(), "nothing was written");
        assert_eq!(report.refused.len(), 1);
        assert!(
            report.refused[0]
                .reason
                .contains("changed while this edit was open"),
            "{}",
            report.refused[0].reason
        );
        assert!(
            report.refused[0].reason.contains("30"),
            "{}",
            report.refused[0].reason
        );
    }

    /// The same hazard from the other side: the key the edit would clear is already gone.
    #[tokio::test]
    async fn an_apply_refuses_a_reset_whose_key_is_already_gone() {
        let catalog = catalog();
        let cli = crate::engine::Cli::new();

        let before = read("gc:\n  coldArchiveAfterDays: 45\n", &catalog).expect("parse");
        let plan = plan(&before, "", &catalog);
        assert_eq!(plan.changes.len(), 1, "the edit drops the key");

        let report = crate::apply::apply_plan(&cli, &catalog, &plan, &Flattened::default(), None)
            .await
            .expect("a refusal is not a failure");

        assert!(report.changes.is_empty());
        assert!(
            report.refused[0].reason.contains("no longer set"),
            "{:?}",
            report.refused
        );
    }

    /// And a per-key compare-and-swap does not invalidate an unrelated edit, which is the whole
    /// reason it is per key: the engine recording one key must not block a plan that touches
    /// another. The comparison is like for like — a plan's `before` is the file's own value —
    /// so an untouched key passes it. (The write that follows a passing guard is
    /// `tests/write.rs`'s subject.)
    #[test]
    fn the_guard_compares_the_files_own_value() {
        let catalog = catalog();
        let before = read("gc:\n  coldArchiveAfterDays: 45\n", &catalog).expect("parse");
        let plan = plan(&before, "gc:\n  coldArchiveAfterDays: 60\n", &catalog);

        assert_eq!(
            before.map().get("gc.coldArchiveAfterDays"),
            plan.changes[0].before.as_ref(),
            "an untouched file passes the compare-and-swap"
        );
        assert_ne!(
            before.map().get("gc.coldArchiveAfterDays"),
            plan.changes[0].after.as_ref(),
            "and the change is a real one"
        );
    }
}
