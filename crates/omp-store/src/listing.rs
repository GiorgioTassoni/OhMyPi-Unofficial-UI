//! Reading one session file into a summary, with the engine's own rules.
//!
//! A session is a JSONL file whose first line is a fixed-width 256-byte title slot, whose
//! second is the header, and whose rest are entries:
//!
//! ```text
//!   {"type":"title","v":1,"title":"…","source":"auto","updatedAt":"…","pad":"   …"}
//!   {"type":"session","version":3,"id":"01a0bb7d-…","timestamp":"…","cwd":"/…","title":"…"}
//!   {"type":"message","id":"…","parentId":"…","message":{"role":"user","content":[…]}}
//!   {"type":"message","message":{"role":"assistant","stopReason":"stop","content":[…]}}
//! ```
//!
//! **Two windows, never the whole file.** The engine reads a 4 KiB prefix and a 32 KiB
//! tail (`session-listing.ts`'s `SESSION_LIST_PREFIX_BYTES`/`…SUFFIX_BYTES`), because a
//! session file can be 17 MB (measured on this machine) and a browser that read it whole
//! would be unusable. The two windows answer different questions: the prefix carries the
//! title, the header and the first message; the tail carries the *last* message, which is
//! the only thing that can say whether the session is finished, cut off, or waiting.
//!
//! Semantics below are transcribed from `session-listing.ts` rather than invented, because
//! the sidebar shows the same sessions the engine's own resume picker does — two
//! implementations that disagree would be a bug the user sees as a wrong badge.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

/// The engine's prefix window: the title slot, the header and the first message.
pub const PREFIX_BYTES: u64 = 4096;
/// The engine's tail window: large enough for a typical final turn, small enough to read
/// per file. A final message bigger than this is the one case that yields `unknown`.
pub const TAIL_BYTES: u64 = 32 * 1024;

/// What the last persisted message says about the session.
///
/// The variants are the engine's own strings (`SessionStatus` in `session-listing.ts`) so a
/// badge and the engine's picker agree on every file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The last assistant turn ended with no unanswered tool calls.
    Complete,
    /// Cut off mid-flight: trailing tool calls, a tool result never continued from, or a
    /// length-truncated turn.
    Interrupted,
    /// The last assistant turn was cancelled by the user.
    Aborted,
    /// The last assistant turn ended in an error.
    Error,
    /// A trailing user message with no assistant reply after it.
    Pending,
    /// Undeterminable: an empty/header-only file, or a final message larger than the tail.
    Unknown,
}

impl Lifecycle {
    /// The engine's spelling, for the UI and for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Lifecycle::Complete => "complete",
            Lifecycle::Interrupted => "interrupted",
            Lifecycle::Aborted => "aborted",
            Lifecycle::Error => "error",
            Lifecycle::Pending => "pending",
            Lifecycle::Unknown => "unknown",
        }
    }
}

/// One session, as the browser needs it.
///
/// Deliberately not the engine's whole `SessionInfo`: `allMessagesText` is a search index's
/// input rather than a list's, and it is left to whoever builds that index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// The engine's session id (the header's `id`, else the id in the filename).
    pub id: String,
    pub path: PathBuf,
    /// The bucket directory's name. A **hint**, not a path: bucket names are a lossy
    /// encoding of a cwd, and [`SessionSummary::cwd`] is the real answer.
    pub bucket: String,
    /// Where the session was started, from its own header. Empty for old sessions.
    pub cwd: String,
    /// The title slot's title (the engine rewrites it in place on every rename), else a
    /// compaction's `shortSummary`.
    pub title: Option<String>,
    /// Who wrote the title: `auto` or `user`. The engine's automatic renames never
    /// overwrite a user title, so a UI that renames has to know which it is looking at.
    pub title_source: Option<String>,
    /// The header's `parentSession`, kept opaque: upstream writes the parent *id* on
    /// `fork()` (`session-manager.ts:1912`) but a *path* on one branch (`:3199`), so a
    /// caller that needs the parent's id has to resolve both shapes (see
    /// [`SessionSummary::parent_id`]).
    pub parent: Option<String>,
    /// The header's `timestamp`, ISO-8601 (the engine's own form) — `None` when absent.
    pub created: Option<String>,
    /// The file's mtime, in milliseconds since the epoch. This is what the browser sorts
    /// by, so it is a number rather than a string.
    pub modified_ms: u64,
    pub size: u64,
    pub message_count: u64,
    /// The first user message's text, else the first assistant text, else
    /// `"(no messages)"` — the engine's own fallback chain, which is what makes a
    /// nameless session displayable.
    pub first_message: String,
    pub lifecycle: Lifecycle,
}

