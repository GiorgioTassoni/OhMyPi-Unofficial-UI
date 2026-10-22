//! Reading archived conversation history from a session file on disk.
//!
//! When an `omp` session undergoes compaction, the sidecar RPC `get_messages_page`
//! only returns the active context window (messages from `firstKeptEntryId` onwards),
//! keeping earlier turns archived in the session's `.jsonl` DAG on disk.
//!
//! This module recovers archived user turns by traversing the session DAG backwards
//! from the leaf entry to root, grouping entries into user turns, and providing
//! chunked, on-demand restoration so the frontend never loads thousands of rows at once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use omp_session::{Message, MessageKind, Transcript};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dto::RowSnapshot;
use crate::session::row_snapshot;

/// Summary information about archived turns in a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedHistoryInfo {
    /// Total user turns in the entire session file.
    pub total_turns: usize,
    /// Number of turns that are currently archived on disk (prior to the active context).
    pub archived_turns: usize,
    /// Number of archived turns remaining to be loaded on demand.
    pub remaining_archived_turns: usize,
}

/// The result of loading a batch of earlier turns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadEarlierTurnsResult {
    /// The row snapshots of the loaded batch, in chronological order.
    pub rows: Vec<RowSnapshot>,
    /// How many archived turns remain after this batch has been loaded.
    pub remaining_archived_turns: usize,
}

/// Internal batch load outcome, tracking the updated loaded counter.
#[derive(Debug, Clone)]
pub struct BatchLoadOutcome {
    pub rows: Vec<RowSnapshot>,
    pub new_loaded: usize,
    pub remaining_archived_turns: usize,
}

/// One user turn and its attendant assistant and tool result messages.
#[derive(Debug, Clone)]
pub struct SessionTurn {
    pub user_entry_id: String,
    pub messages: Vec<Message>,
}

/// Read the active branch of a session file from leaf to root via `parentId`,
/// and identify the latest compaction cutoff if present.
///
/// Returns `(chain_root_to_leaf, latest_first_kept_entry_id)`.
pub fn read_chain_from_file(path: &Path) -> Result<(Vec<Value>, Option<String>), String> {
    let Ok(file) = File::open(path) else {
        return Ok((Vec::new(), None));
    };

    let mut entries: HashMap<String, Value> = HashMap::new();
    let mut last_id: Option<String> = None;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            let id_str = id.to_string();
            entries.insert(id_str.clone(), obj);
            last_id = Some(id_str);
        }
    }

    let Some(last_id) = last_id else {
        return Ok((Vec::new(), None));
    };

    // Trace from leaf back to root
    let mut chain = Vec::new();
    let mut cur = Some(last_id);
    while let Some(id) = cur {
        if let Some(entry) = entries.get(&id) {
            chain.push(entry.clone());
            cur = entry
                .get("parentId")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        } else {
            break;
        }
    }

    // Reverse to chronological order (root to leaf)
    chain.reverse();

    // Find the latest compaction in this branch
    let mut latest_first_kept = None;
    for entry in chain.iter().rev() {
        if entry.get("type").and_then(Value::as_str) == Some("compaction") {
            if let Some(fk) = entry.get("firstKeptEntryId").and_then(Value::as_str) {
                latest_first_kept = Some(fk.to_string());
                break;
            }
        }
    }

    Ok((chain, latest_first_kept))
}

/// Group a sequence of entries into user turns.
///
/// A user turn begins on any `message` entry with `role == "user"` and `synthetic != true`.
/// All subsequent messages until the next user turn belong to that turn.
pub fn parse_turns_from_chain(chain: &[Value]) -> Vec<SessionTurn> {
    let mut turns = Vec::new();
    let mut current_turn: Option<SessionTurn> = None;

    for entry in chain {
        let entry_type = entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if entry_type == "message" {
            let msg = Message::decode(entry);
            let is_user = match &msg.kind {
                MessageKind::User { synthetic, .. } => !synthetic,
                _ => false,
            };

            if is_user {
                if let Some(turn) = current_turn.take() {
                    turns.push(turn);
                }
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                current_turn = Some(SessionTurn {
                    user_entry_id: id,
                    messages: vec![msg],
                });
            } else if let Some(ref mut turn) = current_turn {
                turn.messages.push(msg);
            }
        }
    }

    if let Some(turn) = current_turn {
        turns.push(turn);
    }

    turns
}

