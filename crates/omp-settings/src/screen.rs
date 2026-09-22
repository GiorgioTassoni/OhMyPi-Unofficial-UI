//! The screen model: the catalog, the engine's effective values, and where each one comes
//! from.
//!
//! The screen renders exactly this and nothing else — the frontend holds no catalog — so the
//! shape is the contract between the two, and `src-tauri/src/dto.rs` mirrors it. Three things
//! decide what a row looks like:
//!
//! * **The catalog** decides whether there is a row at all (`curated`), what it says, which
//!   widget it gets, and what a write to it will cost.
//! * **The engine's listing** is the value. It merges the files, an overlay and the defaults,
//!   so it answers "what is this set to", never "where is it written" — that is the files'
//!   question, and getting the two confused is how a screen ends up claiming a value is at its
//!   default when it is written in the project file.
//! * **The conditions** (`ui.condition`) decide whether a row is *disabled*; per `docs/13`
//!   §Q3 it stays visible either way, with the condition named in one line.

use serde::Serialize;
use serde_json::Value;

use crate::catalog::{Catalog, Control, DangerLevel, Disposition, KeySpec, Restart, SettingType};
use crate::conditions::Gate;
use crate::engine::Listing;
use crate::hatch::{Flattened, Refusal};

/// Where a key's value is written.
///
/// The order is the engine's: a project file blocks the global one, and an overlay blocks both
/// (`config/settings.ts:1369` — "the project … correctly blocks lower layers").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// A read-only overlay the user pointed the engine at (`PI_CONFIG_FILES`).
    Overlay,
    /// `<workspace>/.omp/config.yml`.
    Project,
    /// `~/.omp/agent/config.yml`.
    Global,
    /// Nothing the app can read sets it.
    Default,
    /// A file that should have answered this could not be read, so the app will not claim to
    /// know. Saying "default" here is how a screen lies.
    Unknown,
}

/// One config file, as the screen needs to describe it.
#[derive(Debug, Clone)]
pub struct FileState {
    pub path: String,
    pub exists: bool,
    /// Why the file could not be read, when it could not.
    pub error: Option<String>,
    /// What it sets, when it could be read.
    pub keys: Option<Flattened>,
}

impl FileState {
    /// A file that is not there.
    pub fn missing(path: String) -> Self {
        Self {
            path,
            exists: false,
            error: None,
            keys: None,
        }
    }

    /// A file the app read.
    pub fn read(path: String, keys: Flattened) -> Self {
        Self {
            path,
            exists: true,
            error: None,
            keys: Some(keys),
        }
    }

    /// A file that exists and could not be parsed.
    pub fn unreadable(path: String, error: String) -> Self {
        Self {
            path,
            exists: true,
            error: Some(error),
            keys: None,
        }
    }

    /// Whether this file holds a value for a key.
    fn sets(&self, key: &str) -> bool {
        self.keys.as_ref().is_some_and(|keys| keys.sets(key))
    }
}

/// Every file the app can see, and the reasons one might be missing from that picture.
#[derive(Debug, Clone)]
pub struct Files {
    pub agent_dir: String,
    pub global: FileState,
    pub project: FileState,
    pub overlays: Vec<FileState>,
}

/// The files as they travel to the frontend: paths, and why one is not readable.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    pub agent_dir: String,
    pub global_file: FileStateSummary,
    pub project_file: FileStateSummary,
    pub overlays: Vec<FileStateSummary>,
}

/// One file, as the banner describes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStateSummary {
    pub path: String,
    pub exists: bool,
    pub error: Option<String>,
    /// The parts of this file the app would not act on (a key it does not know, a key it will
    /// not write). Surfaced because a *partially* readable file is the confusing case.
    pub refusals: Vec<Refusal>,
}

/// The engine knows settings this build does not, or the other way round.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drift {
    /// Keys the engine reported that the catalog has never heard of — the signature of an
    /// engine newer than the build.
    pub unknown_keys: Vec<String>,
    /// Catalog keys the engine did not report. Should be empty; a non-empty list means the
    /// catalog is newer than the sidecar, and any row in it would be a lie.
    pub missing_keys: Vec<String>,
}