impl SessionSummary {
    /// The parent session's id, resolving both shapes upstream writes.
    ///
    /// A path-shaped `parentSession` yields the id in its filename; anything else is
    /// returned as written, because an id needs no interpretation.
    pub fn parent_id(&self) -> Option<String> {
        let parent = self.parent.as_deref()?;
        if parent.contains('/') {
            return crate::paths::session_id_from_path(Path::new(parent));
        }
        Some(parent.to_string())
    }

    /// The name to show: the title, else the first message, else nothing.
    ///
    /// The engine's `sanitizeSessionName` trims the first line and drops control
    /// characters, which is what keeps a title a single displayable line.
    pub fn display_name(&self) -> Option<String> {
        let title = self.title.as_deref().and_then(sanitize_name);
        if title.is_some() {
            return title;
        }
        let first = sanitize_name(&self.first_message)?;
        (first != "(no messages)").then_some(first)
    }
}

/// Trim to one displayable line, the way `sanitizeSessionName` does.
fn sanitize_name(value: &str) -> Option<String> {
    let first_line = value.split(['\r', '\n']).next().unwrap_or_default();
    let stripped: String = first_line
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let trimmed = stripped.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The two windows of a session file.
struct Windows {
    prefix: String,
    tail: String,
    size: u64,
    modified_ms: u64,
}

/// Read one session file's summary, or `None` when it cannot be read as one.
///
/// `None` is the answer for an unreadable, unparsable or header-less file. A listing that
/// failed as a whole because one file in one bucket was truncated would be worse than one
/// that skips it.
pub fn scan(path: &Path, bucket: &str) -> Option<SessionSummary> {
    let windows = read_windows(path)?;
    let entries = parse_lines(&windows.prefix);

    let header = entries
        .iter()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("session"))?;
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| crate::paths::session_id_from_path(path))?;

    // The slot is line 1 and holds the *current* title: `updateSessionTitle` rewrites it in
    // place, while the header keeps whatever the session was created with. The engine's
    // chain is slot → header → compaction summary, and a slot whose title is blank counts
    // as absent (`normalizeTitleOverride`), which is why this is a `filter`-and-`and_then`
    // rather than a plain `or`.
    let slot = entries
        .first()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("title"));
    let slot_title = slot
        .and_then(|entry| entry.get("title").and_then(Value::as_str))
        .and_then(sanitize_name);
    let header_title = header
        .get("title")
        .and_then(Value::as_str)
        .and_then(sanitize_name);

    let mut count = 0_u64;
    let mut first_message = String::new();
    let mut short_summary: Option<String> = None;
    for entry in &entries {
        match entry.get("type").and_then(Value::as_str) {
            Some("message") => {
                count += 1;
                if first_message.is_empty() {
                    if let Some(message) = entry.get("message") {
                        if message.get("role").and_then(Value::as_str) == Some("user") {
                            first_message = text_of(message.get("content"));
                        }
                    }
                }
            }
            Some("compaction") => {
                if let Some(summary) = entry.get("shortSummary").and_then(Value::as_str) {
                    short_summary = Some(summary.to_string());
                }
            }
            _ => {}
        }
    }

    // A 4 KiB prefix can cut a message in half, so the parsed count can under-report what
    // the file actually holds. The engine takes the larger of the parsed count and a
    // marker scan for exactly that reason.
    let message_count = count.max(count_message_markers(&windows.prefix));

    Some(SessionSummary {
        id,
        path: path.to_path_buf(),
        bucket: bucket.to_string(),
        cwd: header
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        title: slot_title.or(header_title).or(short_summary),
        title_source: slot
            .and_then(|entry| entry.get("source").and_then(Value::as_str))
            .or_else(|| header.get("titleSource").and_then(Value::as_str))
            .map(str::to_string),
        parent: header
            .get("parentSession")
            .and_then(Value::as_str)
            .map(str::to_string),
        created: header
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string),
        modified_ms: windows.modified_ms,
        size: windows.size,
        message_count,
        first_message: if first_message.is_empty() {
            // The prefix window can cut the *first* message in half — a pasted log as an
            // opening prompt is easily larger than 4 KiB — and a cut line does not parse.
            // The engine salvages the text out of the raw bytes here
            // (`extractFirstDisplayMessageFromPrefix`), which is what keeps such a session
            // displayable rather than nameless.
            let salvaged = salvage_first_text(&windows.prefix);
            if salvaged.is_empty() {
                "(no messages)".to_string()
            } else {
                salvaged
            }
        } else {
            first_message
        },
        lifecycle: derive_lifecycle(&windows.tail),
    })
}

