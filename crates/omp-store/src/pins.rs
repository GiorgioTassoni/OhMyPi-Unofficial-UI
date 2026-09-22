//! The pin set, which the engine keeps beside its sessions rather than inside them.
//!
//! `session-pins.json` is a flat JSON array of session **ids** (not paths), so a pin
//! survives a `/move` that renames the file. Measured at v18.2.6: `session-pins.ts` writes
//! it under a cross-process file lock with a temp-then-replace commit, and the read side
//! degrades a corrupt file to an empty set rather than breaking the resume picker. The app
//! mirrors the read; it does not write (see `docs/12` §2.3: pinning is dispatched to the
//! engine's own `/pin`, because OMP's resume ordering is the single source of truth).

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

/// The pinned session ids, or an empty set when the file is missing, unreadable, or not an
/// array of strings.
///
/// An empty answer for a corrupt file is deliberate: a pin is presentation, and refusing to
/// list a project's sessions because one JSON array was truncated would be a worse failure.
pub fn pinned_ids(agent_dir: &Path) -> HashSet<String> {
    let Some(contents) = std::fs::read_to_string(crate::paths::pins_file(agent_dir)).ok() else {
        return HashSet::new();
    };
    let Some(Value::Array(ids)) = serde_json::from_str::<Value>(&contents).ok() else {
        return HashSet::new();
    };

    ids.iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_an_empty_set() {
        let dir = std::env::temp_dir().join("omp-store-pins-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        assert!(pinned_ids(&dir).is_empty());
    }

    #[test]
    fn ids_are_read_and_a_corrupt_file_degrades_to_empty() {
        let dir = std::env::temp_dir().join("omp-store-pins-corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        std::fs::write(crate::paths::pins_file(&dir), "[\"a\", \"b\"]").expect("write");
        let ids = pinned_ids(&dir);
        assert!(ids.contains("a") && ids.contains("b") && ids.len() == 2);

        std::fs::write(crate::paths::pins_file(&dir), "[{\"not\":\"an id\"}").expect("write");
        assert!(pinned_ids(&dir).is_empty());
    }
}