/// One row's control.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum Row {
    /// A settings key with a control.
    Setting(Box<Setting>),
    /// A fact the app reports and nothing here edits.
    Static(Static),
    /// A control that does something rather than storing something.
    Action(Action),
}

/// A settings key, ready to draw.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    pub label: String,
    pub description: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    pub control: Control,
    /// The effective value. Absent when the engine has none (an unset key with no default) and
    /// when it is redacted.
    pub value: Option<Value>,
    /// Whether the engine holds a value — which is a different question from whether the app
    /// may show it.
    pub present: bool,
    /// The engine reported a value it will not print, because the key is a credential.
    pub redacted: bool,
    /// An enum's options, value and humanised label.
    pub choices: Vec<Choice>,
    pub origin: Origin,
    /// Why this row cannot be edited, when a `ui.condition` is unmet.
    pub gated: Option<Gate>,
    pub restart: Restart,
    pub danger: Option<Danger>,
}

/// One enum value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// Why a key is dangerous, in the mapping document's own words.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Danger {
    pub level: DangerLevel,
    pub why: String,
}

/// A value the app reports about itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Static {
    pub id: String,
    pub label: String,
    pub description: String,
    pub value: String,
}

/// A control that runs something.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub label: String,
    pub description: String,
    /// The id the frontend dispatches on. The host does not act on these; they are the
    /// screen's own affordances.
    pub action: String,
    /// `normal` or `danger`, so an action that restarts processes or reveals a file is not
    /// drawn like a toggle.
    pub tone: Tone,
}

/// How loud an action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Normal,
    Danger,
}

/// One sub-group inside a section — the engine's own group name, kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub rows: Vec<Row>,
}

/// One section of the nav.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSection {
    pub id: String,
    pub title: String,
    pub blurb: String,
    pub nav_group: String,
    pub icon: String,
    pub groups: Vec<Group>,
}

/// The whole screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    /// The engine release the catalog was generated against.
    pub catalog_version: String,
    pub sources: Sources,
    pub drift: Drift,
    pub sections: Vec<ScreenSection>,
}

/// Build the screen.
///
/// `app_version` is the desktop app's own version, shown beside the engine's because the two
/// move independently and a bug report needs both.
pub fn build(catalog: &Catalog, listing: &Listing, files: &Files, app_version: &str) -> Screen {
    let mut sections: Vec<ScreenSection> = catalog
        .sections_in_order()
        .into_iter()
        .map(|section| ScreenSection {
            id: section.id.clone(),
            title: section.title.clone(),
            blurb: section.blurb.clone(),
            nav_group: section.nav_group.clone(),
            icon: section.icon.clone(),
            groups: Vec::new(),
        })
        .collect();

    for spec in &catalog.keys {
        if spec.disposition != Disposition::Curated {
            continue;
        }
        let Some(section) = sections
            .iter_mut()
            .find(|section| section.id == spec.section)
        else {
            // A row whose section is not in the catalog would vanish from the screen; the
            // catalog's own test asserts this cannot happen, and a row that cannot be placed
            // is better dropped than invented into another section.
            continue;
        };

        let row = Row::Setting(Box::new(setting_row(catalog, spec, listing, files)));
        match section
            .groups
            .iter_mut()
            .find(|group| group.name == spec.group)
        {
            Some(group) => group.rows.push(row),
            None => section.groups.push(Group {
                name: spec.group.clone(),
                rows: vec![row],
            }),
        }
    }

    add_about(catalog, listing, files, app_version, &mut sections);

    // A section with nothing in it is not a section. `docs/13`'s Advanced is the case that
    // exists: 34 keys, every one of them `hidden` or `deferred`, so the curated screen renders
    // none of them — and a nav entry that opens an empty page reads as a broken screen rather
    // than as a deliberate omission. Those keys are reached the way the mapping says they are,
    // through the raw config, which has its own nav entry whether or not it has rows.
    sections.retain(|section| !section.groups.is_empty() || section.id == RAW_SECTION);

    Screen {
        catalog_version: catalog.engine_version.clone(),
        sources: sources(files),
        drift: drift(catalog, listing),
        sections,
    }
}