/// Calculate archived history stats for a session file given how many turns have
/// already been loaded into the client.
pub fn get_history_info(path: &Path, loaded: usize) -> Result<ArchivedHistoryInfo, String> {
    let (chain, first_kept_id) = read_chain_from_file(path)?;
    if chain.is_empty() {
        return Ok(ArchivedHistoryInfo {
            total_turns: 0,
            archived_turns: 0,
            remaining_archived_turns: 0,
        });
    }

    let total_turns = parse_turns_from_chain(&chain).len();

    let Some(first_kept_id) = first_kept_id else {
        return Ok(ArchivedHistoryInfo {
            total_turns,
            archived_turns: 0,
            remaining_archived_turns: 0,
        });
    };

    let first_kept_idx = chain
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(&first_kept_id));

    let Some(first_kept_idx) = first_kept_idx else {
        return Ok(ArchivedHistoryInfo {
            total_turns,
            archived_turns: 0,
            remaining_archived_turns: 0,
        });
    };

    let archived_turns = parse_turns_from_chain(&chain[..first_kept_idx]).len();
    let remaining_archived_turns = archived_turns.saturating_sub(loaded);

    Ok(ArchivedHistoryInfo {
        total_turns,
        archived_turns,
        remaining_archived_turns,
    })
}

/// Load a batch of archived turns (closest to active context first, in batches of `batch_size`).
pub fn load_batch(
    path: &Path,
    loaded: usize,
    batch_size: usize,
) -> Result<BatchLoadOutcome, String> {
    let (chain, first_kept_id) = read_chain_from_file(path)?;
    let Some(first_kept_id) = first_kept_id else {
        return Ok(BatchLoadOutcome {
            rows: Vec::new(),
            new_loaded: 0,
            remaining_archived_turns: 0,
        });
    };

    let first_kept_idx = chain
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(&first_kept_id));

    let Some(first_kept_idx) = first_kept_idx else {
        return Ok(BatchLoadOutcome {
            rows: Vec::new(),
            new_loaded: 0,
            remaining_archived_turns: 0,
        });
    };

    let archived_turns = parse_turns_from_chain(&chain[..first_kept_idx]);
    let total_archived = archived_turns.len();

    if loaded >= total_archived || batch_size == 0 {
        return Ok(BatchLoadOutcome {
            rows: Vec::new(),
            new_loaded: total_archived,
            remaining_archived_turns: 0,
        });
    }

    let count = batch_size.min(total_archived - loaded);
    let end_idx = total_archived - loaded;
    let start_idx = end_idx - count;

    let batch_turns = &archived_turns[start_idx..end_idx];
    let messages: Vec<Message> = batch_turns
        .iter()
        .flat_map(|t| t.messages.clone())
        .collect();

    let mut transcript = Transcript::new();
    transcript.restore(&messages);
    let rows: Vec<RowSnapshot> = transcript.rows().iter().map(row_snapshot).collect();

    let new_loaded = loaded + count;
    let remaining_archived_turns = total_archived - new_loaded;

    Ok(BatchLoadOutcome {
        rows,
        new_loaded,
        remaining_archived_turns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn empty_file_returns_zero_turns() {
        let file = NamedTempFile::new().unwrap();
        let info = get_history_info(file.path(), 0).unwrap();
        assert_eq!(info.total_turns, 0);
        assert_eq!(info.archived_turns, 0);
        assert_eq!(info.remaining_archived_turns, 0);
    }

    #[test]
    fn uncompacted_file_returns_zero_archived_turns() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"u1","parentId":null,"message":{{"role":"user","content":[{{"type":"text","text":"hello"}}]}}}}"#
        ).unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"a1","parentId":"u1","message":{{"role":"assistant","content":[{{"type":"text","text":"hi"}}]}}}}"#
        ).unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"u2","parentId":"a1","message":{{"role":"user","content":[{{"type":"text","text":"how are you"}}]}}}}"#
        ).unwrap();

        let info = get_history_info(file.path(), 0).unwrap();
        assert_eq!(info.total_turns, 2);
        assert_eq!(info.archived_turns, 0);
        assert_eq!(info.remaining_archived_turns, 0);

        let batch = load_batch(file.path(), 0, 5).unwrap();
        assert!(batch.rows.is_empty());
        assert_eq!(batch.remaining_archived_turns, 0);
    }

    #[test]
    fn compacted_file_chunks_archived_turns_chronologically() {
        let mut file = NamedTempFile::new().unwrap();
        // Create 6 user turns, then a compaction pointing to the 5th turn's assistant message
        let mut last = None;
        let mut turn_ids = Vec::new();
        for i in 1..=6 {
            let u_id = format!("u{i}");
            let parent_json = match &last {
                Some(p) => format!(r#""{p}""#),
                None => "null".to_string(),
            };
            writeln!(
                file,
                r#"{{"type":"message","id":"{u_id}","parentId":{parent_json},"message":{{"role":"user","content":[{{"type":"text","text":"prompt {i}"}}]}}}}"#
            ).unwrap();
            let a_id = format!("a{i}");
            writeln!(
                file,
                r#"{{"type":"message","id":"{a_id}","parentId":"{u_id}","message":{{"role":"assistant","content":[{{"type":"text","text":"answer {i}"}}]}}}}"#
            ).unwrap();
            last = Some(a_id.clone());
            turn_ids.push((u_id, a_id));
        }

        // Compaction entry keeping from u5 onwards (turns 1..4 are archived = 4 turns)
        let comp_id = "comp1";
        writeln!(
            file,
            r#"{{"type":"compaction","id":"{comp_id}","parentId":"{last_id}","firstKeptEntryId":"u5"}}"#,
            last_id = last.unwrap()
        ).unwrap();

        let info = get_history_info(file.path(), 0).unwrap();
        assert_eq!(info.total_turns, 6);
        assert_eq!(info.archived_turns, 4);
        assert_eq!(info.remaining_archived_turns, 4);

        // Load first batch of size 2 (should load turns 3 and 4)
        let batch1 = load_batch(file.path(), 0, 2).unwrap();
        assert_eq!(batch1.new_loaded, 2);
        assert_eq!(batch1.remaining_archived_turns, 2);
        // Turns 3 and 4 each have a user row and an assistant row = 4 rows
        assert_eq!(batch1.rows.len(), 4);
        assert_eq!(batch1.rows[0].text, "prompt 3");
        assert_eq!(batch1.rows[1].text, "answer 3");
        assert_eq!(batch1.rows[2].text, "prompt 4");
        assert_eq!(batch1.rows[3].text, "answer 4");

        // Load second batch of size 2 (should load turns 1 and 2)
        let batch2 = load_batch(file.path(), batch1.new_loaded, 2).unwrap();
        assert_eq!(batch2.new_loaded, 4);
        assert_eq!(batch2.remaining_archived_turns, 0);
        assert_eq!(batch2.rows.len(), 4);
        assert_eq!(batch2.rows[0].text, "prompt 1");
        assert_eq!(batch2.rows[1].text, "answer 1");
        assert_eq!(batch2.rows[2].text, "prompt 2");
        assert_eq!(batch2.rows[3].text, "answer 2");

        // Prepending batch2 then batch1 gives:
        // [batch2 (turns 1, 2), batch1 (turns 3, 4), active (turns 5, 6)]
        // Exactly turns 1, 2, 3, 4, 5, 6 in perfect chronological order!
    }
}
