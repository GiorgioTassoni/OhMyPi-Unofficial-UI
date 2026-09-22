//! The agent roster: the subagents a session has running, and how they are doing.
//!
//! # Two sources, one list
//!
//! `get_subagents` answers with the engine's registry; the `subagent_lifecycle` and
//! `subagent_progress` frames carry changes. Neither is the roster on its own, and the
//! asymmetry is measured rather than assumed (`rpc-subagents.ts`):
//!
//! * the engine's registry holds **only running** agents — a terminal lifecycle
//!   deletes the row, so a polled list forgets an agent the moment it finishes, and
//!   a client that polls shows nothing about the work that just happened;
//! * the frames only describe agents that start *after* the subscription, so a
//!   session that already had subagents when the host connected is invisible to them
//!   (the engine's `off` default means that is every session, unless it asks).
//!
//! So this holds rows built from frames — a settled agent keeps its final status
//! instead of disappearing — and [`AgentRoster::reconcile`] folds a polled list in,
//! which is what keeps the first case honest when frames were missed. Rows are keyed
//! by the engine's own `id`, so nothing here invents an identity.
//!
//! # What the engine refuses to say
//!
//! Two refusals are the engine's own and are mirrored rather than worked around:
//!
//! * a `subagent_progress` payload for an id the engine has never seen *started* is
//!   dropped (`rpc-subagents.ts`, `handleProgress`), so a roster built from frames
//!   cannot contain an agent the engine itself would not list;
//! * a lifecycle payload whose owner disagrees with the row — a different
//!   `parentToolCallId`, or a different `sessionFile` — is ignored, because ids are
//!   recycled across sessions and a settle from one session must not rewrite another's
//!   row.
//!
//! And one thing the engine cannot say at all: a row it no longer lists may have
//! succeeded, failed, or been aborted, and no frame said which. [`Subagent::listed`]
//! is how that is carried — the panel reports "the engine stopped listing it" rather
//! than picking a status the engine never sent.

use std::path::Path;

use serde_json::Value;

use crate::messages::Message;

/// Which model produced this session's agent output, as the engine labels it.
///
/// Open rather than exhaustive: the union is the engine's, and a new value must not
/// make an agent unrenderable.
pub const AGENT_SOURCES: [&str; 3] = ["bundled", "user", "project"];

/// What a subagent is doing, as the engine's own union spells it.
///
/// `pending` is a real state the executor reports before a slot opens; `running` covers
/// everything in flight. The three terminal values are the ones a *frame* carries, which
/// is why they are named here rather than compared as strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Aborted,
}

impl AgentStatus {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    /// The lifecycle frame's four values, which are not the status union.
    ///
    /// `started` means "running" everywhere else (`rpc-subagents.ts`,
    /// `statusFromLifecycle`), and that translation is done here so no caller has to know
    /// two spellings for one state.
    pub fn from_lifecycle(raw: &str) -> Option<Self> {
        match raw {
            "started" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
}

/// An auto-retry the engine is sleeping through, when one is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRetry {
    pub attempt: u64,
    pub max_attempts: u64,
    pub delay_ms: u64,
    pub error_message: String,
    pub started_at_ms: u64,
}

/// The retry that ended a run, when the engine gave up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRetryFailure {
    pub attempt: u64,
    pub error_message: String,
}

/// The executor's own view of one run: identity aside, everything the roster renders.
///
/// A subset of the engine's `AgentProgress`, chosen field by field for what the agents
/// panel shows (`docs/12` §9). Numbers keep the engine's units — `cost` is USD,
/// `duration_ms` and `last_update` are milliseconds — so nothing here converts.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgress {
    pub status: Option<AgentStatus>,
    /// The engine's one-line summary, when the spawn carried one.
    pub description: Option<String>,
    /// The full task text the agent was given.
    pub task: Option<String>,
    /// The `assignment` a `task` item carried, when it had one.
    pub assignment: Option<String>,
    /// What the agent said it was about to do, in its own words.
    pub last_intent: Option<String>,
    /// The tool call in flight right now.
    pub current_tool: Option<String>,
    pub current_tool_args: Option<String>,
    pub current_tool_start_ms: Option<u64>,
    pub tool_count: u64,
    /// Assistant requests across the run — the engine's soft-budget counter.
    pub requests: u64,
    /// Lifetime tokens across all turns, excluding cache re-reads.
    pub tokens: u64,
    /// The latest turn's context size, which is the number a reader compares to the window.
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Cumulative billing cost in USD.
    pub cost: f64,
    pub duration_ms: u64,
    /// `<provider>/<id>`, with a `:<level>` suffix when the level was set explicitly.
    pub resolved_model: Option<String>,
    pub resolved_model_identity: Option<String>,
    pub resolved_thinking_level: Option<String>,
    /// Whether a live advisor was attached to this run.
    pub advisor: bool,
    pub retry: Option<AgentRetry>,
    pub retry_failure: Option<AgentRetryFailure>,
}

