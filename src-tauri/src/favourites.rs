//! The models the user starred, in the order they starred them.
//!
//! App-owned, and it has to be: the engine has no favourite concept, so there is no
//! command to send and no state to mirror — this file and the picker's star button are
//! the whole feature (`docs/12` §14.3 lists it as an app-owned surface, §7.1 gives the
//! group drag-handles). Keeping the *order* app-side follows from the same place: the
//! engine's catalogue is a flat list of what is available, and the order a user wants
//! their own models in is not a property of that list.
//!
//! A key is `"provider/id"` — the pair `set_model` takes, and the only identity the
//! picker has. The id alone is not enough: two providers can serve the same model id
//! (a local `llama.cpp` provider and `openrouter` both carry `~anthropic/…`), and the
//! key has to survive a catalogue that no longer offers the row, so it is stored as
//! text rather than as an index into today's list.
//!
//! Storage is a JSON array under the app's config directory, written whole on every
//! change: the list is a few dozen short strings, and a file that is always a complete
//! answer cannot be half-applied.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// The store file, under the app's config directory.
const STORE_FILE: &str = "favourites.json";

/// The starred keys, and whether the file has been consulted yet.
pub struct Favourites {
    /// `None` when the platform gave no config directory: the list then lives for the
    /// process only, which is better than refusing to launch over a missing home.
    dir: Option<PathBuf>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Read from disk on first use, once — see [`Favourites::ensure_loaded`].
    loaded: bool,
    keys: Vec<String>,
}

impl Favourites {
    /// A store backed by `config_dir`, or by memory alone when there is none.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            dir: config_dir,
            state: Mutex::new(State::default()),
        }
    }

    /// The starred keys, in the user's order.
    pub fn list(&self) -> Result<Vec<String>, String> {
        let mut state = self.lock()?;
        self.ensure_loaded(&mut state);

        Ok(state.keys.clone())
    }

    /// Replace the list, and return what was stored.
    ///
    /// Refuses the whole write when any key is not a `provider/id` pair: a
    /// half-applied list would be an order the user never chose, and the caller gets the
    /// offending key back to show. Duplicates are dropped instead of refused — a star
    /// cannot be set twice, so a repeated key describes the same list — and the returned
    /// list is what the frontend should render, which is how a caller learns its input
    /// was normalised.
    ///
    /// The file is written before the in-memory list moves, so a write that fails leaves
    /// both unchanged and the caller can say the star was not saved. With no config
    /// directory there is nothing to write and the list lives for the process only.
    pub fn set(&self, keys: Vec<String>) -> Result<Vec<String>, String> {
        let mut stored: Vec<String> = Vec::with_capacity(keys.len());
        let mut seen = HashSet::with_capacity(keys.len());

        for key in keys {
            if split(&key).is_none() {
                return Err(format!(
                    "`{key}` is not a provider/id model key, so it cannot be a favourite"
                ));
            }
            if seen.insert(key.clone()) {
                stored.push(key);
            }
        }

        let mut state = self.lock()?;
        // A `set` is a whole list by contract, so the file is about to be replaced
        // either way; marking it loaded keeps a later `list` from re-reading it.
        state.loaded = true;

        if let Some(path) = self.path() {
            write(&path, &stored)
                .map_err(|error| format!("the favourites were not saved: {error}"))?;
        }

        state.keys = stored.clone();

        Ok(stored)
    }

    /// Read the store file the first time anything asks for the list.
    ///
    /// Lazy because the app's own display never depends on it: the picker asks when it
    /// opens, and a missing file is the normal state for a new user. A file that is
    /// unreadable, is not a JSON array, or holds a key this module would refuse to
    /// store is reported and treated as empty — the store's one invariant is that every
    /// key in it is a `provider/id` pair, and `list` must not be able to return
    /// something `set` would reject.
    fn ensure_loaded(&self, state: &mut State) {
        if state.loaded {
            return;
        }
        state.loaded = true;

        let Some(path) = self.path() else {
            return;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // No file yet is a new user, not a problem worth reporting.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                eprintln!(
                    "[omp-desktop] could not read the favourites at {}: {error}",
                    path.display()
                );
                return;
            }
        };

        let keys = match serde_json::from_str::<Vec<String>>(&text) {
            Ok(keys) => keys,
            Err(error) => {
                eprintln!(
                    "[omp-desktop] ignoring the unreadable favourites at {}: {error}",
                    path.display()
                );
                return;
            }
        };

        let total = keys.len();
        state.keys = keys
            .into_iter()
            .filter(|key| split(key).is_some())
            .collect();
        if state.keys.len() != total {
            eprintln!(
                "[omp-desktop] dropped {} favourites that are not provider/id keys",
                total - state.keys.len()
            );
        }
    }

    /// The store file's path, or `None` when there is no config directory.
    fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(STORE_FILE))
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, String> {
        self.state
            .lock()
            .map_err(|_| "the favourites lock was poisoned".to_string())
    }
}

