//! The model catalogue: the picker's rows, cached, and the chip row's engine commands.
//!
//! `docs/12` §7.1 forces the shape of this module with two measured properties of
//! `get_available_models` at v18.2.6: the answer is **601 rows / 1540 KiB** of JSON, and
//! the first call of a process may refresh from the network (measured today: 1.20 s
//! cold, 35 ms warm; §7.1 records a cold run that had not answered after 12 s). A
//! picker that fetched on open would show an empty list for seconds, so the catalogue
//! is fetched out of band, held in memory, and **persisted**, which is what makes the
//! next launch open against a real list.
//!
//! The same popovers send the four commands that change a session: the model, the
//! thinking level, auto-compaction, and compaction itself. They live here rather than in
//! `bridge.rs` because of the house rule that the adapter decides nothing — what these
//! commands do to the host's own control snapshot (below) is a decision.
//!
//! # Why a chip command re-reads `get_state`
//!
//! The chrome that sends one of these commands renders `SessionControl`, and the
//! frontend learns the new value by reading `status()`. Two of the four are covered by
//! events, but lazily: `model_changed` and `thinking_level_changed` reach the pump after
//! the command has already answered. The auto-compaction flag has **no event at all** —
//! measured: toggling it produces no `auto_compaction_*` frame, because those describe a
//! compaction *running*, not the setting. So a command that did not re-read would leave
//! §7.5's toggle showing its old value until the next turn ended, which is why
//! [`LiveSession::reread_control`] exists and why every sender below ends with it.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use omp_transport::protocol::{self, commands};
use omp_transport::OmpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::dto::{ModelCatalogue, ModelOption, MODELS_EVENT};
use crate::session::{self, LiveSession};

/// How long the engine gets to answer `get_available_models`.
///
/// Longer than the client's default: the first call in a process refreshes the
/// catalogue from the network, and the fetch is off the UI path, so waiting beats
/// giving up on a list the picker needs.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// The cache file, under the app's config directory.
///
/// The app's own file, not the engine's: the engine keeps its own catalogue store on
/// disk (`~/.omp/agent/models.db` at v18.2.6), and reading that one would tie this app
/// to a schema that is the engine's to change — the same reason the DTOs are written
/// out by hand rather than derived.
const CACHE_FILE: &str = "models.json";

/// The catalogue, and the one fetch that may be in flight.
#[derive(Clone)]
pub struct Catalogue {
    /// `None` when the platform gave no config directory. The catalogue then lives for
    /// the process only, which is still better than refusing to launch over a missing
    /// home directory.
    dir: Option<PathBuf>,
    cache: Arc<Mutex<Cache>>,
}

/// The in-memory cache, plus whether the file has been consulted yet.
#[derive(Debug, Default)]
struct Cache {
    /// Read from disk on first use, once — see [`Catalogue::ensure_loaded`].
    loaded: bool,
    options: Vec<ModelOption>,
    fetched_at: Option<u64>,
    refreshing: bool,
}

/// The cache file as it is read back.
///
/// Not [`ModelCatalogue`] itself: `refreshing` describes *this* process, and a previous
/// launch's file has nothing to say about it. The rows are the same type the picker
/// gets, so the two cannot drift; the round trip is pinned by a test.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cached {
    fetched_at: Option<u64>,
    options: Vec<ModelOption>,
}

/// The same file as it is written, borrowing the rows the cache already holds.
///
/// A second type rather than a clone: this runs on a fetch of 601 rows, and copying
/// every row to hand it to the serializer would be work whose only purpose is to make
/// the two halves of the format look alike.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile<'a> {
    fetched_at: Option<u64>,
    options: &'a [ModelOption],
}