impl AgentProgress {
    pub fn decode(raw: &Value) -> Option<Self> {
        let object = raw.as_object()?;

        Some(Self {
            status: string(raw, "status")
                .as_deref()
                .and_then(AgentStatus::parse),
            description: string(raw, "description"),
            task: string(raw, "task"),
            assignment: string(raw, "assignment"),
            last_intent: string(raw, "lastIntent"),
            current_tool: string(raw, "currentTool"),
            current_tool_args: string(raw, "currentToolArgs"),
            current_tool_start_ms: object.get("currentToolStartMs").and_then(Value::as_u64),
            tool_count: number(raw, "toolCount"),
            requests: number(raw, "requests"),
            tokens: number(raw, "tokens"),
            context_tokens: object.get("contextTokens").and_then(Value::as_u64),
            context_window: object.get("contextWindow").and_then(Value::as_u64),
            cost: object
                .get("cost")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            duration_ms: number(raw, "durationMs"),
            resolved_model: string(raw, "resolvedModel"),
            resolved_model_identity: string(raw, "resolvedModelIdentity"),
            resolved_thinking_level: string(raw, "resolvedThinkingLevel"),
            advisor: flag(raw, "advisor"),
            retry: raw.get("retryState").and_then(decode_retry),
            retry_failure: raw.get("retryFailure").and_then(decode_retry_failure),
        })
    }
}

fn decode_retry(raw: &Value) -> Option<AgentRetry> {
    raw.as_object()?;

    Some(AgentRetry {
        attempt: number(raw, "attempt"),
        max_attempts: number(raw, "maxAttempts"),
        delay_ms: number(raw, "delayMs"),
        error_message: string(raw, "errorMessage").unwrap_or_default(),
        started_at_ms: number(raw, "startedAtMs"),
    })
}

fn decode_retry_failure(raw: &Value) -> Option<AgentRetryFailure> {
    raw.as_object()?;

    Some(AgentRetryFailure {
        attempt: number(raw, "attempt"),
        error_message: string(raw, "errorMessage").unwrap_or_default(),
    })
}

/// One subagent, as the panel lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Subagent {
    /// The engine's id for this spawn. The key for everything else.
    pub id: String,
    /// Dispatch order within a `task` call: several items run as one batch, and this is
    /// the order they were asked for.
    pub index: u64,
    /// The agent's name (`scout`, `reviewer`, …), which is what a user configured.
    pub agent: String,
    pub agent_source: Option<String>,
    pub description: Option<String>,
    pub status: Option<AgentStatus>,
    /// The task text, from the spawn payload (`get_subagents` carries it; a lifecycle
    /// frame does not).
    pub task: Option<String>,
    pub assignment: Option<String>,
    /// The subagent's own session file, which is also how its transcript is addressed.
    pub session_file: Option<String>,
    /// The `toolCallId` of the `task` call that spawned it — the link back into the
    /// parent's transcript.
    pub parent_tool_call_id: Option<String>,
    /// Whether this spawn runs detached: the parent turn keeps working while it does.
    ///
    /// Unset for a synchronous spawn (the parent is blocked on the call) and for an
    /// `agent()` bridge spawn, which renders inside its own eval cell.
    pub detached: bool,
    /// When **this host** last heard about this row, in epoch milliseconds.
    ///
    /// Deliberately our clock and not the engine's `lastUpdate`: "how long has this been
    /// running" compares against the window's own now, and mixing two clocks in one
    /// subtraction is how a row reads as hours old or freshly started at random. Only a
    /// frame or a first sighting in a list moves it — a poll that learned nothing is not
    /// news, which is also what keeps such a poll from publishing.
    pub last_update_ms: u64,
    /// Whether the engine's most recent `get_subagents` answer contained this row.
    ///
    /// `false` on an active status means the engine stopped listing it without saying
    /// how it ended — a missed frame, or a settle the subscription was too late for.
    pub listed: bool,
    pub progress: Option<AgentProgress>,
}