/// List one bucket, newest first.
pub fn list_bucket(bucket: &Path) -> Vec<SessionSummary> {
    let name = bucket
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut sessions: Vec<SessionSummary> = crate::paths::session_files(bucket)
        .iter()
        .filter_map(|file| scan(file, &name))
        .collect();
    sort(&mut sessions);
    sessions
}

/// List every bucket, newest first.
///
/// The browser's list is one flat, newest-first sequence (it groups in the UI, where a
/// project's own order is already the right one), so this is a merge rather than a tree.
pub fn list_all(sessions_root: &Path) -> Vec<SessionSummary> {
    let mut sessions: Vec<SessionSummary> = crate::paths::buckets(sessions_root)
        .iter()
        .flat_map(|bucket| list_bucket(bucket))
        .collect();
    sort(&mut sessions);
    sessions
}

/// The engine's order: mtime descending, then creation descending, then path descending.
///
/// A session with no `created` compares equal at that step rather than sorting first or
/// last, which is what the engine's `new Date("")` arithmetic does (a `NaN` comparator is
/// treated as "equal" by the sort).
fn sort(sessions: &mut [SessionSummary]) {
    sessions.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| match (&b.created, &a.created) {
                (Some(left), Some(right)) => left.cmp(right),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| b.path.cmp(&a.path))
    });
}

/// Open a file and read its prefix, tail, size and mtime.
fn read_windows(path: &Path) -> Option<Windows> {
    let mut file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let size = metadata.len();
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default();

    let mut prefix = Vec::new();
    file.by_ref()
        .take(PREFIX_BYTES)
        .read_to_end(&mut prefix)
        .ok()?;

    // The tail is always read from `size - TAIL_BYTES` — for a file smaller than the window
    // that offset is zero, so the whole file is the tail. Reading the prefix here instead
    // would hand the status derivation the *first* message of a small session rather than
    // its last, which is a wrong badge on every short session (found by comparing this
    // against the engine's own listing: 10 of 29 sessions disagreed).
    let mut tail = Vec::new();
    file.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES)))
        .ok()?;
    file.read_to_end(&mut tail).ok()?;

    Some(Windows {
        prefix: String::from_utf8_lossy(&prefix).into_owned(),
        tail: String::from_utf8_lossy(&tail).into_owned(),
        size,
        modified_ms,
    })
}

/// Parse the object-valued lines of a JSONL window, skipping anything unparsable.
///
/// Lenient on purpose: a window boundary cuts a line in half, so the first (or last) line
/// is often a fragment. Decoding is lossy too — a multi-byte character split across the
/// boundary is not a reason to drop a session from the list.
fn parse_lines(content: &str) -> Vec<Value> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// Count `"type":"message"` markers, the engine's `countMessageMarkers` fallback.
fn count_message_markers(content: &str) -> u64 {
    let mut count = 0;
    let mut rest = content;
    // The engine's scan is whitespace-insensitive per property, so `"type" : "message"`
    // counts. Searching for the two tokens separately is what makes that true here.
    while let Some(index) = rest.find("\"type\"") {
        rest = &rest[index + 6..];
        let Some(colon) = rest.find(':') else { break };
        let after = rest[colon + 1..].trim_start();
        if let Some(value) = after.strip_prefix("\"message\"") {
            count += 1;
            rest = value;
        }
    }
    count
}

