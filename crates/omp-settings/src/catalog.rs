//! The generated catalog: the engine's settings, as the app presents them.
//!
//! `catalog.json` is produced by `scripts/gen-settings-catalog.ts` from three sources
//! (the engine's own `config list`, the pinned package's schema, and
//! `docs/13-settings-mapping.md`) and is embedded here with `include_str!`, so a build
//! cannot ship a catalog that disagrees with the file in the tree. Regeneration is a
//! deliberate commit; `--check` in CI fails when the engine's key set moves.
//!
//! What this module is *not*: the values. Effective values come from the engine at open
//! time (`engine::list`), and the only thing this side knows about a key is how to draw
//! it, how to validate a write to it, and what a write will cost the user.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// The catalog, embedded at compile time.
const CATALOG_JSON: &str = include_str!("../catalog.json");

/// The catalog the app ships, parsed on first use.
///
/// A parse failure is a broken build (the JSON is embedded), so it is reported rather than
/// worked around: every screen that renders settings depends on this being right.
static CATALOG: LazyLock<Result<Catalog, String>> = LazyLock::new(|| Catalog::parse(CATALOG_JSON));

/// A settings key's value type, as the engine's schema spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingType {
    Boolean,
    Number,
    Enum,
    String,
    Array,
    Record,
}

/// Whether the curated screen renders a row for a key (`docs/13` §"How to read a row").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    /// Rendered as a row in its section.
    Curated,
    /// Belongs to a capability v1 does not have (`docs/11` §2.2) — no row anywhere.
    Deferred,
    /// Internal, unsafe, or inert in a GUI — reachable only through the escape hatch.
    Hidden,
}

/// The widget a key is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    Toggle,
    Select,
    Number,
    Text,
    List,
    Record,
    Secret,
}

/// What a write to a key costs before it takes effect.
///
/// `Live` is the only class that promises immediacy, and it is earned: those keys have an
/// RPC command the app applies to live sessions. Everything else needs the owning process
/// restarted, because a running session's settings are read when it is constructed — see
/// `docs/13` §"The measurement that fixes `restart`".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Restart {
    /// Applied to live sessions through RPC, and recorded for the next ones.
    Live,
    /// Recorded for the next start of each session.
    Sidecar,
    /// Baked into app-owned state built once at launch.
    App,
}

/// How loudly a key has to be confirmed before a write (`docs/13` §"Keys that must NOT be
/// exposed").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DangerLevel {
    /// Needs a typed confirmation: it widens what the agent may execute, or sends data
    /// somewhere, and the failure is silent until it matters.
    Confirm,
    /// A warning banner is enough — the capability behind it is deferred anyway.
    Warn,
}

/// One navigation section of the settings screen.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    pub title: String,
    /// The one line shown under the page title (the reference's pattern).
    pub blurb: String,
    /// The nav column this section sits under: `app`, `agent` or `system`.
    pub nav_group: String,
    /// An icon name that exists in `frontend/src/lib/icons.ts`.
    pub icon: String,
    pub order: usize,
}

/// Everything the app knows about one settings key, minus its value.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySpec {
    pub key: String,
    /// The row's title, humanised from the key by the generator (one humaniser, not two).
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    /// The engine's own description; empty for the keys that carry no `ui` metadata.
    pub description: String,
    /// An enum's domain, resolved by the engine (not by us: several enums are built from
    /// constants, and the engine's type display already resolves them).
    #[serde(default)]
    pub values: Vec<String>,
    /// Humanised labels for enum values, when the generator saw a name to humanise.
    #[serde(default)]
    pub option_labels: HashMap<String, String>,
    #[serde(default)]
    pub credential: bool,
    /// The engine's symbolic condition name (`ui.condition`), when the key has one.
    #[serde(default)]
    pub condition: Option<String>,
    /// The engine's own schema tab, kept for the escape hatch's grouping.
    pub tab: String,
    /// The sub-group heading this key is listed under inside its section.
    pub group: String,
    /// The id of the section that owns it.
    pub section: String,
    pub disposition: Disposition,
    pub control: Control,
    pub restart: Restart,
}

