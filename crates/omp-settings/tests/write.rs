//! The write path, against the engine's own CLI in a throwaway profile.
//!
//! The app never writes a config file: every change goes through `omp config set|reset`, so the
//! engine's own writer holds the cross-process lock and applies its own migrations (`docs/13`
//! protection 3, re-decided against the measured facts — see `crates/omp-settings/src/engine.rs`).
//! That decision is only sound if the CLI really does accept everything a row can produce, and
//! this is where that is measured rather than assumed.
//!
//! Everything here runs against `--profile omp-settings-test-<pid>`, a profile the engine
//! creates for the occasion and this file removes afterwards: the user's own config is never
//! touched, and no test needs to serialize against another.
//!
//! Ignored by default: it runs the real `omp` binary (about 0.25 s per write, no network, no
//! credentials).
//!
//! ```text
//! cargo test -p omp-settings --test write -- --ignored --nocapture
//! ```

use omp_settings::apply;
use omp_settings::{Catalog, Cli};
use serde_json::{json, Value};

/// The profile one test owns, and takes away again.
///
/// A guard rather than a call at the end, so a failed assertion still cleans up. Named per
/// test, not per process: the tests in this file run in parallel, and a shared profile meant
/// one test's cleanup removed the directory another was still writing into — which showed up
/// as a write that "succeeded" and then read back someone else's value.
struct Profile(String);

impl Profile {
    fn new(name: &str) -> Self {
        Self(format!("omp-settings-test-{}-{name}", std::process::id()))
    }

    fn cli(&self) -> Cli {
        Cli::with_profile(self.0.clone())
    }

    fn path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").expect("HOME"))
            .join(".omp/profiles")
            .join(&self.0)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path());
    }
}

/// A valid value for a key the engine reports nothing for.
fn literal_for(kind: omp_settings::SettingType) -> Value {
    use omp_settings::SettingType;
    match kind {
        SettingType::Boolean => json!(true),
        SettingType::Number => json!(1),
        SettingType::String => json!("omp-settings-test"),
        SettingType::Enum => json!("off"),
        SettingType::Array => json!(["a"]),
        SettingType::Record => json!({"default": "test"}),
    }
}

fn catalog() -> &'static Catalog {
    Catalog::embedded().expect("the shipped catalog parses")
}

/// The first key of a given control that the curated screen actually renders.
fn first_curated(control: omp_settings::Control) -> &'static omp_settings::KeySpec {
    catalog()
        .keys
        .iter()
        .find(|spec| {
            spec.control == control && spec.disposition == omp_settings::Disposition::Curated
        })
        .unwrap_or_else(|| panic!("the catalog has a curated key for {control:?}"))
}

/// The value the engine reports for a key.
async fn value(cli: &Cli, key: &str) -> Option<Value> {
    cli.list()
        .await
        .expect("the engine lists its settings")
        .get(key)
        .and_then(|entry| entry.value.clone())
}

#[tokio::test]
#[ignore = "runs the real `omp` binary (no network, no credentials)"]
async fn every_control_a_row_can_produce_is_written_and_read_back() {
    let profile = Profile::new("controls");
    let cli = profile.cli();

    let directory = cli
        .agent_dir()
        .await
        .expect("the engine reports its config directory");
    println!("profile config directory: {directory}");
    assert!(
        directory.contains(&profile.0),
        "the write path must be exercised in the throwaway profile, not the user's: {directory}"
    );

    // A fresh profile: the engine still answers for every key, which is what the screen needs.
    let listing = cli.list().await.expect("the engine lists its settings");
    assert_eq!(
        listing.entries.len(),
        catalog().keys.len(),
        "the engine and the catalog must agree on the key set"
    );

    // One row of every kind, found in the catalog rather than guessed: a key that is *not*
    // curated cannot be written from a row at all (the gate below proves that), so a hardcoded
    // key here would be testing the wrong thing the first time the mapping moves one.
    let mut checked = 0;
    for control in [
        omp_settings::Control::Number,
        omp_settings::Control::Select,
        omp_settings::Control::Toggle,
        omp_settings::Control::Text,
        omp_settings::Control::List,
        omp_settings::Control::Record,
    ] {
        let spec = first_curated(control);

        // Writing back what the engine already reported is always a valid value, which is what
        // makes this a test of the write path rather than of a hand-picked literal.
        let written = value(&cli, &spec.key)
            .await
            .unwrap_or_else(|| literal_for(spec.kind));

        apply::set(&cli, catalog(), &spec.key, written.clone(), None)
            .await
            .unwrap_or_else(|error| panic!("writing {} ({control:?}) failed: {error}", spec.key));

        let read_back = value(&cli, &spec.key).await;
        println!(
            "{control:?}: {} wrote {written} → engine reports {read_back:?}",
            spec.key
        );
        assert_eq!(read_back, Some(written), "{} round-trips", spec.key);
        checked += 1;
    }
    assert_eq!(checked, 6, "every control a row can produce was exercised");

    // A reset returns a key to the engine's default, which is the clear action on a row. The
    // value has to *move* first: writing a key's own default and resetting it proves nothing.
    let number = first_curated(omp_settings::Control::Number);
    let untouched = value(&cli, &number.key)
        .await
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let moved = json!(untouched + 1);

    apply::set(&cli, catalog(), &number.key, moved.clone(), None)
        .await
        .expect("a different number is a value");
    assert_eq!(
        value(&cli, &number.key).await,
        Some(moved),
        "the value moved before the reset, or the reset below proves nothing"
    );

    let outcome = apply::reset(&cli, catalog(), &number.key, None)
        .await
        .expect("reset");
    assert_eq!(outcome.value, None, "a reset reports no value");
    assert_eq!(
        value(&cli, &number.key).await,
        Some(json!(untouched)),
        "the reset put {} back to the engine's default",
        number.key
    );
}

