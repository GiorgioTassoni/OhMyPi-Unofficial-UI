//! The dialogs the engine is waiting on.
//!
//! A `select`, `confirm`, `input` or `editor` request stops the run until the host
//! answers. That makes this store the app's safety boundary as much as its state:
//! an unanswerable dialog is an agent hung forever, and an answer to a dialog the
//! engine has withdrawn is a late write that gets matched against whatever
//! replaced it. Both are refused here, once, rather than left to every caller to
//! remember (`docs/11` D3, `docs/12` §10).
//!
//! Only the blocking four are kept. `notify`, `setStatus`, `setWidget`,
//! `setTitle`, `set_editor_text` and `open_url` are fire-and-forget chrome; the
//! transport decodes them to nothing, so they never reach this module
//! (`docs/02` §7).

use omp_transport::protocol::ui::{Incoming, UiRequest, UiRequestKind, UiResponse};
use serde_json::Value;

/// Why an answer was refused.
///
/// Two variants, because they need different words in front of a user: "that
/// dialog is gone" versus "that answer does not fit this dialog".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerError {
    /// No dialog with that id is pending: it was answered, or the engine withdrew
    /// it, or its deadline passed. All three mean the same thing — do not write.
    NotPending(String),
    /// The answer does not fit the dialog — `confirmed` sent to a `select`, say.
    /// The engine would wait forever for the shape it actually asked for.
    Mismatch {
        kind: UiRequestKind,
        response: &'static str,
    },
}

impl std::fmt::Display for AnswerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPending(id) => write!(formatter, "no dialog `{id}` is pending"),
            Self::Mismatch { kind, response } => write!(
                formatter,
                "a `{response}` answer does not fit a `{}` dialog",
                kind.as_str()
            ),
        }
    }
}

/// The pending dialogs, in the order the engine asked for them.
///
/// A `Vec` rather than a map because the order is the engine's own presentation
/// order — it queues dialogs — and the set is never larger than a handful.
#[derive(Debug, Default)]
pub struct UiRequests {
    pending: Vec<UiRequest>,
}

impl UiRequests {
    /// Fold one decoded frame in, returning whether the visible set changed.
    ///
    /// The return value is the UI's cue to re-read: a `false` on a real request
    /// would mean a dialog the user never sees, and an agent that never continues.
    pub fn apply(&mut self, incoming: Incoming) -> bool {
        match incoming {
            Incoming::Request(request) => {
                // A repeated id replaces its entry. A resumed or reconnecting
                // session replays dialogs, and keeping the stale copy would
                // answer the request the engine is no longer waiting on.
                match self
                    .pending
                    .iter()
                    .position(|pending| pending.id == request.id)
                {
                    Some(index) => self.pending[index] = request,
                    None => self.pending.push(request),
                }
                true
            }
            Incoming::Withdrawn { target_id } => {
                let before = self.pending.len();
                self.pending.retain(|pending| pending.id != target_id);
                self.pending.len() != before
            }
        }
    }

    /// Answer a dialog, returning the frame to send.
    ///
    /// The dialog leaves the set only once its answer is known to be well-formed,
    /// so a rejected answer leaves the user able to choose again.
    pub fn answer(&mut self, id: &str, response: UiResponse) -> Result<Value, AnswerError> {
        let Some(index) = self.pending.iter().position(|pending| pending.id == id) else {
            return Err(AnswerError::NotPending(id.to_string()));
        };

        let kind = self.pending[index].kind;
        let Some(frame) = response.frame(kind, id) else {
            return Err(AnswerError::Mismatch {
                kind,
                response: response.name(),
            });
        };

        self.pending.remove(index);
        Ok(frame)
    }

    /// The dialogs to render, oldest first.
    pub fn pending(&self) -> &[UiRequest] {
        &self.pending
    }

    /// Drop every pending dialog, returning whether there were any.
    ///
    /// Called when the event stream ends: an approval dialog for a dead agent must
    /// not linger on screen, and its id can never be answered again.
    pub fn clear(&mut self) -> bool {
        let had_any = !self.pending.is_empty();
        self.pending.clear();
        had_any
    }
}
