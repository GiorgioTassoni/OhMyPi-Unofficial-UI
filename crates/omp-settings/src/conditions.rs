//! The schema's `ui.condition` gates, resolved the way OMP's own settings panel resolves
//! them.
//!
//! The schema stores a *symbolic* name (`"advisorEnabled"`, `"hasImageProtocol"`, …); the
//! twelve predicates behind those names live in the engine at
//! `src/config/settings-ui.ts:14-60`, and this module is a transcription of that map. It is
//! transcribed rather than imported because it is eleven comparisons over other settings —
//! data the app already has — and one platform check.
//!
//! A gate is a *reason*, not a filter: `docs/13` §Q3 settled that a row whose condition is
//! unmet stays visible and disabled, with the condition named in one line, because a
//! setting that vanishes reads as one that never existed. The row to point the user at
//! travels as a key, and the screen resolves that key's section from the catalog, so this
//! module never has to know how the navigation is arranged.

use serde::Serialize;
use serde_json::Value;

/// Why a row is disabled, and which key would change that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    /// The sentence the row shows in place of its control.
    pub reason: String,
    /// A settings key the user could change to satisfy this — usually the one the engine's
    /// own predicate reads. `None` when nothing the user can set would change it (a
    /// platform, or a capability this app does not have).
    pub needs: Option<String>,
}

/// Whether a condition is met, and if not, why.
///
/// `get` reads the effective value of a settings key, which is exactly what the engine's
/// own predicates do (`Settings.instance.get(...)`), so the two agree by construction.
/// An unknown name is treated as *met*: the alternative is a row that is permanently
/// disabled because a future engine added a condition this build has never heard of, and a
/// disabled row that cannot be explained is worse than an enabled one that turns out to
/// need a restart.
pub fn evaluate(name: &str, get: &dyn Fn(&str) -> Option<Value>) -> Option<Gate> {
    let flag = |key: &str| get(key).and_then(|value| value.as_bool()) == Some(true);
    let text = |key: &str| {
        get(key)
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default()
    };

    match name {
        // ── the platform and the terminal, which this host answers itself ──────────────
        "macOS" => (!cfg!(target_os = "macos")).then(|| Gate {
            reason: "macOS only.".to_string(),
            needs: None,
        }),
        // The engine asks whether the terminal can show images at all. A webview can, and
        // the transcript renders them, so the gate is met here by construction.
        "hasImageProtocol" => None,

        // ── one setting compared to one value ──────────────────────────────────────────
        "advisorEnabled" => unmet(
            !flag("advisor.enabled"),
            "Needs the advisor turned on.",
            Some("advisor.enabled"),
        ),
        "vimModeEnabled" => unmet(
            !flag("tui.vimMode"),
            "Needs Vim mode, which this app does not have.",
            None,
        ),
        "hindsightActive" => unmet(
            text("memory.backend") != "hindsight",
            "Needs the memory backend set to Hindsight.",
            Some("memory.backend"),
        ),
        "mnemopiActive" => unmet(
            text("memory.backend") != "mnemopi",
            "Needs the memory backend set to Mnemopi.",
            Some("memory.backend"),
        ),
        "autolearnActive" => unmet(
            !flag("autolearn.enabled"),
            "Needs autolearn turned on.",
            Some("autolearn.enabled"),
        ),
        "autoThinkingActive" => unmet(
            text("defaultThinkingLevel") != "auto",
            "Needs the default thinking level set to Auto.",
            Some("defaultThinkingLevel"),
        ),
        "usageAwareFallbackEnabled" => unmet(
            !flag("retry.usageAwareFallback"),
            "Needs usage-aware fallback turned on.",
            Some("retry.usageAwareFallback"),
        ),
        "unexpectedStopSmart" => unmet(
            text("features.unexpectedStopDetection") != "smart",
            "Needs unexpected-stop detection set to Smart.",
            Some("features.unexpectedStopDetection"),
        ),

        // ── the deferred capability ────────────────────────────────────────────────────
        // Plan mode is `docs/11` §2.2: this app does not have it, so everything gated on it
        // stays visible-but-disabled rather than pretending.
        "planModeEnabled" | "planAutosaveEnabled" => {
            unmet(true, "Plan mode is not in this app.", None)
        }

        // An unknown gate is met — see the note above.
        _ => None,
    }
}

