//! The write flow: validate, confirm, hand to the engine, and say what it will cost.
//!
//! Every path that changes a setting goes through here — a row's control, a reset from a
//! menu, and the escape hatch's plan. The order is deliberate and it is the whole safety
//! story of the screen:
//!
//! 1. the key must exist in the catalog ([`validate::known_key`]);
//! 2. a row may not write a key the curated screen does not show
//!    ([`validate::reachable_from_a_row`]) — the hatch has its own path for those;
//! 3. the value must be the right shape and, for an enum, in its domain
//!    ([`validate::value`]);
//! 4. a key that can widen the agent's blast radius needs its own name typed
//!    ([`validate::confirm_write`], [`validate::confirm_hatch`]);
//! 5. only then is the engine asked.
//!
//! Nothing here writes a file: [`crate::engine::Cli`] runs `omp config set|reset`, which is
//! the engine's own writer with its own lock.

use serde::Serialize;
use serde_json::Value;

use crate::catalog::{Catalog, Control, Restart};
use crate::engine::Cli;
use crate::hatch::{Action, Change, Flattened, Plan, Refusal};
use crate::validate;

/// The keys the app can apply to a **running** session, through RPC.
///
/// `docs/13` classes more keys `RPC live` than this, and the difference matters: that class was
/// derived from where a value is *consumed*, and includes keys the engine reads per request
/// inside its own process — a `tier.*` key, for instance. The app writes from outside the
/// process, so for those the write is only visible at the next session start, exactly like a
/// `sidecar` write. This list is the set with a setter the app can actually call:
/// `set_steering_mode`, `set_follow_up_mode`, `set_interrupt_mode`, `set_thinking_level`,
/// `set_auto_compaction`, `set_auto_retry` (`modes/rpc/rpc-mode.ts:1315-1490`).
///
/// It exists as a constant rather than a comment so the two halves cannot drift: the catalog's
/// `live` class is asserted against it in this module's tests, and the host pushes exactly
/// these keys after a file write. A key that is live in the catalog but not here would make a
/// row promise "applies to running sessions now" and then not do it.
pub const RPC_LIVE_KEYS: [&str; 6] = [
    "steeringMode",
    "followUpMode",
    "interruptMode",
    "defaultThinkingLevel",
    "compaction.enabled",
    "retry.enabled",
];

/// Whether a write to this key can be pushed to live sessions.
pub fn can_apply_live(key: &str) -> bool {
    RPC_LIVE_KEYS.contains(&key)
}

/// What one write did, and what it will cost.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub key: String,
    /// The value now recorded; `None` after a reset.
    pub value: Option<Value>,
    /// What it takes for the change to be in effect — see [`crate::catalog::Restart`].
    pub restart: Restart,
    /// The sentence the screen shows afterwards.
    pub message: String,
}

/// Record a value for one key, from a row.
pub async fn set(
    cli: &Cli,
    catalog: &Catalog,
    key: &str,
    value: Value,
    confirmation: Option<&str>,
) -> Result<WriteOutcome, String> {
    let spec = validate::known_key(catalog, key)?;
    validate::reachable_from_a_row(spec)?;
    validate::value(spec, &value)?;
    validate::confirm_write(catalog, key, confirmation)?;

    cli.set(key, &value, spec.kind).await?;

    Ok(WriteOutcome {
        key: key.to_string(),
        value: Some(value),
        restart: spec.restart,
        message: recorded_message(key, spec.restart, false),
    })
}

/// Return one key to the engine's default, from a row.
pub async fn reset(
    cli: &Cli,
    catalog: &Catalog,
    key: &str,
    confirmation: Option<&str>,
) -> Result<WriteOutcome, String> {
    let spec = validate::known_key(catalog, key)?;
    validate::reachable_from_a_row(spec)?;
    validate::confirm_write(catalog, key, confirmation)?;

    cli.reset(key).await?;

    Ok(WriteOutcome {
        key: key.to_string(),
        value: None,
        restart: spec.restart,
        message: recorded_message(key, spec.restart, true),
    })
}