impl KeySpec {
    /// Whether this key's value is a secret the app must never display.
    pub fn is_secret(&self) -> bool {
        self.credential || self.control == Control::Secret
    }
}

/// A key that must not be a plain row, with the reason and how loudly to confirm it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DangerSpec {
    pub key: String,
    pub level: DangerLevel,
    pub why: String,
}

/// The whole catalog, with the lookups the screen and the escape hatch need.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    /// The engine release this was generated against; a mismatch with the running sidecar
    /// is what `drift` reports.
    pub engine_version: String,
    /// Digest of the schema file the conditions were read from, for the record.
    pub schema_sha256: String,
    pub sections: Vec<Section>,
    pub keys: Vec<KeySpec>,
    #[serde(default)]
    pub danger: Vec<DangerSpec>,

    /// Key → index into `keys`. Built after deserialisation; never on the wire.
    #[serde(skip)]
    by_key: HashMap<String, usize>,
    /// Key → index into `danger`.
    #[serde(skip)]
    danger_by_key: HashMap<String, usize>,
}

impl Catalog {
    /// Parse a catalog from JSON. Exposed so tests can build small ones.
    pub fn parse(json: &str) -> Result<Self, String> {
        let mut catalog: Catalog = serde_json::from_str(json)
            .map_err(|error| format!("the settings catalog is not readable: {error}"))?;

        catalog.by_key = catalog
            .keys
            .iter()
            .enumerate()
            .map(|(index, spec)| (spec.key.clone(), index))
            .collect();
        catalog.danger_by_key = catalog
            .danger
            .iter()
            .enumerate()
            .map(|(index, spec)| (spec.key.clone(), index))
            .collect();

        Ok(catalog)
    }