/// The nav entry that is a screen of its own rather than a list of settings.
const RAW_SECTION: &str = "raw-config";

/// The rows the app adds to General: the two versions, where the config lives, and the
/// approval mode it starts sessions with.
///
/// `docs/13` §Q2 asked for the mode line specifically: the mode is deliberately not a settings
/// row (it is a global bypass in one click), so its absence has to look like a decision rather
/// than a missing feature.
fn add_about(
    catalog: &Catalog,
    listing: &Listing,
    files: &Files,
    app_version: &str,
    sections: &mut [ScreenSection],
) {
    let Some(general) = sections.iter_mut().find(|section| section.id == "general") else {
        return;
    };

    let approval_key = "tools.approvalMode";
    let mode = listing
        .get(approval_key)
        .and_then(|entry| entry.value.as_ref())
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    general.groups.insert(
        0,
        Group {
            name: "About".to_string(),
            rows: vec![
                Row::Static(Static {
                    id: "app.version".to_string(),
                    label: "Version".to_string(),
                    description: format!(
                        "This app, with the engine it ships (settings catalog generated against {})",
                        catalog.engine_version
                    ),
                    value: app_version.to_string(),
                }),
                Row::Static(Static {
                    id: "app.config-dir".to_string(),
                    label: "Config directory".to_string(),
                    description: "Where the engine keeps this profile's config, sessions and credentials".to_string(),
                    value: files.agent_dir.clone(),
                }),
                Row::Action(Action {
                    id: "app.approval-mode".to_string(),
                    label: "Approval mode".to_string(),
                    description: format!(
                        "Sessions this app starts ask for `{mode}`. The mode is not a setting here — \
                         it is a spawn flag and one config key, changed in the Mode menu."
                    ),
                    action: "open-mode-menu".to_string(),
                    tone: Tone::Normal,
                }),
                Row::Action(Action {
                    id: "app.config-file".to_string(),
                    label: "Config file".to_string(),
                    description: files.global.path.clone(),
                    action: "open-config-file".to_string(),
                    tone: Tone::Normal,
                }),
            ],
        },
    );
}

/// One settings key as a row.
fn setting_row(catalog: &Catalog, spec: &KeySpec, listing: &Listing, files: &Files) -> Setting {
    let entry = listing.get(&spec.key);

    let redacted = entry.is_some_and(|entry| entry.redacted);
    let value = entry.and_then(|entry| entry.value.clone());
    // A redacted credential is *set* without being readable; an absent value with no redaction
    // is a key the engine reports no value for at all.
    let present = value.is_some() || redacted;

    Setting {
        key: spec.key.clone(),
        label: spec.label.clone(),
        description: spec.description.clone(),
        kind: spec.kind,
        control: spec.control,
        value,
        present,
        redacted,
        choices: choices(spec),
        origin: origin(spec.key.as_str(), files),
        gated: spec
            .condition
            .as_deref()
            .and_then(|name| crate::conditions::evaluate(name, &|key| listing.value_of(key))),
        restart: spec.restart,
        danger: catalog.danger(&spec.key).map(|entry| Danger {
            level: entry.level,
            why: entry.why.clone(),
        }),
    }
}

/// An enum's options, labelled where the generator had a name to humanise.
fn choices(spec: &KeySpec) -> Vec<Choice> {
    if spec.control != Control::Select {
        return Vec::new();
    }
    spec.values
        .iter()
        .map(|value| Choice {
            value: value.clone(),
            label: spec
                .option_labels
                .get(value)
                .cloned()
                .unwrap_or_else(|| value.clone()),
        })
        .collect()
}

