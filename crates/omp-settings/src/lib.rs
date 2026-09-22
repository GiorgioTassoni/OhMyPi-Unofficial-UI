//! The engine's settings, as the app reads, validates and writes them.
//!
//! This crate owns the whole settings surface, and nothing in it renders:
//!
//! * [`catalog`] — the generated catalog (`catalog.json`, from
//!   `scripts/gen-settings-catalog.ts`): what exists, how it is drawn, what a write costs.
//! * [`conditions`] — the schema's `ui.condition` gates, transcribed from the engine's own
//!   settings panel, so a gated row can say *why* it is disabled.
//! * [`engine`] — the config CLI, which is the app's only writer.
//! * [`hatch`] — the raw config escape hatch: what a config file sets, what a hand-edit
//!   would change, and the backup that makes an edit reversible.
//! * [`validate`] — what may be written, and how loudly a key has to be confirmed.
//! * [`apply`] — the write flow itself: validate, confirm, hand to the engine, report.
//! * [`screen`] — the projection the frontend draws: catalog + effective values + provenance.
//!
//! Two rules run through all of it, and both were measured rather than assumed:
//!
//! 1. **The app never writes the config file.** Every write goes through `omp config set` /
//!    `omp config reset`, because the engine's own writer holds a native cross-process lock,
//!    resolves symlinks to their physical target and applies migrations — see [`engine`].
//! 2. **A write is not live.** A running session reads its settings when it is constructed;
//!    `Settings.reloadFromDisk()` has exactly one caller in the entire engine. So the only
//!    keys the app may promise immediacy for are the ones it applies *through RPC*, and
//!    [`catalog::Restart`] is the type that keeps that promise honest.

pub mod apply;
pub mod catalog;
pub mod conditions;
pub mod engine;
pub mod hatch;
pub mod screen;
pub mod validate;

pub use apply::{ApplyReport, ChangeResult, WriteOutcome};
pub use catalog::{
    Catalog, Control, DangerLevel, DangerSpec, Disposition, KeySpec, Restart, Section, SettingType,
};
pub use engine::{Cli, Entry, Listing};
pub use hatch::{
    backup, backups, redact, Action, Change, Flattened, Plan, Refusal, SetKey, SyntaxError,
};
pub use screen::{
    Action as ActionRow, Drift, FileState, Files, Origin, Row, Screen, Setting, Sources, Static,
    Tone,
};
