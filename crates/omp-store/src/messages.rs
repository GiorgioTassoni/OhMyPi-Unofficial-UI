//! Reading a session's *content*, for the search index (`docs/12` §7.4, decision D7).
//!
//! [`crate::listing`] reads enough of a file to describe it; this reads the file to *quote*
//! it. The difference matters for cost: a listing reads two windows (4 KiB + 32 KiB) of a
//! file that can be 25 MB, while an index has to see every message once — so this streams
//! line by line and never holds the file, and it caps what one record may contribute so a
//! single 20 MB tool result cannot become the index.
//!
//! The three kinds are the engine's own message roles plus the two things a search actually
//! looks for, taken from the entries as they are written rather than from the app's rendered
//! rows: a cold session has no rows, and searching must cover every session on disk, not
//! only the ones that have been opened.
//!
//! ```text
//!   line 1        {"type":"title","title":"…"}                    → Title
//!   {"type":"message","message":{"role":"user","content":[…]}}     → Prompt (text blocks)
//!   {"type":"message","message":{"role":"assistant","content":[…]}} → Answer (text) + Thinking
//!                                                                  + Tool (toolCall name+input)
//!   {"type":"message","message":{"role":"toolResult","content":[…]}} → Result
//! ```

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// How much of one record the index may keep.
///
/// A tool result can be tens of megabytes (a directory listing, a log, a whole file read),
/// and an index that stored them would be a copy of the store. The first few kilobytes are
/// what a search hit needs to *find* the message and show a snippet; the rest is one click
/// away in the thread itself. Truncation is marked so a snippet never pretends to be
/// complete.
const MAX_RECORD_BYTES: usize = 8 * 1024;

/// What a record is, from the search UI's point of view — a hit on reasoning reads
/// differently from a hit on a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// The session's title, from the in-place slot.
    Title,
    /// A user message.
    Prompt,
    /// An assistant message's answer text.
    Answer,
    /// A model's reasoning. Indexed because it is often where a decision is explained, and
    /// labelled so a hit can say so.
    Thinking,
    /// A tool call: its name and its input, which is where a path or a command lives.
    Tool,
    /// A tool's result text.
    Result,
}

impl RecordKind {
    /// The engine's spelling, for the index's columns and the UI's labels.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Title => "title",
            RecordKind::Prompt => "prompt",
            RecordKind::Answer => "answer",
            RecordKind::Thinking => "thinking",
            RecordKind::Tool => "tool",
            RecordKind::Result => "result",
        }
    }
}

/// One searchable piece of a session, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Where in the session's message stream this came from, for ordering hits in a thread
    /// and for a jump target. Counts `message` entries, so reasoning and tool calls from the
    /// same message share an ordinal with their answer — which is what a reader expects.
    pub ordinal: u64,
    pub kind: RecordKind,
    pub text: String,
}

/// Every record in one session file, in order.
///
/// An unreadable file yields an empty list rather than an error: one truncated file must not
/// stop the index from covering the rest, the same way `listing` skips what it cannot parse.
/// Nothing that is not a `message` entry (or the title slot) becomes a record — bookkeeping
/// such as `title_change`, `session_exit` and `custom` markers is not content.
pub fn read_records(path: &Path) -> Vec<Record> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    let mut ordinal = 0_u64;
    let mut first_line = true;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }

        // The title slot is line 1 and holds the *current* title: the engine rewrites it in
        // place on every rename, so it is both the freshest and the cheapest thing here.
        if first_line {
            first_line = false;
            if let Ok(entry) = serde_json::from_str::<Value>(trimmed) {
                if entry.get("type").and_then(Value::as_str) == Some("title") {
                    if let Some(title) = entry.get("title").and_then(Value::as_str) {
                        if !title.trim().is_empty() {
                            records.push(Record {
                                ordinal: 0,
                                kind: RecordKind::Title,
                                text: title.trim().to_string(),
                            });
                        }
                    }
                    continue;
                }
                // No slot: this line is something else, and it is still worth reading.
                collect(&mut records, &entry, &mut ordinal);
                continue;
            }
            continue;
        }

        let Ok(entry) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        collect(&mut records, &entry, &mut ordinal);
    }

    records
}

/// Turn one entry into records, advancing the ordinal once per message.
fn collect(records: &mut Vec<Record>, entry: &Value, ordinal: &mut u64) {
    if entry.get("type").and_then(Value::as_str) != Some("message") {
        return;
    }
    let Some(message) = entry.get("message") else {
        return;
    };
    *ordinal += 1;
    let at = *ordinal;

    let role = message.get("role").and_then(Value::as_str).unwrap_or("");
    let blocks = message.get("content").and_then(Value::as_array);

    // A `toolResult` carries the same `content` shape as a message, and its text is where a
    // command's output (and an `edit`'s diff, when the engine puts it in the text) lives.
    if role == "toolResult" {
        let text = blocks.map(|blocks| text_of(blocks)).unwrap_or_default();
        push(records, at, RecordKind::Result, &text);
        return;
    }

    let mut answer = String::new();
    let mut reasoning = String::new();
    let mut tools: Vec<String> = Vec::new();

    for block in blocks.into_iter().flatten() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    // A prompt is one record regardless of how many blocks carry it; an
                    // answer is too, so a hit does not repeat the same message five times.
                    answer.push_str(text);
                    answer.push(' ');
                }
            }
            Some("thinking") => {
                // The reasoning field is named differently across providers, and both
                // spellings appear in real files.
                if let Some(text) = block
                    .get("thinking")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                {
                    reasoning.push_str(text);
                    reasoning.push(' ');
                }
            }
            Some("toolCall") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                let input = block
                    .get("input")
                    .map(|value| serde_json::to_string(value).unwrap_or_default())
                    .unwrap_or_default();
                tools.push(format!("{name} {input}"));
            }
            _ => {}
        }
    }

    let kind = match role {
        "user" => RecordKind::Prompt,
        "assistant" => RecordKind::Answer,
        // A `system`/`developer` message is context rather than a turn; its text is still
        // worth finding (a project's own instructions, for instance).
        _ => RecordKind::Answer,
    };

    push(records, at, kind, &answer);
    push(records, at, RecordKind::Thinking, &reasoning);
    for tool in tools {
        push(records, at, RecordKind::Tool, &tool);
    }
}

/// Append a record when it has anything to say, capped at [`MAX_RECORD_BYTES`].
fn push(records: &mut Vec<Record>, ordinal: u64, kind: RecordKind, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    records.push(Record {
        ordinal,
        kind,
        text: truncate(trimmed),
    });
}

/// Cut to the cap **at a character boundary**, marking that there is more.
///
/// Counting bytes rather than characters would split a multi-byte character and produce text
/// the index cannot store; a truncated record says so, because a snippet that silently stops
/// mid-sentence reads as the whole message.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_RECORD_BYTES {
        return text.to_string();
    }
    let mut end = MAX_RECORD_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &text[..end])
}

/// The joined text of a message's `content` blocks.
fn text_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}