/// Where a key's value is written.
///
/// The precedence is the engine's, highest layer first: an overlay blocks a project file,
/// and a project file blocks the global one (`config/settings.ts:1369` — "the project …
/// correctly blocks lower layers").
fn origin(key: &str, files: &Files) -> Origin {
    if files.overlays.iter().any(|overlay| overlay.sets(key)) {
        return Origin::Overlay;
    }
    if files.project.sets(key) {
        return Origin::Project;
    }
    if files.global.sets(key) {
        return Origin::Global;
    }

    // Nothing readable claims the key. If a file that could have claimed it was unreadable,
    // the honest answer is that the app does not know.
    let blind = files.global.exists && files.global.keys.is_none()
        || files.project.exists && files.project.keys.is_none()
        || files
            .overlays
            .iter()
            .any(|overlay| overlay.exists && overlay.keys.is_none());
    if blind {
        return Origin::Unknown;
    }

    Origin::Default
}

/// The files as the banner describes them.
fn sources(files: &Files) -> Sources {
    let summarise = |file: &FileState| FileStateSummary {
        path: file.path.clone(),
        exists: file.exists,
        error: file.error.clone(),
        refusals: file
            .keys
            .as_ref()
            .map(|keys| keys.refusals.clone())
            .unwrap_or_default(),
    };

    Sources {
        agent_dir: files.agent_dir.clone(),
        global_file: summarise(&files.global),
        project_file: summarise(&files.project),
        overlays: files.overlays.iter().map(summarise).collect(),
    }
}