impl Catalogue {
    /// A cache backed by `config_dir`, or by memory alone when there is none.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            dir: config_dir,
            cache: Arc::new(Mutex::new(Cache::default())),
        }
    }

    /// The catalogue as the picker reads it.
    ///
    /// Never blocks on the network. The worst case is the first call in a process,
    /// which reads the cache file once; everything after that is the in-memory rows.
    pub fn snapshot(&self) -> Result<ModelCatalogue, String> {
        let mut cache = self.lock()?;
        self.ensure_loaded(&mut cache);

        Ok(ModelCatalogue {
            options: cache.options.clone(),
            fetched_at: cache.fetched_at,
            refreshing: cache.refreshing,
        })
    }

    /// Fetch through `fetch`, replace the cache, persist it, and announce it.
    ///
    /// **A second call while a fetch is in flight returns immediately**, without
    /// fetching. Returns rather than joins, for two reasons: a picker's refresh
    /// affordance is idempotent, so making the second caller wait on work it did not
    /// start buys it nothing, and a 1.5 MB round trip cannot answer anything the
    /// in-flight one will not. Whichever fetch is running announces the result through
    /// [`MODELS_EVENT`], so both callers learn the same thing at the same moment.
    ///
    /// The error is the fetch's own, so a failure reaches whoever asked for it with the
    /// engine's message — and the sink is told on that path too, because the picker's
    /// refreshing state has to clear whatever happened to the fetch.
    ///
    /// `fetch` is a parameter rather than a call to [`fetch`] so this cache can be
    /// exercised without an engine; the two callers are the `refresh_models` command and
    /// the tests.
    pub async fn refresh<F>(&self, sink: &impl CatalogueSink, fetch: F) -> Result<(), String>
    where
        F: Future<Output = Result<Vec<ModelOption>, String>>,
    {
        {
            let mut cache = self.lock()?;
            self.ensure_loaded(&mut cache);
            if cache.refreshing {
                return Ok(());
            }
            cache.refreshing = true;
        }

        // Outside the lock: a fetch takes seconds, and `snapshot` must not wait on it.
        let fetched = fetch.await;

        let outcome = {
            let mut cache = self.lock()?;
            cache.refreshing = false;

            match fetched {
                Ok(options) => {
                    cache.options = options;
                    cache.fetched_at = Some(now_millis());
                    self.persist(&cache);
                    Ok(())
                }
                // The rows already cached stay: a catalogue that could not be refreshed
                // is still the last one the engine gave, and showing nothing instead
                // would be a worse answer to "what can I choose?".
                Err(error) => Err(error),
            }
        };

        sink.catalogue(self.snapshot()?);

        outcome
    }

    /// Read the cache file the first time anything asks for the catalogue.
    ///
    /// Lazy rather than at startup, so resolving the config directory, reading and
    /// parsing 1540 KiB cost nothing until a picker actually opens. A file that is
    /// missing, unreadable or not the shape we wrote is reported and treated as empty:
    /// the picker then simply has nothing cached to show until its first refresh, which
    /// is an inconvenience rather than a crash on launch.
    fn ensure_loaded(&self, cache: &mut Cache) {
        if cache.loaded {
            return;
        }
        cache.loaded = true;

        let Some(path) = self.path() else {
            return;
        };

        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // No file yet is a first launch, not a problem worth reporting.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                eprintln!(
                    "[omp-desktop] could not read the model cache at {}: {error}",
                    path.display()
                );
                return;
            }
        };

        match serde_json::from_str::<Cached>(&text) {
            Ok(cached) => {
                cache.options = cached.options;
                cache.fetched_at = cached.fetched_at;
            }
            Err(error) => eprintln!(
                "[omp-desktop] ignoring the unreadable model cache at {}: {error}",
                path.display()
            ),
        }
    }

    /// Write the cache for the next launch.
    ///
    /// Best-effort and reported rather than returned: the fetch that produced these
    /// rows succeeded, and failing the command because a file could not be written
    /// would tell the picker its rows are stale when they are the freshest it has. What
    /// a failed write costs is the *next* launch's starting list.
    fn persist(&self, cache: &Cache) {
        let Some(path) = self.path() else {
            return;
        };

        let write = || -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let cached = CacheFile {
                fetched_at: cache.fetched_at,
                options: &cache.options,
            };
            std::fs::write(&path, serde_json::to_vec(&cached)?)
        };

        if let Err(error) = write() {
            eprintln!(
                "[omp-desktop] could not write the model cache at {}: {error}",
                path.display()
            );
        }
    }

    /// The cache file's path, or `None` when there is no config directory.
    fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(CACHE_FILE))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Cache>, String> {
        self.cache
            .lock()
            .map_err(|_| "the model catalogue lock was poisoned".to_string())
    }
}