impl Subagent {
    /// Decode one row of a `get_subagents` answer.
    ///
    /// Requires an `id`: a row without one cannot be merged with the frames that
    /// describe the same agent, so it would be a duplicate rather than a row.
    pub fn decode_snapshot(raw: &Value) -> Option<Self> {
        let id = string(raw, "id")?;
        let progress = raw.get("progress").and_then(AgentProgress::decode);

        Some(Self {
            id,
            index: number(raw, "index"),
            agent: string(raw, "agent").unwrap_or_default(),
            agent_source: string(raw, "agentSource"),
            // The snapshot and the progress payload describe the same run; the top level
            // wins only when it has something, which is how the engine folds them too.
            description: string(raw, "description")
                .or_else(|| progress.as_ref().and_then(|p| p.description.clone())),
            status: string(raw, "status")
                .as_deref()
                .and_then(AgentStatus::parse)
                .or_else(|| progress.as_ref().and_then(|p| p.status)),
            task: string(raw, "task"),
            assignment: string(raw, "assignment"),
            session_file: string(raw, "sessionFile"),
            parent_tool_call_id: string(raw, "parentToolCallId"),
            detached: flag(raw, "detached"),
            last_update_ms: number(raw, "lastUpdate"),
            listed: true,
            progress,
        })
    }

    /// Fold a lifecycle frame's payload into this row.
    fn apply_lifecycle(&mut self, payload: &Value, at_ms: u64) {
        if let Some(status) = string(payload, "status")
            .as_deref()
            .and_then(AgentStatus::from_lifecycle)
        {
            self.status = Some(status);
        }
        self.agent = string(payload, "agent").unwrap_or(std::mem::take(&mut self.agent));
        self.index = number_or(payload, "index", self.index);
        self.agent_source = string(payload, "agentSource").or(self.agent_source.take());
        self.description = string(payload, "description").or(self.description.take());
        self.session_file = string(payload, "sessionFile").or(self.session_file.take());
        self.parent_tool_call_id =
            string(payload, "parentToolCallId").or(self.parent_tool_call_id.take());
        if let Some(detached) = payload.get("detached").and_then(Value::as_bool) {
            self.detached = detached;
        }
        self.last_update_ms = at_ms;
    }

    /// Fold a progress frame's payload into this row.
    fn apply_progress(&mut self, payload: &Value, at_ms: u64) {
        let Some(progress) = payload.get("progress").and_then(AgentProgress::decode) else {
            return;
        };

        self.index = number_or(payload, "index", self.index);
        self.agent = string(payload, "agent").unwrap_or(std::mem::take(&mut self.agent));
        self.agent_source = string(payload, "agentSource").or(self.agent_source.take());
        self.task = string(payload, "task").or(self.task.take());
        self.assignment = string(payload, "assignment").or(self.assignment.take());
        self.session_file = string(payload, "sessionFile").or(self.session_file.take());
        self.parent_tool_call_id =
            string(payload, "parentToolCallId").or(self.parent_tool_call_id.take());
        if let Some(detached) = payload.get("detached").and_then(Value::as_bool) {
            self.detached = detached;
        }
        self.description = progress.description.clone().or(self.description.take());
        if let Some(status) = progress.status {
            self.status = Some(status);
        }
        self.last_update_ms = at_ms;
        self.progress = Some(progress);
    }

    /// Whether the frame's owner is this row's owner.
    ///
    /// The engine's own rule (`hasSameOwner`), and it exists because ids are recycled:
    /// comparing either field only when both sides have it means a frame from another
    /// session cannot settle this row, while a frame that simply omits the field is still
    /// accepted.
    fn same_owner(&self, payload: &Value) -> bool {
        let parent = string(payload, "parentToolCallId");
        if let (Some(frame), Some(row)) = (parent.as_deref(), self.parent_tool_call_id.as_deref()) {
            return frame == row;
        }

        let session = string(payload, "sessionFile");
        if let (Some(frame), Some(row)) = (session.as_deref(), self.session_file.as_deref()) {
            return frame == row;
        }

        true
    }

    /// Whether a lifecycle payload's owner is this row's owner.
    fn same_owner_lifecycle(&self, payload: &Value) -> bool {
        self.same_owner(payload)
    }
}

/// The session's subagents, newest state last.
#[derive(Debug, Clone, Default)]
pub struct AgentRoster {
    agents: Vec<Subagent>,
}