/// Which side of a version bump this build is on.
fn drift(catalog: &Catalog, listing: &Listing) -> Drift {
    let unknown_keys = listing
        .entries
        .keys()
        .filter(|key| !catalog.knows(key))
        .cloned()
        .collect();

    let missing_keys = catalog
        .keys
        .iter()
        .filter(|spec| listing.get(&spec.key).is_none())
        .map(|spec| spec.key.clone())
        .collect();

    Drift {
        unknown_keys,
        missing_keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::engine::Entry;
    use crate::hatch::flatten;
    use std::collections::BTreeMap;

    const CATALOG: &str = r#"{
      "engineVersion": "18.2.6",
      "schemaSha256": "00",
      "sections": [
        { "id": "general", "title": "General", "blurb": "app-level switches", "navGroup": "app", "icon": "sliders", "order": 1 },
        { "id": "agent", "title": "Agent", "blurb": "what the agent may do", "navGroup": "agent", "icon": "bot", "order": 2 }
      ],
      "keys": [
        { "key": "power.sleepPrevention", "label": "Sleep prevention", "type": "enum", "description": "d",
          "values": ["off", "idle"], "optionLabels": {"off": "Off", "idle": "Idle"}, "credential": false,
          "tab": "interaction", "group": "Power", "section": "general", "disposition": "curated",
          "control": "select", "restart": "sidecar" },
        { "key": "advisor.immuneTurns", "label": "Immune turns", "type": "number", "description": "d",
          "values": [], "credential": false, "condition": "advisorEnabled", "tab": "model",
          "group": "Advisor", "section": "agent", "disposition": "curated", "control": "number",
          "restart": "sidecar" },
        { "key": "tools.approval", "label": "Tool approval", "type": "record", "description": "d",
          "values": [], "credential": false, "tab": "tools", "group": "Approval", "section": "agent",
          "disposition": "hidden", "control": "record", "restart": "sidecar" },
        { "key": "advisor.enabled", "label": "Advisor", "type": "boolean", "description": "d",
          "values": [], "credential": false, "tab": "model", "group": "Advisor", "section": "agent",
          "disposition": "curated", "control": "toggle", "restart": "sidecar" },
        { "key": "gc.wal", "label": "WAL", "type": "boolean", "description": "d",
          "values": [], "credential": false, "tab": "interaction", "group": "Housekeeping",
          "section": "general", "disposition": "deferred", "control": "toggle", "restart": "sidecar" }
      ],
      "danger": []
    }"#;

    fn catalog() -> Catalog {
        Catalog::parse(CATALOG).expect("parse")
    }

    fn listing(entries: &[(&str, Value)]) -> Listing {
        Listing {
            entries: entries
                .iter()
                .map(|(key, value)| {
                    (
                        (*key).to_string(),
                        Entry {
                            value: Some(value.clone()),
                            kind: SettingType::Boolean,
                            description: String::new(),
                            redacted: false,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn files(global: &str, project: &str) -> Files {
        let catalog = catalog();
        Files {
            agent_dir: "/home/u/.omp/agent".to_string(),
            global: FileState::read(
                "/home/u/.omp/agent/config.yml".to_string(),
                flatten(global, &catalog).expect("global"),
            ),
            project: FileState::read(
                "/w/.omp/config.yml".to_string(),
                flatten(project, &catalog).expect("project"),
            ),
            overlays: Vec::new(),
        }
    }

    #[test]
    fn only_curated_keys_become_rows() {
        let screen = build(&catalog(), &listing(&[]), &files("", ""), "0.1.0");

        let general = screen
            .sections
            .iter()
            .find(|s| s.id == "general")
            .expect("general");
        let keys: Vec<&str> = general
            .groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .filter_map(|row| match row {
                Row::Setting(setting) => Some(setting.key.as_str()),
                _ => None,
            })
            .collect();
        assert!(keys.contains(&"power.sleepPrevention"));
        assert!(
            !keys.contains(&"tools.approval"),
            "hidden keys are not rows"
        );
        assert!(!keys.contains(&"gc.wal"), "deferred keys are not rows");

        // The agent section keeps its own key, under the engine's own group name.
        let agent = screen
            .sections
            .iter()
            .find(|s| s.id == "agent")
            .expect("agent");
        assert_eq!(agent.groups[0].name, "Advisor");
    }

    #[test]
    fn general_leads_with_what_the_app_knows_about_itself() {
        let listing = listing(&[("tools.approvalMode", serde_json::json!("write"))]);
        let screen = build(&catalog(), &listing, &files("", ""), "0.1.0");
        let general = screen
            .sections
            .iter()
            .find(|s| s.id == "general")
            .expect("general");

        assert_eq!(
            general.groups[0].name, "About",
            "the app's own facts come first"
        );
        let labels: Vec<String> = general.groups[0]
            .rows
            .iter()
            .map(|row| match row {
                Row::Static(item) => item.label.clone(),
                Row::Action(item) => item.label.clone(),
                Row::Setting(setting) => setting.label.clone(),
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "Version",
                "Config directory",
                "Approval mode",
                "Config file"
            ]
        );

        // The mode row is an action pointing at the menu that owns the decision.
        match &general.groups[0].rows[2] {
            Row::Action(action) => {
                assert_eq!(action.action, "open-mode-menu");
                assert!(
                    action.description.contains("write"),
                    "{}",
                    action.description
                );
            }
            other => panic!("expected an action, got {other:?}"),
        }

        // And the version row carries the app's own version.
        match &general.groups[0].rows[0] {
            Row::Static(item) => assert_eq!(item.value, "0.1.0"),
            other => panic!("expected a static, got {other:?}"),
        }
    }

    #[test]
    fn a_value_reports_where_it_is_written_and_not_what_it_is() {
        let listing = listing(&[("power.sleepPrevention", serde_json::json!("display"))]);
        let screen = build(
            &catalog(),
            &listing,
            &files(
                "power:\n  sleepPrevention: idle\n",
                "power:\n  sleepPrevention: display\n",
            ),
            "0.1.0",
        );

        let row = setting(&screen, "power.sleepPrevention");
        assert_eq!(
            row.origin,
            Origin::Project,
            "the project file blocks the global one"
        );
        assert_eq!(
            row.value,
            Some(serde_json::json!("display")),
            "and the value is still the engine's"
        );
        assert!(row.present);
        assert!(!row.redacted);
    }

    #[test]
    fn a_key_no_file_sets_is_at_its_default() {
        let listing = listing(&[("power.sleepPrevention", serde_json::json!("idle"))]);
        let screen = build(&catalog(), &listing, &files("", ""), "0.1.0");
        assert_eq!(
            setting(&screen, "power.sleepPrevention").origin,
            Origin::Default
        );
    }

    /// A file that exists and cannot be read is the one case where provenance must not be
    /// guessed: claiming "default" would be a claim about a file nobody could parse.
    #[test]
    fn an_unreadable_file_makes_provenance_unknown() {
        let listing = listing(&[("power.sleepPrevention", serde_json::json!("idle"))]);
        let mut files = files("", "");
        files.global = FileState::unreadable(
            "/home/u/.omp/agent/config.yml".to_string(),
            "line 2, column 6: mapping values are not allowed in this context".to_string(),
        );

        let screen = build(&catalog(), &listing, &files, "0.1.0");
        assert_eq!(
            setting(&screen, "power.sleepPrevention").origin,
            Origin::Unknown
        );
        assert!(screen.sources.global_file.error.is_some());
        assert!(screen.sources.global_file.exists);
    }

    #[test]
    fn a_redacted_credential_is_present_without_a_value() {
        let catalog = catalog();
        let listing = Listing {
            entries: BTreeMap::from([(
                "power.sleepPrevention".to_string(),
                Entry {
                    value: None,
                    kind: SettingType::String,
                    description: String::new(),
                    redacted: true,
                },
            )]),
        };

        let screen = build(&catalog, &listing, &files("", ""), "0.1.0");
        let row = setting(&screen, "power.sleepPrevention");
        assert!(row.present, "the engine says it holds a value");
        assert!(row.redacted, "and that the app may not see it");
        assert_eq!(row.value, None);
    }

    #[test]
    fn an_unmet_condition_disables_the_row_and_says_why() {
        let off = listing(&[("advisor.enabled", serde_json::json!(false))]);
        let screen = build(&catalog(), &off, &files("", ""), "0.1.0");

        let gate = setting(&screen, "advisor.immuneTurns")
            .gated
            .clone()
            .expect("gated");
        assert_eq!(gate.needs.as_deref(), Some("advisor.enabled"));

        // And is met as soon as the setting it reads is on.
        let on = listing(&[("advisor.enabled", serde_json::json!(true))]);
        let screen = build(&catalog(), &on, &files("", ""), "0.1.0");
        assert_eq!(setting(&screen, "advisor.immuneTurns").gated, None);
    }

    #[test]
    fn an_enum_row_carries_labelled_choices() {
        let screen = build(&catalog(), &listing(&[]), &files("", ""), "0.1.0");
        let row = setting(&screen, "power.sleepPrevention");

        assert_eq!(
            row.choices
                .iter()
                .map(|choice| (choice.value.as_str(), choice.label.as_str()))
                .collect::<Vec<_>>(),
            vec![("off", "Off"), ("idle", "Idle")]
        );
        assert!(
            setting(&screen, "advisor.immuneTurns").choices.is_empty(),
            "a number has no choices"
        );
    }

    #[test]
    fn drift_is_reported_in_both_directions() {
        let engine_ahead = listing(&[
            ("power.sleepPrevention", serde_json::json!("idle")),
            ("something.new", serde_json::json!(1)),
        ]);
        let screen = build(&catalog(), &engine_ahead, &files("", ""), "0.1.0");
        assert_eq!(screen.drift.unknown_keys, vec!["something.new"]);
        assert!(
            screen.drift.missing_keys.contains(&"gc.wal".to_string()),
            "keys the engine did not report"
        );
    }

    /// The screen the frontend actually receives, built from the *shipped* catalog.
    ///
    /// Every other test in this module uses a fixture, which is the right way to test a rule
    /// and the wrong way to notice that the real data does not fit the shape. This one asserts
    /// the things the frontend would silently render as a gap: a curated key with no row, an
    /// enum with nothing to choose, a row with no label, a section the nav can reach that has
    /// nothing in it.
    #[test]
    fn the_shipped_catalog_builds_a_screen_with_no_gaps() {
        let catalog = Catalog::embedded().expect("the shipped catalog parses");
        let files = Files {
            agent_dir: "/home/u/.omp/agent".to_string(),
            global: FileState::missing("/home/u/.omp/agent/config.yml".to_string()),
            project: FileState::missing("/w/.omp/config.yml".to_string()),
            overlays: Vec::new(),
        };

        let screen = build(catalog, &Listing::default(), &files, "0.1.0");

        let rows: Vec<&Setting> = screen
            .sections
            .iter()
            .flat_map(|section| section.groups.iter())
            .flat_map(|group| group.rows.iter())
            .filter_map(|row| match row {
                Row::Setting(setting) => Some(setting.as_ref()),
                _ => None,
            })
            .collect();

        let curated = catalog
            .keys
            .iter()
            .filter(|spec| spec.disposition == Disposition::Curated)
            .count();
        assert_eq!(
            rows.len(),
            curated,
            "every curated key has exactly one row, and nothing else does"
        );

        for row in &rows {
            assert!(!row.label.is_empty(), "{} has no label", row.key);
            // The description is the engine's own, verbatim — 44 of the 314 curated keys have
            // none at all (the schema declares no `ui.description` for them), and the screen
            // shows the key path in their place rather than inventing prose. What must never
            // happen is the host composing a description of its own.
            let spec = catalog.spec(&row.key).expect("the row's own key");
            assert_eq!(
                row.description, spec.description,
                "{} must carry the engine's description, not one of ours",
                row.key
            );
            assert!(
                catalog
                    .section(&catalog.spec(&row.key).expect("the key").section)
                    .is_some(),
                "{} is in a section the nav cannot reach",
                row.key
            );
            if row.control == Control::Select {
                assert!(
                    !row.choices.is_empty(),
                    "{} is a select with nothing to choose",
                    row.key
                );
                for choice in &row.choices {
                    assert!(
                        !choice.label.is_empty(),
                        "{} has an unlabelled choice",
                        row.key
                    );
                    assert!(
                        catalog
                            .spec(&row.key)
                            .expect("the key")
                            .values
                            .contains(&choice.value),
                        "{} offers a value the engine's schema does not list",
                        row.key
                    );
                }
            }
            if let Some(gate) = &row.gated {
                assert!(
                    !gate.reason.is_empty(),
                    "{} is disabled with no reason",
                    row.key
                );
            }
        }

        // A gated row is *visible*, which is the decision `docs/13` §Q3 settled: hiding a row
        // that depends on another setting makes the app look like it lost a feature.
        let gated = rows.iter().filter(|row| row.gated.is_some()).count();
        assert!(
            gated > 0,
            "some rows are gated; the shipped catalog has 45 conditions"
        );

        // The nav's last section is the escape hatch, and it has no rows of its own.
        let raw = screen
            .sections
            .iter()
            .find(|section| section.id == "raw-config")
            .expect("the raw config section");
        assert!(raw.groups.is_empty(), "the hatch is not a list of settings");

        // Every section that reaches the nav has something in it — the ones that do not are
        // dropped rather than rendered as an empty page (`docs/13`'s Advanced is 34 keys and
        // not one curated row).
        for section in &screen.sections {
            if section.id == "raw-config" {
                continue; // the hatch, which has a screen of its own
            }
            assert!(
                !section.groups.is_empty(),
                "section `{}` would render as an empty page",
                section.id
            );
            assert!(
                !section.blurb.is_empty(),
                "section `{}` has no blurb",
                section.id
            );
        }
        assert!(
            screen.sections.len() < catalog.sections.len(),
            "at least one section in the catalog has no curated rows, or this test proves nothing"
        );

        // The app's own facts lead the General page.
        let general = screen
            .sections
            .iter()
            .find(|s| s.id == "general")
            .expect("general");
        assert_eq!(general.groups[0].name, "About");
        assert_eq!(general.groups[0].rows.len(), 4);
    }

    fn setting<'a>(screen: &'a Screen, key: &str) -> &'a Setting {
        screen
            .sections
            .iter()
            .flat_map(|section| section.groups.iter())
            .flat_map(|group| group.rows.iter())
            .find_map(|row| match row {
                Row::Setting(setting) if setting.key == key => Some(setting.as_ref()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no row for {key}"))
    }
}