    /// The catalog the app ships.
    pub fn embedded() -> Result<&'static Catalog, String> {
        match &*CATALOG {
            Ok(catalog) => Ok(catalog),
            Err(error) => Err(error.clone()),
        }
    }

    /// The spec for a key, when the catalog knows it.
    pub fn spec(&self, key: &str) -> Option<&KeySpec> {
        self.by_key.get(key).map(|index| &self.keys[*index])
    }

    /// Whether the engine has a setting the catalog does not know about (a version bump).
    pub fn knows(&self, key: &str) -> bool {
        self.by_key.contains_key(key)
    }

    /// The danger entry for a key, when it has one.
    pub fn danger(&self, key: &str) -> Option<&DangerSpec> {
        self.danger_by_key
            .get(key)
            .map(|index| &self.danger[*index])
    }

    /// A section by id.
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.id == id)
    }

    /// Sections in the order the mapping document lists them.
    pub fn sections_in_order(&self) -> Vec<&Section> {
        let mut sections: Vec<&Section> = self.sections.iter().collect();
        sections.sort_by_key(|section| section.order);
        sections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalog this small is enough to exercise the lookups, and it is *not* the shipped
    /// one: the shipped file has its own test in `tests/catalog.rs`, which is where the
    /// counts that matter live.
    const TINY: &str = r#"{
      "engineVersion": "18.2.6",
      "schemaSha256": "00",
      "sections": [
        { "id": "general", "title": "General", "blurb": "b", "navGroup": "app", "icon": "sliders", "order": 1 }
      ],
      "keys": [
        { "key": "power.sleepPrevention", "label": "Sleep prevention", "type": "enum",
          "description": "d", "values": ["off", "idle"], "optionLabels": {"off": "Off"},
          "credential": false, "tab": "interaction", "group": "Power", "section": "general",
          "disposition": "curated", "control": "select", "restart": "sidecar" },
        { "key": "tools.approvalMode", "label": "Approval mode", "type": "enum",
          "description": "d", "values": ["yolo"], "credential": false, "tab": "tools",
          "group": "Approval", "section": "general", "disposition": "hidden",
          "control": "select", "restart": "sidecar" }
      ],
      "danger": [
        { "key": "tools.approvalMode", "level": "confirm", "why": "a global bypass" }
      ]
    }"#;

    #[test]
    fn a_catalog_answers_the_questions_the_screen_asks() {
        let catalog = Catalog::parse(TINY).expect("parse");

        let spec = catalog.spec("power.sleepPrevention").expect("the key");
        assert_eq!(spec.label, "Sleep prevention");
        assert_eq!(spec.kind, SettingType::Enum);
        assert_eq!(spec.values, vec!["off", "idle"]);
        assert_eq!(
            spec.option_labels.get("off").map(String::as_str),
            Some("Off")
        );
        assert_eq!(spec.control, Control::Select);
        assert_eq!(spec.restart, Restart::Sidecar);
        assert_eq!(spec.disposition, Disposition::Curated);

        // A key the catalog does not carry is the drift case, not a panic.
        assert!(catalog.spec("no.such.key").is_none());
        assert!(!catalog.knows("no.such.key"));

        let danger = catalog.danger("tools.approvalMode").expect("the danger");
        assert_eq!(danger.level, DangerLevel::Confirm);
        assert_eq!(danger.why, "a global bypass");
        assert!(catalog.danger("power.sleepPrevention").is_none());

        assert_eq!(
            catalog.section("general").map(|s| s.title.as_str()),
            Some("General")
        );
        assert_eq!(catalog.sections_in_order().len(), 1);
    }

    #[test]
    fn a_secret_is_recognised_by_either_marking() {
        let mut catalog = Catalog::parse(TINY).expect("parse");
        catalog.keys[0].credential = true;
        assert!(
            catalog.keys[0].is_secret(),
            "the schema's marking is enough"
        );

        let mut catalog = Catalog::parse(TINY).expect("parse");
        catalog.keys[0].control = Control::Secret;
        assert!(catalog.keys[0].is_secret(), "and so is the control");

        let catalog = Catalog::parse(TINY).expect("parse");
        assert!(!catalog.keys[0].is_secret());
    }

    /// The counts that would be missed silently, taken from the shipped file rather than
    /// from a fixture: a catalog with no keys renders an empty screen and no test fails.
    #[test]
    fn the_shipped_catalog_is_whole() {
        let catalog = Catalog::embedded().expect("the embedded catalog parses");

        assert_eq!(catalog.engine_version, "18.2.6");
        assert_eq!(
            catalog.sections.len(),
            16,
            "fifteen sections plus the raw screen"
        );
        assert_eq!(catalog.keys.len(), 505, "one per schema key");

        let curated: Vec<&KeySpec> = catalog
            .keys
            .iter()
            .filter(|spec| spec.disposition == Disposition::Curated)
            .collect();
        assert_eq!(
            curated.len(),
            309,
            "the rows the screen renders — 314 in the mapping's own column, less the five keys \
             `docs/13` §\"Keys that must NOT be exposed\" forbids a row for"
        );

        // The invariant that keeps the two halves of the mapping document from disagreeing
        // again: a key the do-not-expose list calls dangerous must not be reachable from a row.
        // It is not a style rule — five keys (enabledProviders, tools.approval,
        // skills.customDirectories, bash.allowCompoundCommands, bashInterceptor.patterns) were
        // `yes` in the mapping's table and forbidden by its own list, and only this assertion
        // catches that.
        for spec in &curated {
            if let Some(danger) = catalog.danger(&spec.key) {
                assert_ne!(
                    danger.level,
                    DangerLevel::Confirm,
                    "{} is curated and on the do-not-expose list",
                    spec.key
                );
            }
        }

        let secrets = catalog.keys.iter().filter(|spec| spec.is_secret()).count();
        assert_eq!(secrets, 8, "the schema's credentials");

        assert_eq!(catalog.danger.len(), 30, "the do-not-surface list");
        assert_eq!(
            catalog
                .danger
                .iter()
                .filter(|entry| entry.level == DangerLevel::Confirm)
                .count(),
            22,
            "of which this many need a typed confirmation"
        );

        // Every curated key must land in a section the catalog actually describes, or the
        // screen would drop it on the floor.
        for spec in catalog
            .keys
            .iter()
            .filter(|spec| spec.disposition == Disposition::Curated)
        {
            assert!(
                catalog.section(&spec.section).is_some(),
                "{} names a section that is not in the catalog: {}",
                spec.key,
                spec.section
            );
        }
    }
}
