//! The engine's project registry, as far as the app needs it.
//!
//! `projects.json` is `{"version":1,"projects":[{"path","addedAt","hidden"}]}` — measured on
//! a real store, where it held a single entry while the session root held nine buckets. So
//! it is **not** the app's list of projects: that comes from the sessions themselves (each
//! header records its own `cwd`), which is the same reason `docs/12` §2.1 groups by working
//! directory. What this file adds is the one thing a bucket cannot say: that the user asked
//! for a project to be hidden.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

/// The registry's shape, with only the fields the app uses.
///
/// `addedAt` and `version` are ignored rather than modelled: nothing in the app orders or
/// migrates by them, and a field that is parsed but never read is a claim about a schema we
/// do not own.
#[derive(Debug, Deserialize)]
struct Registry {
    #[serde(default)]
    projects: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    path: String,
    #[serde(default)]
    hidden: bool,
}

/// The project paths the user has hidden, or an empty set when the file is absent.
///
/// Unreadable JSON yields an empty set: showing a project the user hid is a smaller failure
/// than hiding every project because one file could not be parsed.
pub fn hidden_projects(agent_dir: &Path) -> HashSet<String> {
    let Some(contents) = std::fs::read_to_string(crate::paths::projects_file(agent_dir)).ok()
    else {
        return HashSet::new();
    };
    let Ok(registry) = serde_json::from_str::<Registry>(&contents) else {
        return HashSet::new();
    };

    registry
        .projects
        .into_iter()
        .filter(|entry| entry.hidden)
        .map(|entry| entry.path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hidden_entries_are_reported() {
        let dir = std::env::temp_dir().join("omp-store-projects");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        std::fs::write(
            crate::paths::projects_file(&dir),
            r#"{"version":1,"projects":[
                {"path":"/tmp/visible","addedAt":"2026-09-11T08:13:16.778Z","hidden":false},
                {"path":"/tmp/hidden","addedAt":"2026-09-11T08:13:16.778Z","hidden":true}
            ]}"#,
        )
        .expect("write");

        let hidden = hidden_projects(&dir);
        assert!(hidden.contains("/tmp/hidden"));
        assert!(!hidden.contains("/tmp/visible"));
    }

    #[test]
    fn a_missing_file_is_an_empty_set() {
        let dir = std::env::temp_dir().join("omp-store-projects-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        assert!(hidden_projects(&dir).is_empty());
    }
}