impl AgentRoster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn agents(&self) -> &[Subagent] {
        &self.agents
    }

    /// How many are in flight, which is the number the sidebar's row shows.
    pub fn active(&self) -> usize {
        self.agents
            .iter()
            .filter(|agent| agent.status.is_some_and(AgentStatus::is_active))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Drop every row, for a session whose engine has been replaced.
    ///
    /// A restart's roster says nothing about the roster before it: `get_subagents` on a
    /// resumed session answers with an empty registry (the engine's `clear()`), so keeping
    /// the old rows would show agents nothing is running any more.
    pub fn clear(&mut self) {
        self.agents.clear();
    }

    /// Apply one `subagent_lifecycle` / `subagent_progress` / `subagent_event` frame.
    ///
    /// Returns whether the roster changed, so a caller can publish on real change rather
    /// than on every frame. `at_ms` is when the frame was seen: the engine stamps
    /// `lastUpdate` with *its* clock, and a host that used that value for "how long has
    /// this been running" would be comparing two clocks.
    pub fn apply_frame(&mut self, frame: &Value, at_ms: u64) -> bool {
        let kind = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(payload) = frame.get("payload") else {
            return false;
        };

        match kind {
            // Full subagent event streams are forwarded only at the `events` level, which
            // this client does not ask for. Recognised so that raising the level is a
            // one-line change rather than a silent gap.
            "subagent_event" => false,
            "subagent_lifecycle" => self.apply_lifecycle(payload, at_ms),
            "subagent_progress" => self.apply_progress(payload, at_ms),
            _ => false,
        }
    }

    fn apply_lifecycle(&mut self, payload: &Value, at_ms: u64) -> bool {
        let Some(id) = string(payload, "id") else {
            return false;
        };
        let Some(status) = string(payload, "status")
            .as_deref()
            .and_then(AgentStatus::from_lifecycle)
        else {
            return false;
        };

        if let Some(row) = self.agents.iter_mut().find(|row| row.id == id) {
            if !row.same_owner_lifecycle(payload) {
                return false;
            }
            row.apply_lifecycle(payload, at_ms);
            row.listed = true;

            return true;
        }

        // A settle for an agent this roster never saw start. The engine drops these too
        // (`handleLifecycle`: `!existing && payload.status !== "started"`), and inventing a
        // row from a terminal frame would produce an agent with no task, no index and no
        // transcript — a row that says less than its absence does.
        if status != AgentStatus::Running {
            return false;
        }

        let mut row = Subagent {
            id,
            index: number(payload, "index"),
            agent: String::new(),
            agent_source: None,
            description: None,
            status: Some(status),
            task: None,
            assignment: None,
            session_file: None,
            parent_tool_call_id: None,
            detached: false,
            last_update_ms: at_ms,
            listed: true,
            progress: None,
        };
        row.apply_lifecycle(payload, at_ms);
        self.agents.push(row);
        self.sort();

        true
    }

    fn apply_progress(&mut self, payload: &Value, at_ms: u64) -> bool {
        let Some(progress) = payload.get("progress") else {
            return false;
        };
        let Some(id) = string(progress, "id") else {
            return false;
        };

        let Some(row) = self.agents.iter_mut().find(|row| row.id == id) else {
            // Mirrors the engine: progress for an id it never saw start is dropped.
            return false;
        };
        if !row.same_owner(payload) {
            return false;
        }

        row.apply_progress(payload, at_ms);
        row.listed = true;

        true
    }

    /// Fold a `get_subagents` answer in.
    ///
    /// The engine's list is authoritative for *which* agents are live, and richer than a
    /// frame for the rows it holds (it carries `task`, which no frame does). So a row in
    /// both places is refreshed from the list, and a row the list omits while still
    /// claiming to run is marked [`Subagent::listed`] false rather than deleted or given a
    /// status nothing reported.
    pub fn reconcile(&mut self, live: &[Subagent], at_ms: u64) -> bool {
        let mut changed = false;

        for snapshot in live {
            match self.agents.iter_mut().find(|row| row.id == snapshot.id) {
                Some(row) => {
                    // The list is authoritative for identity and status; the row's own
                    // progress stays when it is the fresher view of the run (its
                    // `durationMs` grows monotonically, so the larger one is later).
                    let mut merged = snapshot.clone();
                    merged.last_update_ms = row.last_update_ms;
                    merged.progress = match (&row.progress, &snapshot.progress) {
                        (Some(ours), Some(theirs)) => {
                            Some(if ours.duration_ms > theirs.duration_ms {
                                ours.clone()
                            } else {
                                theirs.clone()
                            })
                        }
                        (Some(ours), None) => Some(ours.clone()),
                        (None, theirs) => theirs.clone(),
                    };
                    if *row != merged {
                        *row = merged;
                        changed = true;
                    }
                }
                None => {
                    let mut snapshot = snapshot.clone();
                    snapshot.last_update_ms = at_ms;
                    self.agents.push(snapshot);
                    changed = true;
                }
            }
        }

        let live_ids: Vec<&str> = live.iter().map(|snapshot| snapshot.id.as_str()).collect();
        for row in self.agents.iter_mut() {
            let listed = live_ids.contains(&row.id.as_str());
            if row.listed != listed {
                row.listed = listed;
                changed = true;
            }
        }

        if changed {
            self.sort();
        }

        changed
    }

    /// The engine's own order: dispatch index, then id (`getSubagents`).
    fn sort(&mut self) {
        self.agents.sort_by(|left, right| {
            left.index
                .cmp(&right.index)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
}

/// One page of a subagent transcript, as a byte cursor reports it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptPage {
    /// Where this page started.
    pub from_byte: u64,
    /// Where the next one starts. Pass it back as `from_byte` for the delta.
    pub next_byte: u64,
    /// The file was shorter than the cursor, so this page restarts at zero.
    ///
    /// The engine's own flag (`readRpcSubagentTranscript`), and the reason the caller has
    /// to *replace* rather than append: a reset page's rows are the file's beginning, not a
    /// continuation of what the window already showed.
    pub reset: bool,
    pub messages: Vec<Message>,
}

/// A page of a subagent's transcript, read from disk.
///
/// The engine answers this over RPC while it still owns the id
/// (`get_subagent_messages`), and **stops** doing so the moment the registry is cleared —
/// which a `new_session`, a `switch_session` or a fork all do. The transcript is a plain
/// session JSONL beside the parent's artifacts, written by the subagent itself, so the host
/// can read what the engine will no longer serve. `docs/12` §9's parked rows are that case.
///
/// The rules are the engine's, deliberately, because a client that paged differently would
/// disagree with the engine about the same file:
///
/// * byte offsets, not lines or characters;
/// * a page ends at the last newline it read, so a half-written line is re-read next time
///   rather than parsed as corruption;
/// * a cursor past the end of the file means the file *shrank*, and that page comes back
///   with [`TranscriptPage::reset`] set and starts at zero again;
/// * a file that is not there is an empty page, not an error: a subagent whose transcript
///   was removed is a subagent with nothing to show, which is what an empty page says.
pub fn read_transcript(path: &Path, from_byte: u64) -> std::io::Result<TranscriptPage> {
    let mut start = from_byte;

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TranscriptPage {
                from_byte: start,
                next_byte: start,
                reset: false,
                messages: Vec::new(),
            })
        }
        Err(error) => return Err(error),
    };

    let mut reset = false;
    if start as usize > bytes.len() {
        start = 0;
        reset = true;
    }

    let slice = bytes.get(start as usize..).unwrap_or_default();
    let complete = match slice.iter().rposition(|byte| *byte == b'\n') {
        Some(last) => &slice[..=last],
        // Nothing but a partial line: the cursor does not move, so the line is re-read once
        // it is whole.
        None => &slice[..0],
    };

    let mut messages = Vec::new();
    for line in complete.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            // The engine skips an unparsable line rather than failing the page
            // (`parseSessionEntries`), and a transcript that refuses to render because one
            // bookkeeping line was truncated is worse than one missing that line.
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        if let Some(message) = entry.get("message") {
            messages.push(Message::decode(message));
        }
    }

    Ok(TranscriptPage {
        from_byte: start,
        next_byte: start + complete.len() as u64,
        reset,
        messages,
    })
}