/// Write the list, creating the directory if this is the first save.
fn write(path: &Path, keys: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, serde_json::to_vec(keys)?)
}

/// Split a favourite key into the provider and the model id it names.
///
/// `None` for anything the picker could not have produced: a key with no `/`, an empty
/// half, or whitespace. The split is at the *first* slash because a model id may contain
/// one (measured at v18.2.6: `~anthropic/claude-fable-latest` on `openrouter`), so only
/// the provider is required to be slash-free, and whitespace is refused because a key
/// names a model on the wire — a key that needed trimming would name one that does not
/// exist.
fn split(key: &str) -> Option<(&str, &str)> {
    let (provider, id) = key.split_once('/')?;
    let usable = !provider.is_empty() && !id.is_empty() && !key.contains(char::is_whitespace);

    usable.then_some((provider, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own: the store writes a real file.
    fn store_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "omp-desktop-favourites-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_order_is_the_users_and_survives_a_restart() {
        let dir = store_dir("order");
        let stored = Favourites::new(Some(dir.clone()))
            .set(keys(&[
                "openrouter/~anthropic/claude-fable-latest",
                "anthropic/x",
                "local/y",
            ]))
            .expect("the keys are well formed");

        assert_eq!(
            stored,
            keys(&[
                "openrouter/~anthropic/claude-fable-latest",
                "anthropic/x",
                "local/y"
            ]),
            "a model id may contain a slash; the provider is only the first segment"
        );

        // A fresh process: nothing in memory, everything from the file.
        let reopened = Favourites::new(Some(dir.clone()));
        assert_eq!(reopened.list().expect("the store reads"), stored);

        let file = std::fs::read_to_string(dir.join(STORE_FILE)).expect("the file exists");
        assert_eq!(
            file, r#"["openrouter/~anthropic/claude-fable-latest","anthropic/x","local/y"]"#,
            "the store is a plain JSON array of keys"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unusable_key_refuses_the_whole_write() {
        let dir = store_dir("invalid");
        let store = Favourites::new(Some(dir.clone()));
        store.set(keys(&["openrouter/x"])).expect("the first write");

        for bad in ["", "openrouter", "/x", "openrouter/", "open router/x"] {
            let refused = store
                .set(keys(&["anthropic/y", bad]))
                .expect_err("a key that is not a provider/id pair is refused");
            assert!(
                refused.contains(bad),
                "the message names the key: {refused}"
            );
        }

        // Nothing moved: neither the list nor the file it was loaded from.
        assert_eq!(
            store.list().expect("the store reads"),
            keys(&["openrouter/x"])
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(STORE_FILE)).expect("the file exists"),
            r#"["openrouter/x"]"#
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeated_key_stores_once_and_the_first_position_wins() {
        let dir = store_dir("duplicates");
        let store = Favourites::new(Some(dir.clone()));

        let stored = store
            .set(keys(&["anthropic/x", "openrouter/y", "anthropic/x"]))
            .expect("duplicates are normalised, not refused");

        assert_eq!(stored, keys(&["anthropic/x", "openrouter/y"]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_or_foreign_file_reads_as_empty_and_can_be_replaced() {
        let dir = store_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("the directory");
        std::fs::write(dir.join(STORE_FILE), b"{\"not\": \"an array\"}").expect("the file");

        let store = Favourites::new(Some(dir.clone()));
        assert!(store
            .list()
            .expect("a corrupt store reads as empty")
            .is_empty());

        store
            .set(keys(&["anthropic/x"]))
            .expect("the write succeeds");
        assert_eq!(
            store.list().expect("the store reads"),
            keys(&["anthropic/x"])
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_the_store_would_refuse_never_comes_back_out_of_it() {
        let dir = store_dir("filtered");
        std::fs::create_dir_all(&dir).expect("the directory");
        std::fs::write(dir.join(STORE_FILE), br#"["anthropic/x","not-a-key"]"#).expect("the file");

        let store = Favourites::new(Some(dir.clone()));
        assert_eq!(
            store.list().expect("the store reads"),
            keys(&["anthropic/x"]),
            "`list` must not be able to return something `set` would reject"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_with_no_config_directory_lives_for_the_process() {
        let store = Favourites::new(None);
        assert!(store
            .list()
            .expect("nothing cached is not an error")
            .is_empty());

        let stored = store
            .set(keys(&["anthropic/x"]))
            .expect("the write succeeds");
        assert_eq!(store.list().expect("the store reads"), stored);
    }
}