/// Classify a session from its tail window: the last `message` entry decides.
///
/// Walking backwards matters — everything after the last message is bookkeeping
/// (`title_change`, `session_exit`, custom markers), and none of it says whether the turn
/// finished.
fn derive_lifecycle(tail: &str) -> Lifecycle {
    for entry in parse_lines(tail).iter().rev() {
        if entry.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        return match message.get("role").and_then(Value::as_str) {
            Some("assistant") => match message.get("stopReason").and_then(Value::as_str) {
                Some("error") => Lifecycle::Error,
                Some("aborted") => Lifecycle::Aborted,
                Some("length") => Lifecycle::Interrupted,
                _ => {
                    // No stop reason worth a badge: the question is whether the turn left
                    // unanswered tool calls behind, which is what "interrupted" means.
                    let unanswered = message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block.get("type").and_then(Value::as_str) == Some("toolCall")
                            })
                        });
                    if unanswered {
                        Lifecycle::Interrupted
                    } else {
                        Lifecycle::Complete
                    }
                }
            },
            // Tools ran but the agent never produced the assistant turn that follows them.
            Some("toolResult") => Lifecycle::Interrupted,
            // A user message with no reply persisted after it.
            Some("user") => Lifecycle::Pending,
            _ => Lifecycle::Unknown,
        };
    }

    Lifecycle::Unknown
}

/// The text of a message's content, whether it is a string or blocks.
///
/// Blocks are joined with a space, as the engine's `textContent(content, " ")` does: a
/// multi-part message is one display line in the list.
fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

/// Pull a displayable first message out of a prefix that may end mid-line.
///
/// A port of the engine's `extractFirstDisplayMessageFromPrefix`: the first **user** text
/// wins, and a developer/assistant text is remembered as a fallback, so a session whose
/// user message was cut off is still named after something. The string extraction is
/// deliberately byte-level — the point is to read a *truncated* JSON line, which no parser
/// will accept.
fn salvage_first_text(prefix: &str) -> String {
    let mut fallback: Option<String> = None;
    let mut cursor = 0;

    while let Some(index) = prefix[cursor..].find("\"role\"") {
        let index = cursor + index;
        let role = extract_string_property(prefix, "role", index);
        let text = extract_string_property(prefix, "content", index)
            .or_else(|| extract_string_property(prefix, "text", index))
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        if let Some(text) = text {
            match role.as_deref() {
                Some("user") => return text,
                Some("developer") | Some("assistant") if fallback.is_none() => {
                    fallback = Some(text)
                }
                _ => {}
            }
        }

        cursor = index + "\"role\"".len();
    }

    fallback.unwrap_or_default()
}

/// The value of `"<name>": "…"` at or after `start`, decoded leniently.
///
/// A missing closing quote is treated as "the value runs to the end", which is what makes
/// this work on a window boundary; a truncated escape sequence is dropped rather than
/// discarded, mirroring the engine's `decodeJsonStringFragment`.
fn extract_string_property(source: &str, name: &str, start: usize) -> Option<String> {
    let from = start.min(source.len());
    let key = format!("\"{name}\"");
    let property = from + source[from..].find(&key)?;
    let colon = property + key.len() + source[property + key.len()..].find(':')?;

    let bytes = source.as_bytes();
    let mut index = colon + 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return None;
    }

    let value_start = index + 1;
    let mut escaped = false;
    let mut value_end = bytes.len();
    for (offset, byte) in bytes[value_start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'"' => {
                value_end = value_start + offset;
                break;
            }
            _ => {}
        }
    }

    Some(decode_json_fragment(&source[value_start..value_end]))
}

/// Decode the body of a JSON string, tolerating a fragment cut mid-escape.
fn decode_json_fragment(value: &str) -> String {
    let trimmed = value.strip_suffix('\\').unwrap_or(value);
    if let Ok(decoded) = serde_json::from_str::<String>(&format!("\"{trimmed}\"")) {
        return decoded;
    }
    trimmed
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}
