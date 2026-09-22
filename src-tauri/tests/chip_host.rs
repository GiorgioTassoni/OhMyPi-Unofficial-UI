//! The composer's chip row, proved without a window.
//!
//! `docs/12` §5.1 gives the row four chips, and three of them reach the engine:
//! the model picker (§7.1), the effort popover (§7.6) and the context popover (§7.5).
//! What a window cannot show is whether the host behind them holds a catalogue the
//! picker can draw and whether its commands actually change the session.
//!
//! Two properties are pinned here, and both started as measurements rather than
//! assumptions:
//!
//! 1. **The catalogue the picker reads.** `get_available_models` is 601 rows / 1540 KiB
//!    and may refresh from the network on a process's first call, so the host caches it
//!    and persists it. The first test asserts the rows have the shape §7.1 renders
//!    (`name`, `provider`, `id`, the model's own `efforts`), that a fresh launch reads
//!    them back from the file, and that a warm read neither refetches nor re-reads the
//!    file.
//! 2. **The three session commands.** §7.6's level, §7.5's auto-compaction toggle, and
//!    §7.1's model selection all go through the host, which then re-reads `get_state`
//!    so the chips show what the engine now holds. The second test asserts both halves:
//!    the engine changed, and the control snapshot says so, because a chip that lags
//!    its own click is the bug §7.1 warns about ("never silently reverting").
//!
//! Ignored by default: they spawn a real agent, need provider credentials, and reach
//! the network.
//!
//! ```text
//! cargo test -p omp-desktop --test chip_host -- --ignored --nocapture
//! ```

mod support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omp_desktop::dto::ModelCatalogue;
use omp_desktop::models::{self, Catalogue, CatalogueSink};
use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, OmpClient, SidecarSpec};
use serde_json::Value;
use support::{process_exists, registry, wait_for, RecordingSink};

/// The floor the catalogue has to clear, against the 601 rows measured at v18.2.6.
///
/// A floor rather than the exact count: which models a machine can reach depends on the
/// providers it has configured, and a test that pinned 601 would fail on a correct
/// installation with fewer credentials.
const MINIMUM_ROWS: usize = 100;

/// The chip row's host, as a window would drive it: a session, a catalogue with a cache
/// directory of its own, and the announcements the picker subscribes to.
struct Harness {
    live: Arc<LiveSession>,
    catalogue: Catalogue,
    announced: Arc<Announcements>,
    cache: PathBuf,
}