/// Where a landed catalogue fetch is announced.
///
/// A seam rather than a direct `AppHandle` dependency, for the reason
/// [`crate::session::ActivitySink`] gives: it keeps the cache free of Tauri types, so it
/// can be exercised headlessly — by this module's tests and by `tests/chip_host.rs`.
pub trait CatalogueSink: Send + Sync + 'static {
    /// The whole catalogue, after any fetch that landed.
    fn catalogue(&self, catalogue: ModelCatalogue);
}

impl CatalogueSink for AppHandle {
    fn catalogue(&self, catalogue: ModelCatalogue) {
        // `Emitter::emit` needs the event name; the sink exists so callers do not.
        let _ = self.emit(MODELS_EVENT, catalogue);
    }
}

/// So one sink can be shared, as [`crate::session::ActivitySink`] allows.
impl<T: CatalogueSink + ?Sized> CatalogueSink for std::sync::Arc<T> {
    fn catalogue(&self, catalogue: ModelCatalogue) {
        (**self).catalogue(catalogue);
    }
}

/// Fetch the catalogue from a live session.
///
/// Takes the client rather than the session: the catalogue is a property of the engine
/// build and its provider configuration, not of one conversation, so every session in
/// the app answers with the same rows.
pub async fn fetch(client: &OmpClient) -> Result<Vec<ModelOption>, String> {
    let response = client
        .request(commands::get_available_models(), Some(FETCH_TIMEOUT))
        .await
        .map_err(|error| format!("the model catalogue did not arrive: {error}"))?;

    if !protocol::is_success(&response) {
        return Err(response
            .get("error")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| "the engine refused to list its models".to_string()));
    }

    parse(&response)
}

/// Every row of a `get_available_models` response, as picker rows.
///
/// The shape is `{"data": {"models": [...]}}` — measured at v18.2.6, where the array
/// itself is 1540 KiB and arrives through the client's chunk reassembly.
pub fn parse(response: &Value) -> Result<Vec<ModelOption>, String> {
    let rows = response
        .get("data")
        .and_then(|data| data.get("models"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "the model catalogue did not arrive in the shape the engine documents".to_string()
        })?;

    let options: Vec<ModelOption> = rows.iter().filter_map(option).collect();

    // A row the app cannot select is dropped, and the drop is reported: silently serving
    // 599 of 601 rows would hide exactly the upstream change that breaks the picker.
    if options.len() != rows.len() {
        eprintln!(
            "[omp-desktop] dropped {} of {} catalogue rows the picker cannot select",
            rows.len() - options.len(),
            rows.len()
        );
    }

    Ok(options)
}

/// One engine row, as the picker's row.
///
/// `None` when the row has no `id` or no `provider`: `set_model{provider, modelId}`
/// needs both, and a row that cannot be selected is worse on screen than absent.
///
/// The rest is defensive rather than measured. `name` is present on all 601 rows at
/// v18.2.6, so falling back to the id exists for a provider that names models by id
/// alone — and because a row with an empty name renders as a blank line in the picker.
pub fn option(row: &Value) -> Option<ModelOption> {
    let id = string(row, "id")?;
    let provider = string(row, "provider")?;
    let reasoning = row
        .get("reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let efforts = efforts(row, reasoning);
    let default_effort = thinking_object(row).and_then(|block| string(block, "defaultLevel"));

    Some(ModelOption {
        provider,
        name: string(row, "name").unwrap_or_else(|| id.clone()),
        id,
        context_window: context_window(row),
        reasoning,
        default_effort,
        efforts,
    })
}

/// A non-empty string field, or `None`.
///
/// Empty counts as absent: an empty provider or name is not something the picker can
/// render, and `set_model` with an empty provider would be a request the engine refuses.
fn string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

/// The model's supported thinking efforts.
///
/// Handles both formats emitted across engine versions and RPC endpoints:
/// 1. `thinking` as an array of strings: `["low", "medium", "high", "max"]`
/// 2. `thinking` as an object: `{ "efforts": [...] }` or `{ "levels": [...] }`
///
/// When a model declares `reasoning: true` but specifies no explicit levels,
/// returns a standard reasoning effort ladder rather than an empty array.
fn efforts(row: &Value, reasoning: bool) -> Vec<String> {
    if let Some(arr) = row.get("thinking").and_then(Value::as_array) {
        let list: Vec<String> = arr
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
        if !list.is_empty() {
            return list;
        }
    }

    if let Some(obj) = thinking_object(row) {
        if let Some(arr) = obj
            .get("efforts")
            .or_else(|| obj.get("levels"))
            .and_then(Value::as_array)
        {
            let list: Vec<String> = arr
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect();
            if !list.is_empty() {
                return list;
            }
        }
    }

    if reasoning {
        vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "max".to_string(),
        ]
    } else {
        Vec::new()
    }
}