/// The two values the CLI's own argument parser would otherwise eat: a negative number, and a
/// string that starts with a dash. `engine.rs` places every value after `--` for exactly this
/// reason, and this is what proves the choice (measured: without `--`, `-5` is refused as
/// `Unknown option '-5'`).
#[tokio::test]
#[ignore = "runs the real `omp` binary (no network, no credentials)"]
async fn a_value_that_looks_like_a_flag_still_reaches_the_file() {
    let profile = Profile::new("flags");
    let cli = profile.cli();

    // Straight through the CLI: this test is about the argv `engine.rs` builds (every value
    // after `--`), not about which keys the app lets a row write.
    cli.set(
        "gc.coldArchiveAfterDays",
        &json!(-7),
        omp_settings::SettingType::Number,
    )
    .await
    .expect("a negative number is a value, not a flag");
    assert_eq!(
        value(&cli, "gc.coldArchiveAfterDays").await,
        Some(json!(-7)),
        "and it round-trips as a number rather than a string"
    );

    cli.set(
        "theme.dark",
        &json!("-weird"),
        omp_settings::SettingType::String,
    )
    .await
    .expect("a string that starts with a dash is a value too");
    assert_eq!(value(&cli, "theme.dark").await, Some(json!("-weird")));
}

/// A credential is written like any other string, and the engine redacts it on the way back —
/// which is what lets a row be replace-only: the app writes a value it can never read again.
#[tokio::test]
#[ignore = "runs the real `omp` binary (no network, no credentials)"]
async fn a_credential_is_written_and_never_read_back() {
    let profile = Profile::new("credentials");
    let cli = profile.cli();

    let key = "searxng.token";
    assert!(
        catalog().spec(key).is_some_and(|spec| spec.is_secret()),
        "the test is about a key the catalog calls a secret"
    );

    apply::set(&cli, catalog(), key, json!("not-a-real-token"), None)
        .await
        .expect("a secret row writes its value");

    let listing = cli.list().await.expect("list");
    let entry = listing.get(key).expect("the key is listed");
    println!("{key}: value={:?} redacted={}", entry.value, entry.redacted);

    assert!(
        entry.redacted,
        "the engine says it holds a secret it will not print"
    );
    assert_eq!(entry.value, None, "and the app never receives it");
}

/// Refusals happen before the engine is asked, and the ones the engine makes are its own words.
///
/// The first half is the app's guarantee — a stale row cannot write a key the screen does not
/// show, and nothing is written when it tries. The second half is what the row's error message
/// will contain, in the engine's phrasing, because the app passes it through.
#[tokio::test]
#[ignore = "runs the real `omp` binary (no network, no credentials)"]
async fn a_refused_write_writes_nothing() {
    let profile = Profile::new("refusals");
    let cli = profile.cli();

    // A key the curated screen does not render: refused here, before any process is spawned.
    let hidden = "bash.patterns";
    assert!(
        catalog()
            .spec(hidden)
            .is_some_and(|spec| spec.disposition != omp_settings::Disposition::Curated),
        "the test is about a key that is not a row"
    );
    let before = value(&cli, hidden).await;
    let refused = apply::set(&cli, catalog(), hidden, json!(["ls *"]), None)
        .await
        .expect_err("a hidden key is not writable from a row");
    println!("refused: {refused}");
    assert!(refused.contains("raw config file"), "{refused}");
    assert_eq!(value(&cli, hidden).await, before, "and nothing was written");

    // A value outside the enum's domain: refused by the app, naming the domain.
    let refused = apply::set(
        &cli,
        catalog(),
        "power.sleepPrevention",
        json!("sometimes"),
        None,
    )
    .await
    .expect_err("not a value of that enum");
    println!("refused: {refused}");
    assert!(refused.contains("off, idle, display, system"), "{refused}");

    // And the engine's own refusal is what a bad value produces when it is asked directly —
    // the case a *new* engine key would hit, since the catalog cannot know its domain.
    let error = cli
        .set(
            "power.sleepPrevention",
            &json!("sometimes"),
            omp_settings::SettingType::Enum,
        )
        .await
        .expect_err("the engine refuses it too");
    println!("the engine's own words: {error}");
    assert!(error.contains("Valid values"), "{error}");
}