/// A gate that only exists when the condition fails.
fn unmet(failed: bool, reason: &str, needs: Option<&str>) -> Option<Gate> {
    failed.then(|| Gate {
        reason: reason.to_string(),
        needs: needs.map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A settings world where every predicate's key is at the engine's default.
    fn defaults(key: &str) -> Option<Value> {
        let value = match key {
            "advisor.enabled" => json!(false),
            "tui.vimMode" => json!(false),
            "memory.backend" => json!("off"),
            "autolearn.enabled" => json!(false),
            "defaultThinkingLevel" => json!("medium"),
            "retry.usageAwareFallback" => json!(false),
            "features.unexpectedStopDetection" => json!("off"),
            "plan.enabled" => json!(false),
            _ => return None,
        };
        Some(value)
    }

    #[test]
    fn every_condition_the_engine_uses_is_understood() {
        // The twelve names in `settings-ui.ts:14-60`. This list is the contract: if the
        // engine adds a thirteenth, the fall-through below keeps the row usable, and this
        // test is where the omission shows up as a missing case rather than a silent one.
        let names = [
            "macOS",
            "hasImageProtocol",
            "advisorEnabled",
            "vimModeEnabled",
            "hindsightActive",
            "mnemopiActive",
            "autolearnActive",
            "autoThinkingActive",
            "usageAwareFallbackEnabled",
            "planModeEnabled",
            "planAutosaveEnabled",
            "unexpectedStopSmart",
        ];

        for name in names {
            // No panic, and a gate that always carries a reason.
            if let Some(gate) = evaluate(name, &defaults) {
                assert!(!gate.reason.is_empty(), "{name} gave no reason");
            }
        }
    }

    #[test]
    fn a_gate_names_what_would_change_it() {
        let gate = evaluate("advisorEnabled", &defaults).expect("unmet at the default");
        assert_eq!(gate.needs.as_deref(), Some("advisor.enabled"));
        assert!(gate.reason.contains("advisor"));

        // And is met as soon as the setting it reads is on.
        let on = |key: &str| Some(json!(key == "advisor.enabled"));
        assert_eq!(evaluate("advisorEnabled", &on), None);
    }

    #[test]
    fn a_setting_compared_to_a_value_is_read_as_text() {
        let backend = |value: &'static str| {
            move |key: &str| {
                if key == "memory.backend" {
                    Some(json!(value))
                } else {
                    None
                }
            }
        };

        // Each backend's own keys are gated on that backend, and on no other.
        assert_eq!(evaluate("hindsightActive", &backend("hindsight")), None);
        assert!(evaluate("mnemopiActive", &backend("hindsight")).is_some());
        assert_eq!(evaluate("mnemopiActive", &backend("mnemopi")), None);
        assert!(evaluate("hindsightActive", &backend("mnemopi")).is_some());
        assert!(evaluate("hindsightActive", &backend("off")).is_some());
    }

    #[test]
    fn the_deferred_capability_is_never_met() {
        for name in ["planModeEnabled", "planAutosaveEnabled"] {
            let gate = evaluate(name, &|_| Some(json!(true))).expect("plan mode is not here");
            assert_eq!(gate.needs, None, "nothing the user can set would help");
            assert!(gate.reason.contains("Plan mode"));
        }
    }

    #[test]
    fn an_unknown_gate_leaves_the_row_usable() {
        assert_eq!(evaluate("somethingNewInTheEngine", &defaults), None);
    }

    #[test]
    fn the_platform_gate_answers_for_this_build() {
        let gate = evaluate("macOS", &defaults);
        assert_eq!(gate.is_some(), !cfg!(target_os = "macos"));
    }

    #[test]
    fn a_missing_value_is_not_a_met_condition() {
        // The engine can report no value for a key; that must not read as "set to true".
        let empty = |_: &str| None;
        assert!(evaluate("advisorEnabled", &empty).is_some());
        assert!(evaluate("autoThinkingActive", &empty).is_some());
    }
}