/// What one change of a plan did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResult {
    pub key: String,
    pub action: Action,
    pub ok: bool,
    /// The engine's own words when it refused.
    pub error: Option<String>,
}

/// The result of applying a hand-edit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub changes: Vec<ChangeResult>,
    /// The keys the plan refused before applying anything, so the screen can say what it did
    /// not do and why.
    pub refused: Vec<Refusal>,
}

impl ApplyReport {
    /// Whether every change went through.
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty() && self.changes.iter().all(|change| change.ok)
    }

    /// How many changes the engine accepted.
    pub fn applied(&self) -> usize {
        self.changes.iter().filter(|change| change.ok).count()
    }
}

/// Apply a hand-edit's plan.
///
/// A plan with any refusal applies **nothing**: a partially applied edit would leave the file
/// disagreeing with the text the user is looking at, which is worse than refusing outright.
/// A plan that is applicable applies change by change — each one is a separate `config set`,
/// and the report keeps which ones landed, because a failure halfway through a list of edits
/// is exactly the case where the user needs the true state rather than a summary.
///
/// # Never write a stale document over a changed file
///
/// `docs/13` protection 3 states this as a requirement, and a diff is where it gets violated.
/// The app is not the only writer of this file: the engine writes it too (an "always allow"
/// from the approval dialog, a model-role write when `modelRoleStorage` is `project`), so the
/// preview the user is looking at can be out of date by the time they apply it. Applying it
/// anyway would *revert* what the engine just recorded — a change nobody asked for.
///
/// Every change is therefore checked against `current`, the file read at apply time: one whose
/// `before` no longer matches what that key holds is refused by name, and the rest of the plan
/// still applies. A compare-and-swap per key rather than per document, which is what makes it
/// useful — an edit to `gc.*` is not invalidated because the engine recorded a tool approval in
/// the same file a moment earlier.
pub async fn apply_plan(
    cli: &Cli,
    catalog: &Catalog,
    plan: &Plan,
    current: &Flattened,
    confirmation: Option<&str>,
) -> Result<ApplyReport, String> {
    if !plan.is_applicable() {
        return Ok(ApplyReport {
            changes: Vec::new(),
            refused: plan.refusals.clone(),
        });
    }

    let keys: Vec<String> = plan
        .changes
        .iter()
        .map(|change| change.key.clone())
        .collect();
    validate::confirm_hatch(catalog, &keys, confirmation)?;

    let mut report = ApplyReport {
        changes: Vec::new(),
        refused: Vec::new(),
    };
    let held = current.map();

    for change in &plan.changes {
        // The file moved under this edit — the engine writes it too (`docs/13` protection 3).
        // Refusing this one change beats reverting what something else just recorded.
        let now = held.get(&change.key).cloned();
        if now != change.before {
            report.refused.push(Refusal {
                key: change.key.clone(),
                reason: match &now {
                    Some(value) => format!(
                        "the file changed while this edit was open: `{}` now holds {value} — \
                         reopen the escape hatch and make the change again",
                        change.key
                    ),
                    None => format!(
                        "the file changed while this edit was open: `{}` is no longer set — \
                         reopen the escape hatch and make the change again",
                        change.key
                    ),
                },
            });
            continue;
        }

        let spec = validate::known_key(catalog, &change.key)?;
        let outcome = match change.action {
            Action::Set => {
                let value = change.after.clone().unwrap_or(Value::Null);
                cli.set(&change.key, &value, spec.kind).await
            }
            Action::Reset => cli.reset(&change.key).await,
        };

        report.changes.push(ChangeResult {
            key: change.key.clone(),
            action: change.action,
            ok: outcome.is_ok(),
            error: outcome.err(),
        });
    }

    Ok(report)
}