fn string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn number(raw: &Value, key: &str) -> u64 {
    raw.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn number_or(raw: &Value, key: &str, fallback: u64) -> u64 {
    raw.get(key).and_then(Value::as_u64).unwrap_or(fallback)
}

fn flag(raw: &Value, key: &str) -> bool {
    raw.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One `subagent_lifecycle` frame, shaped as the engine emits it
    /// (`SubagentLifecyclePayload`).
    fn lifecycle(id: &str, status: &str, session_file: Option<&str>) -> Value {
        json!({
            "type": "subagent_lifecycle",
            "payload": {
                "id": id,
                "agent": "scout",
                "agentSource": "bundled",
                "description": "Measure the wire",
                "status": status,
                "sessionFile": session_file,
                "parentToolCallId": "call-1",
                "index": 0,
                "detached": true,
            },
        })
    }

    /// One `subagent_progress` frame, shaped as the engine emits it
    /// (`SubagentProgressPayload` + `AgentProgress`).
    fn progress(id: &str, status: &str, tool_count: u64) -> Value {
        json!({
            "type": "subagent_progress",
            "payload": {
                "index": 0,
                "agent": "scout",
                "agentSource": "bundled",
                "task": "Measure the wire",
                "parentToolCallId": "call-1",
                "progress": {
                    "index": 0,
                    "id": id,
                    "agent": "scout",
                    "agentSource": "bundled",
                    "status": status,
                    "task": "Measure the wire",
                    "description": "Measure the wire",
                    "lastIntent": "reading the source",
                    "currentTool": "read",
                    "currentToolArgs": "{\"path\":\"src/rpc.ts\"}",
                    "currentToolStartMs": 1000,
                    "recentTools": [],
                    "recentOutput": [],
                    "toolCount": tool_count,
                    "requests": 3,
                    "tokens": 4200,
                    "contextTokens": 1800,
                    "contextWindow": 200_000,
                    "cost": 0.0125,
                    "durationMs": 900,
                    "resolvedModel": "anthropic/sonnet:hi",
                    "resolvedModelIdentity": "anthropic/sonnet",
                    "resolvedThinkingLevel": "hi",
                },
                "sessionFile": "/tmp/sess/Subagent.jsonl",
            },
        })
    }

    /// A `get_subagents` row (`RpcSubagentSnapshot`).
    fn snapshot(id: &str, status: &str) -> Value {
        json!({
            "id": id,
            "index": 0,
            "agent": "scout",
            "agentSource": "bundled",
            "description": "Measure the wire",
            "status": status,
            "task": "Measure the wire",
            "sessionFile": "/tmp/sess/Subagent.jsonl",
            "lastUpdate": 1_700_000_000_000u64,
            "parentToolCallId": "call-1",
            "progress": {
                "index": 0,
                "id": id,
                "agent": "scout",
                "agentSource": "bundled",
                "status": status,
                "task": "Measure the wire",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 1,
                "requests": 1,
                "tokens": 10,
                "cost": 0.0,
                "durationMs": 10,
            },
        })
    }

    #[test]
    fn a_started_lifecycle_becomes_a_running_row() {
        let mut roster = AgentRoster::new();

        assert!(roster.apply_frame(&lifecycle("a1", "started", Some("/tmp/a1.jsonl")), 5));
        assert_eq!(roster.agents().len(), 1);

        let row = &roster.agents()[0];
        assert_eq!(row.id, "a1");
        assert_eq!(row.status, Some(AgentStatus::Running));
        assert_eq!(row.agent, "scout");
        assert_eq!(row.session_file.as_deref(), Some("/tmp/a1.jsonl"));
        assert_eq!(row.parent_tool_call_id.as_deref(), Some("call-1"));
        assert!(row.detached);
        assert_eq!(roster.active(), 1);
    }

    /// The whole reason this type exists rather than a poll: the engine deletes a settled
    /// agent from its registry, so the row has to survive its own completion.
    #[test]
    fn a_settled_agent_keeps_its_row_and_its_final_status() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", Some("/tmp/a1.jsonl")), 5);
        roster.apply_frame(&progress("a1", "running", 4), 6);

        assert!(roster.apply_frame(&lifecycle("a1", "completed", None), 7));

        let row = &roster.agents()[0];
        assert_eq!(row.status, Some(AgentStatus::Completed));
        assert_eq!(roster.active(), 0);
        // Whatever the last progress said is still readable: a settled agent's cost and
        // token count are part of what the panel reports about it.
        assert_eq!(
            row.progress.as_ref().map(|progress| progress.tool_count),
            Some(4)
        );
    }

    #[test]
    fn progress_for_an_unknown_agent_is_dropped() {
        let mut roster = AgentRoster::new();

        assert!(!roster.apply_frame(&progress("ghost", "running", 1), 5));
        assert!(roster.is_empty());
    }

    #[test]
    fn a_settle_for_an_agent_never_seen_started_is_ignored() {
        let mut roster = AgentRoster::new();

        assert!(!roster.apply_frame(&lifecycle("a1", "completed", Some("/tmp/a1.jsonl")), 5));
        assert!(roster.is_empty());
    }

    /// Ids are recycled across sessions, so the owner decides which row a settle lands on.
    #[test]
    fn a_frame_from_another_owner_does_not_touch_the_row() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", Some("/tmp/a1.jsonl")), 5);

        let mut other = lifecycle("a1", "failed", None);
        other["payload"]["parentToolCallId"] = json!("call-2");
        assert!(!roster.apply_frame(&other, 6));

        assert_eq!(
            roster.agents()[0].status,
            Some(AgentStatus::Running),
            "a settle from another session's call must not rewrite this row"
        );
    }

    #[test]
    fn a_progress_frame_carries_what_the_panel_shows() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", Some("/tmp/a1.jsonl")), 5);
        roster.apply_frame(&progress("a1", "running", 7), 42);

        let row = &roster.agents()[0];
        assert_eq!(row.task.as_deref(), Some("Measure the wire"));
        assert_eq!(row.last_update_ms, 42);

        let progress = row.progress.as_ref().expect("the frame carried progress");
        assert_eq!(progress.tool_count, 7);
        assert_eq!(progress.requests, 3);
        assert_eq!(progress.tokens, 4200);
        assert_eq!(progress.context_tokens, Some(1800));
        assert_eq!(progress.context_window, Some(200_000));
        assert_eq!(progress.cost, 0.0125);
        assert_eq!(progress.duration_ms, 900);
        assert_eq!(progress.last_intent.as_deref(), Some("reading the source"));
        assert_eq!(progress.current_tool.as_deref(), Some("read"));
        assert_eq!(
            progress.resolved_model.as_deref(),
            Some("anthropic/sonnet:hi")
        );
        assert_eq!(progress.resolved_thinking_level.as_deref(), Some("hi"));
    }

    #[test]
    fn a_rate_limited_agent_reports_its_retry() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", None), 5);

        let mut frame = progress("a1", "running", 2);
        frame["payload"]["progress"]["retryState"] = json!({
            "attempt": 2,
            "maxAttempts": 5,
            "delayMs": 8000,
            "errorMessage": "429 rate limited",
            "startedAtMs": 1_700_000_000_000u64,
        });
        roster.apply_frame(&frame, 6);

        let retry = roster.agents()[0]
            .progress
            .as_ref()
            .and_then(|progress| progress.retry.clone())
            .expect("the retry is what explains a stalled agent");
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.max_attempts, 5);
        assert_eq!(retry.error_message, "429 rate limited");
    }

    #[test]
    fn reconcile_adds_the_agents_the_frames_missed_and_marks_the_gone_ones() {
        let mut roster = AgentRoster::new();
        // Subscribed late: this agent started before the app connected, so only the list
        // knows about it.
        roster.apply_frame(&lifecycle("a1", "started", Some("/tmp/a1.jsonl")), 5);

        let engine = vec![
            Subagent::decode_snapshot(&snapshot("a0", "running")).expect("a row with an id"),
            Subagent::decode_snapshot(&snapshot("a1", "running")).expect("a row with an id"),
        ];

        assert!(roster.reconcile(&engine, 100));
        assert_eq!(roster.agents().len(), 2);

        // The list carries `task`, which no frame does.
        let a0 = &roster.agents()[0];
        assert_eq!(a0.id, "a0");
        assert_eq!(a0.task.as_deref(), Some("Measure the wire"));
        assert!(a0.listed);

        // Now the engine stops listing `a0` while we last saw it running, and no frame
        // said how it ended. The row stays and says exactly that.
        let engine = vec![Subagent::decode_snapshot(&snapshot("a1", "running")).expect("a row")];
        assert!(roster.reconcile(&engine, 200));

        let a0 = roster
            .agents()
            .iter()
            .find(|row| row.id == "a0")
            .expect("kept");
        assert!(!a0.listed);
        assert_eq!(
            a0.status,
            Some(AgentStatus::Running),
            "no frame reported an end, so the status must not be invented"
        );
    }

    #[test]
    fn reconcile_with_an_unchanged_list_reports_no_change() {
        let mut roster = AgentRoster::new();
        let engine = vec![Subagent::decode_snapshot(&snapshot("a0", "running")).expect("a row")];

        assert!(roster.reconcile(&engine, 100));
        assert!(
            !roster.reconcile(&engine, 150),
            "a poll that learned nothing must not publish"
        );
    }

    #[test]
    fn rows_keep_the_engines_dispatch_order() {
        let mut roster = AgentRoster::new();
        for (id, index) in [("b", 2), ("a", 1), ("c", 1)] {
            let mut frame = lifecycle(id, "started", None);
            frame["payload"]["index"] = json!(index);
            roster.apply_frame(&frame, 5);
        }

        let ids: Vec<&str> = roster.agents().iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c", "b"], "index first, then id");
    }

    #[test]
    fn a_restart_clears_a_roster_that_no_longer_describes_anything() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", None), 5);

        roster.clear();

        assert!(roster.is_empty());
    }

    #[test]
    fn a_frame_this_build_does_not_know_is_not_a_change() {
        let mut roster = AgentRoster::new();
        roster.apply_frame(&lifecycle("a1", "started", None), 5);

        assert!(!roster.apply_frame(&json!({ "type": "subagent_event", "payload": {} }), 6));
        assert!(!roster.apply_frame(&json!({ "type": "subagent_lifecycle" }), 6));
        assert!(!roster.apply_frame(
            &json!({ "type": "subagent_lifecycle", "payload": { "id": "x" } }),
            6
        ));
        assert!(!roster.apply_frame(
            &json!({ "type": "subagent_lifecycle", "payload": { "id": "x", "status": "weird" } }),
            6
        ));
    }

    /// One `message` entry, as a session JSONL holds it.
    fn message_entry(text: &str) -> String {
        format!(
            "{{\"type\":\"message\",\"id\":\"{}\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}}]}}}}\n",
            text
        )
    }

    fn scratch(name: &str, contents: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("omp-agents-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, contents).expect("a scratch transcript");

        path
    }

    /// The engine's page rule, asserted on a file rather than through a live engine: the
    /// cursor advances by *bytes* and stops at the last complete line.
    #[test]
    fn a_transcript_page_stops_at_the_last_complete_line() {
        let path = scratch("page", "");
        // The third entry is written in two halves, which is what a subagent mid-write
        // leaves behind: `write` is not atomic, and a page read during it must neither
        // parse the fragment nor move the cursor past it.
        let third = message_entry("three");
        let (head, tail) = third.split_at(20);
        let partial = format!("{}{}{}", message_entry("one"), message_entry("two"), head);
        std::fs::write(&path, &partial).expect("a scratch transcript");

        let page = read_transcript(&path, 0).expect("a page");
        assert_eq!(page.from_byte, 0);
        assert!(!page.reset);
        assert_eq!(page.messages.len(), 2);
        assert_eq!(
            page.next_byte,
            (message_entry("one").len() + message_entry("two").len()) as u64,
            "the cursor stops before the partial line"
        );

        // Nothing new yet, and once the line is finished the delta is exactly that entry.
        assert!(read_transcript(&path, page.next_byte)
            .expect("a page")
            .messages
            .is_empty());
        std::fs::write(&path, format!("{partial}{tail}")).expect("the line completes");
        let page = read_transcript(&path, page.next_byte).expect("a page");
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].text(), "three");

        std::fs::remove_file(&path).ok();
    }

    /// A cursor past the end means the file shrank, and the engine says so rather than
    /// pretending the page is empty.
    #[test]
    fn a_cursor_past_the_end_resets_the_page() {
        let path = scratch("shrink", &message_entry("one"));

        let page = read_transcript(&path, 10_000).expect("a page");

        assert!(page.reset);
        assert_eq!(page.from_byte, 0);
        assert_eq!(page.messages.len(), 1);

        std::fs::remove_file(&path).ok();
    }

    /// A missing transcript is an empty page: a parked row whose file was cleaned up has
    /// nothing to show, which is not an error the panel could act on.
    #[test]
    fn a_missing_transcript_is_an_empty_page() {
        let path = std::env::temp_dir().join("omp-agents-does-not-exist.jsonl");

        let page = read_transcript(&path, 42).expect("an empty page");

        assert_eq!(page.from_byte, 42);
        assert_eq!(page.next_byte, 42);
        assert!(!page.reset);
        assert!(page.messages.is_empty());
    }

    /// The engine's own status vocabulary is what the panel keys its dots off, so a value
    /// that is not in it must not become a status this app invented.
    #[test]
    fn an_unknown_status_is_not_a_status() {
        assert_eq!(AgentStatus::parse("idle"), None);
        assert_eq!(AgentStatus::from_lifecycle("parked"), None);
        assert_eq!(
            AgentStatus::from_lifecycle("started"),
            Some(AgentStatus::Running)
        );
    }
}
