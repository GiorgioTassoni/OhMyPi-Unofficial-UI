//! What may be written, and how loudly it has to be confirmed.
//!
//! Every write the app makes goes through here first, so a typo is refused with a sentence
//! the user reads rather than a config file the engine starts ignoring. The engine has its
//! own validation and this is not a substitute for it — it is the half that can name the
//! *setting* instead of the line, and it catches the case the engine cannot see: a value
//! that is the right type but the wrong domain, sent by a row whose enum came from a stale
//! catalog.

use serde_json::Value;

use crate::catalog::{Catalog, DangerLevel, KeySpec, SettingType};

/// The one-word confirmation a hand-edit of the config file needs when the plan touches a
/// key that can widen the agent's blast radius (`docs/13` §Q7).
pub const HATCH_CONFIRMATION: &str = "confirm";

/// Whether a value may be written for `spec`.
///
/// The message is written for the row that sent it: it names the key and what the key
/// takes. `reset` is the way to clear a key, so `null` is refused rather than treated as
/// one.
pub fn value(spec: &KeySpec, value: &Value) -> Result<(), String> {
    match spec.kind {
        SettingType::Boolean => want(value.is_boolean(), spec, "true or false"),
        SettingType::Number => want(value.is_number(), spec, "a number"),
        SettingType::String => want(value.is_string(), spec, "text"),
        SettingType::Array => want(value.is_array(), spec, "a list"),
        SettingType::Record => want(value.is_object(), spec, "a record"),
        SettingType::Enum => match value.as_str() {
            None => Err(describe(spec, "one of its values")),
            Some(text) if spec.values.iter().any(|allowed| allowed == text) => Ok(()),
            Some(text) => Err(format!(
                "`{}` does not take {text:?} — one of: {}",
                spec.key,
                spec.values.join(", ")
            )),
        },
    }
}

/// The message for a value of the wrong shape.
fn want(ok: bool, spec: &KeySpec, expected: &str) -> Result<(), String> {
    if ok {
        return Ok(());
    }
    Err(describe(spec, expected))
}

/// The sentence a refused write shows.
fn describe(spec: &KeySpec, expected: &str) -> String {
    format!("`{}` takes {expected}.", spec.key)
}

/// Refuse a write to a key the catalog does not know.
///
/// Drift, not a typo: the engine has a setting this build has never seen. The escape hatch
/// is where such a key belongs, and the message says so.
pub fn known_key<'a>(catalog: &'a Catalog, key: &str) -> Result<&'a KeySpec, String> {
    catalog.spec(key).ok_or_else(|| {
        format!(
            "this build's settings catalog has no `{key}` — the engine has a setting it does not know. \
             Use the raw config file for keys like this one."
        )
    })
}

/// Refuse a write to a key that is not reachable from the curated screen.
///
/// `deferred` keys belong to a capability this app does not have and `hidden` ones are
/// internal or unsafe — the curated screen renders neither, so a write to one could only
/// come from a stale row. The escape hatch has its own path, and its own confirmation.
pub fn reachable_from_a_row(spec: &KeySpec) -> Result<(), String> {
    match spec.disposition {
        crate::catalog::Disposition::Curated => Ok(()),
        crate::catalog::Disposition::Deferred => Err(format!(
            "`{}` belongs to a capability this app does not have — write it in the raw config file if you need it.",
            spec.key
        )),
        crate::catalog::Disposition::Hidden => Err(format!(
            "`{}` is not surfaced by the app — write it in the raw config file if you need it.",
            spec.key
        )),
    }
}

/// Enforce the typed confirmation a dangerous key needs.
///
/// The check lives here rather than in the dialog because a dialog is a courtesy: this is
/// the only path to the engine, and a key that can turn every approval off in one click has
/// to be hard to write by accident, not hard to *try* to write.
pub fn confirm_write(
    catalog: &Catalog,
    key: &str,
    confirmation: Option<&str>,
) -> Result<(), String> {
    let Some(entry) = catalog.danger(key) else {
        return Ok(());
    };
    if entry.level != DangerLevel::Confirm {
        return Ok(());
    }

    if confirmation.map(str::trim) == Some(key) {
        return Ok(());
    }

    Err(format!(
        "`{key}` changes what the agent may do and cannot be set from a row without confirmation: {}\n  \
         Type the setting's name (`{key}`) to confirm.",
        entry.why
    ))
}