/// The model's `thinking` block, when structured as an object.
fn thinking_object(row: &Value) -> Option<&Value> {
    row.get("thinking").filter(|block| block.is_object())
}

/// `contextWindow`, when the engine gave a number.
///
/// `None` covers a missing field and a non-numeric one alike: the picker shows the
/// window as a fact, so a `0` invented here would be shown as one. Read as a float to
/// accept a JSON writer that emits `1e6`, clamped away from a value no window can be.
fn context_window(row: &Value) -> Option<u64> {
    let window = row.get("contextWindow")?.as_f64()?;
    (window.is_finite() && window >= 0.0).then_some(window as u64)
}

/// Unix milliseconds, or 0 for a clock set before the epoch.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Ask the engine to make `provider/id` the session's model.
pub async fn set_model(live: &LiveSession, provider: &str, model_id: &str) -> Result<(), String> {
    send(
        live,
        commands::set_model(provider, model_id),
        "model change",
    )
    .await
}

/// Ask the engine for a new thinking level (`docs/12` §7.6).
///
/// The level is not validated here against the model's `efforts`: the picker has that
/// list in front of it, and the engine's own refusal — returned verbatim — is the
/// authority on what a model will do.
pub async fn set_thinking_level(live: &LiveSession, level: &str) -> Result<(), String> {
    send(live, commands::set_thinking_level(level), "thinking level").await
}

/// Turn the engine's automatic compaction on or off (`docs/12` §7.5).
pub async fn set_auto_compaction(live: &LiveSession, enabled: bool) -> Result<(), String> {
    send(
        live,
        commands::set_auto_compaction(enabled),
        "auto-compaction change",
    )
    .await
}

/// Compact now, with the popover's optional free-text instructions.
pub async fn compact(live: &LiveSession, instructions: Option<&str>) -> Result<(), String> {
    send(live, commands::compact(instructions), "compaction").await
}