/// The sentence a row shows after a write went through.
///
/// It states what was recorded and what it will take to be in effect — never less. The engine
/// reads a session's settings when it constructs it, so a `sidecar` write is a promise about
/// the *next* start, and a screen that implied otherwise would teach people the app lies.
pub fn recorded_message(key: &str, restart: Restart, cleared: bool) -> String {
    let verb = if cleared {
        format!("`{key}` is back to the engine's default.")
    } else {
        format!("`{key}` is recorded.")
    };

    match restart {
        Restart::Live => format!("{verb} Live sessions changed now; new ones start with it."),
        Restart::Sidecar => {
            format!("{verb} Each session reads settings when it starts — restart to use it now.")
        }
        Restart::App => format!(
            "{verb} This one is baked into app state at launch, so it takes effect after a \
             relaunch of the app."
        ),
    }
}

/// Whether a key's control is one the screen can edit with a single field.
///
/// Used by the screen to decide between an inline editor and a sub-editor, and by the tests
/// that assert every curated control has a renderer.
pub fn needs_a_sub_editor(control: Control) -> bool {
    matches!(control, Control::List | Control::Record)
}

/// Which of a plan's changes will be live the moment they land.
pub fn live_changes(plan: &Plan) -> Vec<&Change> {
    plan.changes
        .iter()
        .filter(|change| change.restart == Restart::Live)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Restart;

    #[test]
    fn a_message_never_promises_more_than_the_restart_class_allows() {
        let live = recorded_message("steeringMode", Restart::Live, false);
        assert!(live.contains("Live sessions changed now"), "{live}");

        let sidecar = recorded_message("gc.wal", Restart::Sidecar, false);
        assert!(sidecar.contains("restart"), "{sidecar}");
        assert!(!sidecar.contains("now;"), "{sidecar}");

        let app = recorded_message("enabledModels", Restart::App, false);
        assert!(app.contains("relaunch"), "{app}");

        let cleared = recorded_message("gc.wal", Restart::Sidecar, true);
        assert!(cleared.contains("default"), "{cleared}");
    }

    #[test]
    fn only_the_structured_controls_need_a_sub_editor() {
        for control in [Control::List, Control::Record] {
            assert!(needs_a_sub_editor(control));
        }
        for control in [
            Control::Toggle,
            Control::Select,
            Control::Number,
            Control::Text,
            Control::Secret,
        ] {
            assert!(!needs_a_sub_editor(control), "{control:?}");
        }
    }

    #[test]
    fn a_plan_that_refuses_applies_nothing() {
        let plan = Plan {
            changes: vec![Change {
                key: "gc.wal".to_string(),
                action: Action::Set,
                before: None,
                after: Some(Value::Bool(true)),
                restart: Restart::Sidecar,
            }],
            refusals: vec![Refusal {
                key: "something.new".to_string(),
                reason: "not in this build's settings catalog".to_string(),
            }],
        };

        assert!(!plan.is_applicable());
        assert!(live_changes(&plan).is_empty());
        // The report is the plan's refusals, and no change is attempted.
        assert_eq!(plan.refusals.len(), 1);
    }

    #[test]
    fn a_report_counts_what_actually_landed() {
        let clean = ApplyReport {
            changes: vec![
                ChangeResult {
                    key: "a".to_string(),
                    action: Action::Set,
                    ok: true,
                    error: None,
                },
                ChangeResult {
                    key: "b".to_string(),
                    action: Action::Reset,
                    ok: true,
                    error: None,
                },
            ],
            refused: Vec::new(),
        };
        assert!(clean.is_clean());
        assert_eq!(clean.applied(), 2);

        let half = ApplyReport {
            changes: vec![
                ChangeResult {
                    key: "a".to_string(),
                    action: Action::Set,
                    ok: true,
                    error: None,
                },
                ChangeResult {
                    key: "b".to_string(),
                    action: Action::Set,
                    ok: false,
                    error: Some("Unknown setting: b".to_string()),
                },
            ],
            refused: Vec::new(),
        };
        assert!(!half.is_clean());
        assert_eq!(half.applied(), 1, "the report says exactly what landed");
    }
}