/// Enforce the confirmation a hand-edit needs, naming the keys that forced it.
pub fn confirm_hatch(
    catalog: &Catalog,
    keys: &[String],
    confirmation: Option<&str>,
) -> Result<(), String> {
    let dangerous: Vec<&str> = keys
        .iter()
        .filter(|key| {
            catalog
                .danger(key)
                .is_some_and(|entry| entry.level == DangerLevel::Confirm)
        })
        .map(String::as_str)
        .collect();

    if dangerous.is_empty() {
        return Ok(());
    }

    if confirmation
        .map(str::trim)
        .is_some_and(|typed| typed.eq_ignore_ascii_case(HATCH_CONFIRMATION))
    {
        return Ok(());
    }

    let mut listed = dangerous.join(", ");
    if dangerous.len() == 1 {
        listed = dangerous[0].to_string();
    }

    Err(format!(
        "This change writes {listed}, which the app does not surface because it is a way to widen what the agent may do or where data goes.\n  \
         Type `{HATCH_CONFIRMATION}` to apply it anyway."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use serde_json::json;

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
        { "key": "modelRoles", "label": "Model roles", "type": "record", "description": "",
          "values": [], "credential": false, "tab": "model", "group": "Roles & Selection", "section": "general",
          "disposition": "curated", "control": "record", "restart": "sidecar" },
        { "key": "enabledModels", "label": "Enabled models", "type": "array", "description": "",
          "values": [], "credential": false, "tab": "model", "group": "Roles & Selection", "section": "general",
          "disposition": "curated", "control": "list", "restart": "app" },
        { "key": "bash.patterns", "label": "Bash patterns", "type": "array", "description": "",
          "values": [], "credential": false, "tab": "tools", "group": "Bash", "section": "general",
          "disposition": "hidden", "control": "list", "restart": "sidecar" },
        { "key": "tools.format", "label": "Tool format", "type": "string", "description": "",
          "values": ["text", "json"], "credential": false, "tab": "tools", "group": "Tool Exposure",
          "section": "general", "disposition": "deferred", "control": "select", "restart": "sidecar" }
      ],
      "danger": [
        { "key": "bash.patterns", "level": "confirm", "why": "a loose pattern approves commands without a dialog" },
        { "key": "tools.format", "level": "warn", "why": "deferred" }
      ]
    }"#;

    fn catalog() -> Catalog {
        Catalog::parse(CATALOG).expect("parse")
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_refused_by_name() {
        let catalog = catalog();
        let number = catalog.spec("gc.coldArchiveAfterDays").expect("the key");
        assert_eq!(value(number, &json!(45)), Ok(()));
        assert!(value(number, &json!("45"))
            .expect_err("text is not a number")
            .contains("a number"));

        let flag = catalog.spec("magicKeywords.enabled").expect("the key");
        assert_eq!(value(flag, &json!(true)), Ok(()));
        assert!(value(flag, &json!(1)).is_err());

        let record = catalog.spec("modelRoles").expect("the key");
        assert_eq!(value(record, &json!({"default": "x/y"})), Ok(()));
        assert!(value(record, &json!(["x"])).is_err());

        let list = catalog.spec("enabledModels").expect("the key");
        assert_eq!(value(list, &json!(["a"])), Ok(()));
        assert!(value(list, &json!("a")).is_err());
    }

    #[test]
    fn an_enum_is_checked_against_its_own_domain() {
        let catalog = catalog();
        let spec = catalog.spec("power.sleepPrevention").expect("the key");

        assert_eq!(value(spec, &json!("display")), Ok(()));

        let error = value(spec, &json!("sometimes")).expect_err("not a value");
        assert!(error.contains("off, idle, display, system"), "{error}");
    }

    #[test]
    fn null_is_refused_because_reset_is_the_way_to_clear_a_key() {
        let catalog = catalog();
        let spec = catalog.spec("enabledModels").expect("the key");
        assert!(value(spec, &Value::Null).is_err());
    }

    #[test]
    fn an_unknown_key_is_drift_and_says_where_to_go() {
        let catalog = catalog();
        let error = known_key(&catalog, "something.new").expect_err("drift");
        assert!(error.contains("raw config file"), "{error}");
        assert!(known_key(&catalog, "enabledModels").is_ok());
    }

    #[test]
    fn a_row_cannot_write_a_key_the_screen_does_not_show() {
        let catalog = catalog();

        let hidden = catalog.spec("bash.patterns").expect("the key");
        assert!(reachable_from_a_row(hidden)
            .expect_err("hidden")
            .contains("not surfaced"));

        let deferred = catalog.spec("tools.format").expect("the key");
        assert!(reachable_from_a_row(deferred)
            .expect_err("deferred")
            .contains("does not have"));

        let curated = catalog.spec("enabledModels").expect("the key");
        assert_eq!(reachable_from_a_row(curated), Ok(()));
    }

    #[test]
    fn a_dangerous_key_needs_its_own_name_typed() {
        let catalog = catalog();

        let refusal = confirm_write(&catalog, "bash.patterns", None).expect_err("unconfirmed");
        assert!(refusal.contains("bash.patterns"), "{refusal}");
        assert!(
            refusal.contains("without a dialog"),
            "the reason travels: {refusal}"
        );

        let wrong = Some("yes");
        assert!(confirm_write(&catalog, "bash.patterns", wrong).is_err());
        assert_eq!(
            confirm_write(&catalog, "bash.patterns", Some("bash.patterns")),
            Ok(())
        );
        assert_eq!(
            confirm_write(&catalog, "bash.patterns", Some(" bash.patterns ")),
            Ok(()),
            "padding is not a reason to refuse"
        );

        // A `warn` key is not gated, and neither is an ordinary one.
        assert_eq!(confirm_write(&catalog, "tools.format", None), Ok(()));
        assert_eq!(confirm_write(&catalog, "enabledModels", None), Ok(()));
    }

    #[test]
    fn a_hand_edit_needs_one_word_when_the_plan_is_dangerous() {
        let catalog = catalog();
        let keys = vec!["enabledModels".to_string(), "bash.patterns".to_string()];

        let refusal = confirm_hatch(&catalog, &keys, None).expect_err("unconfirmed");
        assert!(refusal.contains("bash.patterns"), "{refusal}");
        assert!(
            !refusal.contains("enabledModels"),
            "only the dangerous ones are listed: {refusal}"
        );

        assert_eq!(confirm_hatch(&catalog, &keys, Some("CONFIRM")), Ok(()));
        assert!(confirm_hatch(&catalog, &keys, Some("ok")).is_err());

        // An ordinary plan is not gated at all.
        let plain = vec!["enabledModels".to_string()];
        assert_eq!(confirm_hatch(&catalog, &plain, None), Ok(()));

        // A `warn` key does not gate the hatch either.
        let warned = vec!["tools.format".to_string()];
        assert_eq!(confirm_hatch(&catalog, &warned, None), Ok(()));
    }
}