/// Send one chip-row command and leave the control snapshot current.
///
/// See the module docs for why the re-read is here rather than left to the pump.
async fn send(live: &LiveSession, command: Value, what: &str) -> Result<(), String> {
    let client = live
        .client()
        .ok_or_else(|| "the session is shutting down".to_string())?;

    session::send(&client, command, what).await?;
    // Logged and dropped: the command was accepted and the state it changed is what this
    // re-reads, so a miss is a stale chip rather than a failed action — but a *silent*
    // miss is how a stale chip becomes unexplainable.
    if let Err(error) = live.reread_control().await {
        eprintln!("[omp-desktop] could not refresh session state: {error}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::oneshot;

    use super::*;

    /// The measured row, verbatim from v18.2.6.
    fn measured_row() -> Value {
        json!({
            "contextWindow": 1000000,
            "id": "~anthropic/claude-fable-latest",
            "name": "Claude Fable Latest",
            "provider": "openrouter",
            "reasoning": true,
            "thinking": {
                "defaultLevel": "high",
                "efforts": ["low", "medium", "high", "xhigh", "max"],
                "mode": "effort",
                "requiresEffort": true
            }
        })
    }

    /// A directory of this test's own: the cache writes real files, so two tests must
    /// not share one.
    fn cache_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("omp-desktop-models-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A sink that keeps the last catalogue it was handed.
    #[derive(Default)]
    struct LastCatalogue(Mutex<Option<ModelCatalogue>>);

    impl LastCatalogue {
        fn last(&self) -> Option<ModelCatalogue> {
            self.0.lock().ok().and_then(|last| last.clone())
        }
    }

    impl CatalogueSink for LastCatalogue {
        fn catalogue(&self, catalogue: ModelCatalogue) {
            if let Ok(mut last) = self.0.lock() {
                *last = Some(catalogue);
            }
        }
    }

    /// A row, for the tests that only care about identity.
    fn row(provider: &str, id: &str) -> ModelOption {
        ModelOption {
            provider: provider.to_string(),
            id: id.to_string(),
            name: id.to_string(),
            context_window: None,
            reasoning: false,
            default_effort: None,
            efforts: Vec::new(),
        }
    }

    #[test]
    fn a_measured_row_decodes_into_the_picker_row() {
        let option = option(&measured_row()).expect("the row is selectable");

        assert_eq!(option.provider, "openrouter");
        assert_eq!(option.id, "~anthropic/claude-fable-latest");
        assert_eq!(option.name, "Claude Fable Latest");
        assert_eq!(option.context_window, Some(1_000_000));
        assert!(option.reasoning);
        assert_eq!(option.default_effort.as_deref(), Some("high"));
        assert_eq!(option.efforts, ["low", "medium", "high", "xhigh", "max"]);
    }

    #[test]
    fn absent_or_unusable_fields_do_not_invent_values() {
        // A model that does not think carries no `thinking` block at all.
        let decoded = option(&json!({"id": "local/one", "provider": "llama.cpp"}))
            .expect("a row without thinking is still selectable");
        assert_eq!(decoded.context_window, None);
        assert_eq!(decoded.default_effort, None);
        assert!(decoded.efforts.is_empty());
        assert!(!decoded.reasoning);
        assert_eq!(decoded.name, "local/one", "the name falls back to the id");

        // A non-numeric window is absent, not zero: the picker renders it as a fact.
        let decoded = option(&json!({
            "id": "local/two",
            "provider": "llama.cpp",
            "contextWindow": "128k",
            "name": ""
        }))
        .expect("a row is selectable");
        assert_eq!(decoded.context_window, None);
        assert_eq!(
            decoded.name, "local/two",
            "an empty name falls back to the id"
        );
    }

    #[test]
    fn only_string_efforts_survive_and_rows_without_identity_are_dropped() {
        let options = parse(&json!({"data": {"models": [
            {"id": "a/1", "provider": "p", "thinking": {"efforts": ["low", 7, "high", null]}},
            {"id": "a/2"},
            {"provider": "p"},
            {"id": "", "provider": "p"},
        ]}}))
        .expect("the payload is the documented shape");

        assert_eq!(options.len(), 1, "got {options:?}");
        assert_eq!(options[0].efforts, ["low", "high"]);
    }

    #[test]
    fn a_payload_that_is_not_the_documented_shape_is_refused() {
        let refused = parse(&json!({"data": {}})).expect_err("no models array");
        assert!(refused.contains("shape"), "got {refused}");
    }

    #[test]
    fn array_thinking_and_implicit_reasoning_efforts_decode() {
        let array_row = json!({
            "id": "qwen-custom",
            "provider": "llama.cpp",
            "reasoning": true,
            "thinking": ["low", "medium", "high", "max"]
        });
        let decoded = option(&array_row).expect("selectable");
        assert!(decoded.reasoning);
        assert_eq!(decoded.efforts, ["low", "medium", "high", "max"]);

        let implicit_row = json!({
            "id": "local-reasoning",
            "provider": "llama.cpp",
            "reasoning": true
        });
        let decoded_implicit = option(&implicit_row).expect("selectable");
        assert!(decoded_implicit.reasoning);
        assert_eq!(decoded_implicit.efforts, ["low", "medium", "high", "max"]);
    }

    #[test]
    fn the_catalogue_survives_a_restart_and_a_corrupt_file_does_not_crash_it() {
        let dir = cache_dir("persist");
        let sink = LastCatalogue::default();

        let first = Catalogue::new(Some(dir.clone()));
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(first.refresh(
                &sink,
                std::future::ready(Ok(vec![row("openrouter", "one")])),
            ))
            .expect("the fetch lands");

        // A fresh process: nothing in memory, everything from the file.
        let second = Catalogue::new(Some(dir.clone()));
        let cached = second.snapshot().expect("the cache reads");
        assert_eq!(cached.options.len(), 1);
        assert_eq!(cached.options[0].id, "one");
        assert!(cached.fetched_at.is_some());
        assert!(!cached.refreshing);

        // A file written by a different version, or by a half-finished write.
        std::fs::write(dir.join(CACHE_FILE), b"{ this is not the cache }").expect("the file");
        let third = Catalogue::new(Some(dir.clone()));
        assert!(third
            .snapshot()
            .expect("a corrupt cache reads as empty")
            .options
            .is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fetch that fails is announced too.
    ///
    /// The picker's spinner is cleared by the announcement, not by the command's
    /// rejection — a frontend that only listened for success would sit on `refreshing`
    /// for ever — and the rows it keeps are the last ones that landed, not none.
    #[tokio::test]
    async fn a_failed_fetch_keeps_the_rows_and_still_announces() {
        let dir = cache_dir("failure");
        let catalogue = Catalogue::new(Some(dir.clone()));
        let sink = LastCatalogue::default();

        catalogue
            .refresh(&sink, async { Ok(vec![row("openrouter", "one")]) })
            .await
            .expect("the first fetch lands");
        let refusal = "the engine refused to list its models".to_string();

        let error = catalogue
            .refresh(&sink, {
                let refusal = refusal.clone();
                async move { Err(refusal) }
            })
            .await
            .expect_err("the failure reaches whoever asked for it");

        assert_eq!(error, refusal);
        let announced = sink.last().expect("the failure is announced");
        assert!(!announced.refreshing, "the spinner has to clear");
        assert_eq!(
            announced.options.len(),
            1,
            "the rows from the last successful fetch stay"
        );
        assert!(!catalogue.snapshot().expect("the cache reads").refreshing);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The in-flight guard, observed the way a caller can observe it: the second call
    /// returns while the first is still parked, the fetch seam runs once, and the public
    /// snapshot reports the refresh that is running. Nothing here reads a field.
    #[tokio::test]
    async fn a_second_refresh_does_not_start_a_second_fetch() {
        let dir = cache_dir("dedup");
        let catalogue = Arc::new(Catalogue::new(Some(dir.clone())));

        let fetches = Arc::new(AtomicUsize::new(0));
        let (entered, first_in_flight) = oneshot::channel::<()>();
        let (release, gate) = oneshot::channel::<()>();

        let first = {
            let catalogue = Arc::clone(&catalogue);
            let fetches = Arc::clone(&fetches);
            tokio::spawn(async move {
                let sink = LastCatalogue::default();
                catalogue
                    .refresh(&sink, async move {
                        fetches.fetch_add(1, Ordering::SeqCst);
                        let _ = entered.send(());
                        let _ = gate.await;
                        Ok(vec![row("openrouter", "one")])
                    })
                    .await
            })
        };

        first_in_flight.await.expect("the fetch is entered");
        assert!(
            catalogue.snapshot().expect("the cache reads").refreshing,
            "a fetch in flight is what the picker's spinner reads"
        );

        let sink = LastCatalogue::default();
        tokio::time::timeout(
            Duration::from_secs(5),
            catalogue.refresh(&sink, async {
                fetches.fetch_add(1, Ordering::SeqCst);
                Ok(vec![row("openrouter", "two")])
            }),
        )
        .await
        .expect("the second refresh returns rather than waiting on the first")
        .expect("the second refresh is not an error");

        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "the second refresh must not start a fetch of its own"
        );

        release.send(()).expect("the first fetch is released");
        first
            .await
            .expect("the first refresh task")
            .expect("the first refresh lands");

        let settled = catalogue.snapshot().expect("the cache reads");
        assert!(!settled.refreshing, "the guard clears when a fetch lands");
        assert_eq!(settled.options.len(), 1);
        assert_eq!(settled.options[0].id, "one", "the first fetch's rows won");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