impl Harness {
    async fn open(label: &str) -> Self {
        let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();
        let live = open(
            &spec,
            ClientOptions {
                request_timeout: Duration::from_secs(30),
                ..Default::default()
            },
            RecordingSink::default(),
            registry(),
        )
        .await
        .expect("the session opens");

        // A store of this test's own: the catalogue writes a real file, and two tests
        // must not share one.
        let cache = std::env::temp_dir().join(format!(
            "omp-desktop-chip-host-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&cache);

        Self {
            live,
            catalogue: Catalogue::new(Some(cache.clone())),
            announced: Arc::new(Announcements::default()),
            cache,
        }
    }

    fn client(&self) -> Arc<OmpClient> {
        self.live.client().expect("the session is open")
    }

    /// The file the next launch reads.
    fn cache_file(&self) -> PathBuf {
        self.cache.join("models.json")
    }

    /// Close the session, and prove the agent process went with it.
    ///
    /// An orphaned agent is a ~200 MB process the user cannot see, and these tests open
    /// one per test.
    async fn close(self) {
        let pid = self
            .live
            .status()
            .expect("status")
            .sidecar_pid
            .expect("the agent is running");

        self.live.shutdown(Duration::from_secs(10)).await;
        wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;

        let _ = std::fs::remove_dir_all(&self.cache);
    }
}

/// The catalogues the host announced, as the window would have received them on
/// `models-updated`.
#[derive(Default)]
struct Announcements(Mutex<Vec<ModelCatalogue>>);

impl Announcements {
    fn last(&self) -> Option<ModelCatalogue> {
        self.0.lock().ok().and_then(|all| all.last().cloned())
    }
}

impl CatalogueSink for Announcements {
    fn catalogue(&self, catalogue: ModelCatalogue) {
        if let Ok(mut all) = self.0.lock() {
            all.push(catalogue);
        }
    }
}

/// The engine's own `get_state` payload.
///
/// Read raw rather than through the control snapshot: the test's job is to check the
/// snapshot *against* what the engine says, and comparing our own decode with itself
/// would prove nothing.
async fn state(client: &OmpClient) -> Value {
    let response = client
        .request(commands::get_state(), Some(Duration::from_secs(30)))
        .await
        .expect("the engine answers get_state");
    assert!(
        protocol::is_success(&response),
        "get_state refused: {response}"
    );

    response
        .get("data")
        .cloned()
        .expect("get_state carries data")
}

/// A boolean field of a state payload, as the engine reports it.
fn flag(state: &Value, key: &str) -> bool {
    state.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A string field of a state payload.
fn text(state: &Value, key: &str) -> String {
    state
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the state payload carries `{key}`"))
        .to_string()
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn the_catalogue_answers_with_the_shape_the_picker_reads() {
    let chip = Harness::open("catalogue").await;

    chip.catalogue
        .refresh(&chip.announced, models::fetch(&chip.client()))
        .await
        .expect("the engine lists its models");

    // ---- what the picker renders ------------------------------------------
    let announced = chip
        .announced
        .last()
        .expect("a fetch that lands is announced on `models-updated`");
    assert!(
        announced.options.len() > MINIMUM_ROWS,
        "the catalogue measured 601 rows at v18.2.6, got {}",
        announced.options.len()
    );
    assert!(
        !announced.refreshing,
        "a fetch that landed is not in flight"
    );
    println!("{} rows in the catalogue", announced.options.len());

    for option in &announced.options {
        assert!(
            !option.provider.is_empty(),
            "a row the picker groups: {option:?}"
        );
        assert!(
            !option.id.is_empty(),
            "a row `set_model` can name: {option:?}"
        );
        assert!(
            !option.name.is_empty(),
            "a row with something to draw: {option:?}"
        );
    }

    // §7.6's note — "the model does not support the chosen level" — is only possible if
    // the rows carry the levels they do support.
    let thinking: Vec<&str> = announced
        .options
        .iter()
        .filter(|option| !option.efforts.is_empty())
        .map(|option| option.id.as_str())
        .take(3)
        .collect();
    assert!(
        !thinking.is_empty(),
        "no row reports its efforts, so the effort popover could never check a level"
    );
    println!("rows with efforts, e.g. {thinking:?}");

    // ---- the file the next launch opens against ---------------------------
    let persisted: Value = serde_json::from_slice(
        &std::fs::read(chip.cache_file()).expect("the catalogue is persisted when it lands"),
    )
    .expect("the cache file is JSON");
    assert_eq!(
        persisted["options"].as_array().map(Vec::len),
        Some(announced.options.len()),
        "the whole catalogue is written, not a sample of it"
    );
    assert!(
        persisted["fetchedAt"].as_u64().is_some(),
        "the file carries when it was fetched: {persisted:#?}"
    );

    let reopened = Catalogue::new(Some(chip.cache.clone()));
    let warm = reopened.snapshot().expect("the cache reads");
    assert_eq!(warm.options.len(), announced.options.len());
    assert_eq!(
        warm.options.first().map(|row| &row.id),
        announced.options.first().map(|row| &row.id)
    );
    assert_eq!(
        warm.fetched_at, announced.fetched_at,
        "reading the cache is not a fetch"
    );
    assert!(!warm.refreshing);

    // ---- `models()` is the cache, not the disk ----------------------------
    // The load happens once, so removing the file cannot change what a warm read
    // answers: that is what "never blocks" and "opens immediately" both rest on.
    std::fs::remove_file(chip.cache_file()).expect("the cache file is removed");
    let again = reopened.snapshot().expect("the cache reads");
    assert_eq!(
        again.fetched_at, warm.fetched_at,
        "a warm read leaves `fetchedAt` where it was"
    );
    assert_eq!(again.options.len(), warm.options.len());

    chip.close().await;
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn the_engine_accepts_the_three_session_commands() {
    // Every value is read from the session first and put back afterwards, so a run
    // leaves nothing changed — this drives the user's real agent configuration.
    let chip = Harness::open("commands").await;
    let client = chip.client();
    let before = state(&client).await;

    // ---- the control snapshot is what the chips render --------------------
    let status = chip.live.status().expect("status");
    assert_eq!(
        status.control.session_file.as_deref(),
        before.get("sessionFile").and_then(Value::as_str),
        "`sessionFile` is decoded exactly as the engine reports it, present or absent"
    );
    assert_eq!(
        status.control.auto_compaction_enabled,
        Some(flag(&before, "autoCompactionEnabled")),
        "the popover's toggle reads the engine's own flag"
    );

    let provider = text(&before["model"], "provider");
    let model_id = text(&before["model"], "id");
    let level_now = text(&before, "thinkingLevel");

    // A level the model actually accepts (§7.6's `efforts`), so the test does not
    // depend on which model the session happens to run.
    let catalogue = models::fetch(&client)
        .await
        .expect("the engine lists its models");
    let target = catalogue
        .iter()
        .find(|option| option.provider == provider && option.id == model_id)
        .and_then(|option| {
            option
                .efforts
                .iter()
                .find(|level| **level != level_now)
                .cloned()
        })
        .unwrap_or_else(|| {
            if level_now == "low" {
                "high".to_string()
            } else {
                "low".to_string()
            }
        });
    println!("{provider}/{model_id}: level {level_now:?} -> {target:?}");

    // ---- §7.6: the thinking level ----------------------------------------
    models::set_thinking_level(&chip.live, &target)
        .await
        .expect("the engine accepts a level the model supports");

    assert_eq!(
        state(&client)
            .await
            .get("thinkingLevel")
            .and_then(Value::as_str),
        Some(target.as_str()),
        "the level the engine now holds"
    );
    assert_eq!(
        chip.live
            .status()
            .expect("status")
            .control
            .thinking_level
            .as_deref(),
        Some(target.as_str()),
        "and the chip the user is looking at, without waiting for the pump"
    );

    // ---- §7.5: auto-compaction -------------------------------------------
    // The one chip change the engine emits **no** event for, which is why the host
    // re-reads `get_state` itself.
    let enabled = flag(&before, "autoCompactionEnabled");
    models::set_auto_compaction(&chip.live, !enabled)
        .await
        .expect("the engine accepts the toggle");

    assert_eq!(
        flag(&state(&client).await, "autoCompactionEnabled"),
        !enabled,
        "the engine's flag moved"
    );
    assert_eq!(
        chip.live
            .status()
            .expect("status")
            .control
            .auto_compaction_enabled,
        Some(!enabled),
        "and the toggle on screen, with no event to lean on"
    );

    // ---- §7.1: the model ------------------------------------------------
    models::set_model(&chip.live, &provider, &model_id)
        .await
        .expect("the engine accepts the model it is already using");

    let after = state(&client).await;
    assert_eq!(text(&after["model"], "provider"), provider);
    assert_eq!(text(&after["model"], "id"), model_id);
    assert_eq!(
        chip.live
            .status()
            .expect("status")
            .control
            .model
            .as_ref()
            .and_then(|model| model.id.clone()),
        Some(model_id.clone()),
        "the model chip follows the session, event or no event"
    );

    // ---- put both back, and prove it -------------------------------------
    models::set_auto_compaction(&chip.live, enabled)
        .await
        .expect("the toggle is restored");
    models::set_thinking_level(&chip.live, &level_now)
        .await
        .expect("the level is restored");

    let restored = state(&client).await;
    assert_eq!(flag(&restored, "autoCompactionEnabled"), enabled);
    assert_eq!(text(&restored, "thinkingLevel"), level_now);
    assert_eq!(
        chip.live
            .status()
            .expect("status")
            .control
            .thinking_level
            .as_deref(),
        Some(level_now.as_str())
    );

    chip.close().await;
}
