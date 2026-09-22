//! The pending-dialog store, and the one rule about announcing it.
//!
//! Two things change the set: the pump, when a dialog arrives or is withdrawn, and
//! an answer from the UI. If only one of them announced the result, the other would
//! leave the window showing a dialog that is already gone — an approval the user
//! cannot dismiss, or one they can answer twice. So mutation and announcement are
//! the same call here, and neither caller can forget.
//!
//! (It was not academic: the first version published from the pump only, and the
//! approval test caught the answered dialog still being shown as pending.)

use std::sync::{Arc, Mutex};

use omp_session::{Transcript, UiRequests};
use omp_transport::protocol::ui::{Incoming, UiResponse};
use serde_json::Value;

use crate::dto::UiRequestSnapshot;
use crate::session::{ActivitySink, Reporting};

/// The pending set plus the channel it is announced on.
pub struct Dialogs {
    pending: Mutex<UiRequests>,
    /// Where announcements go: the thread they belong to, the sink they are written to,
    /// and the roster the sidebar reads.
    ///
    /// "Needs you" is one of the dots the sidebar draws (`docs/12` §2.2) and this store
    /// is the only thing that knows it changed, so the roster is republished beside the
    /// dialog set's own announcement — the same argument that made mutation and
    /// announcement one call here in the first place.
    reporting: Reporting,
    /// The conversation, for the one thing this store has to say there.
    ///
    /// A dialog that cannot be recorded is a dialog that cannot be answered — the engine
    /// waits on it until its own deadline and the tool fails with no reason the user can see.
    /// The transcript is where a failure is visible (`session::last_failure` reads it, and the
    /// sidebar's red dot is derived from it), so the store that lost the dialog is what has to
    /// write there rather than returning quietly (`docs/14` §7 #6).
    transcript: Arc<Mutex<Transcript>>,
}

impl Dialogs {
    pub fn new(reporting: Reporting, transcript: Arc<Mutex<Transcript>>) -> Self {
        Self {
            pending: Mutex::new(UiRequests::default()),
            reporting,
            transcript,
        }
    }

    /// Fold a decoded frame in.
    pub fn apply(&self, incoming: Incoming) {
        let changed = match self.pending.lock() {
            Ok(mut pending) => pending.apply(incoming),
            Err(_) => {
                // Poisoned means a panic happened inside this store, and the dialog in hand
                // cannot be kept — so it cannot be answered either. Saying so is the whole
                // point: the alternative is an agent blocked on a question nobody can see.
                self.report_panic();
                return;
            }
        };

        if changed {
            self.publish();
        }
    }

    /// Answer a dialog, returning the frame to send.
    ///
    /// A refused answer leaves the set untouched and publishes nothing: the dialog
    /// is still there, and the UI is already showing it.
    pub fn answer(&self, id: &str, response: UiResponse) -> Result<Value, String> {
        let frame = match self.pending.lock() {
            Ok(mut pending) => pending
                .answer(id, response)
                .map_err(|error| error.to_string())?,
            Err(_) => return Err("the pending-dialog store was poisoned".to_string()),
        };

        self.publish();
        Ok(frame)
    }

    /// The dialogs to render.
    pub fn snapshot(&self) -> Result<Vec<UiRequestSnapshot>, String> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| "the pending-dialog store was poisoned".to_string())?;

        Ok(snapshots(&pending))
    }

    /// How many dialogs are waiting, for the sidebar's row.
    ///
    /// Infallible where [`Self::snapshot`] is not: a poisoned store leaves the count at
    /// zero rather than hiding every thread's row.
    pub fn count(&self) -> usize {
        self.pending
            .lock()
            .map(|pending| pending.pending().len())
            .unwrap_or_default()
    }

    /// Drop every dialog, for when the engine is gone.
    ///
    /// Nothing in the set can be answered any more, so leaving it published would
    /// hold an approval on screen behind a dead agent.
    pub fn clear(&self) {
        let cleared = self
            .pending
            .lock()
            .map(|mut pending| pending.clear())
            .unwrap_or(false);

        if cleared {
            self.publish();
        }
    }

    fn publish(&self) {
        match self.snapshot() {
            Ok(pending) => {
                self.reporting
                    .sink
                    .ui_requests(&self.reporting.thread.current(), pending);
                // The dialog set is part of the sidebar's row, so the roster follows the
                // set on every change — a new approval has to show as "needs you" without
                // waiting for the next event tick.
                self.reporting
                    .sink
                    .threads(self.reporting.roster.snapshots());
            }
            Err(error) => eprintln!("[omp-desktop] {error}"),
        }
    }

    /// Say, where the user can see it, that a dialog was lost.
    ///
    /// The row's error comes out of the conversation (`docs/12` §2.2), so the notice goes
    /// into the transcript and the roster is republished beside it: the thread's row turns
    /// red over the approval that is stuck, which is the only visible trace an unanswerable
    /// dialog can leave.
    fn report_panic(&self) {
        if let Ok(mut transcript) = self.transcript.lock() {
            transcript.apply_error(
                "a dialog the agent is waiting on could not be recorded: the pending-dialog \
                 store was poisoned. The agent will wait until its own deadline.",
                "extension_ui_request",
            );
        }

        self.reporting
            .sink
            .threads(self.reporting.roster.snapshots());
    }
}

impl std::fmt::Debug for Dialogs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pending = self
            .pending
            .lock()
            .map(|pending| pending.pending().len())
            .unwrap_or_default();

        formatter
            .debug_struct("Dialogs")
            .field("pending", &pending)
            .finish()
    }
}

/// The dialogs, flattened for the frontend.
///
/// The one place the app's own state becomes the frontend's contract; see `dto.rs`
/// for why that conversion is written out rather than derived.
fn snapshots(pending: &UiRequests) -> Vec<UiRequestSnapshot> {
    pending
        .pending()
        .iter()
        .map(|request| UiRequestSnapshot {
            id: request.id.clone(),
            kind: request.kind.as_str().to_string(),
            title: request.title.clone(),
            message: request.message.clone(),
            options: request.options.clone(),
            prefill: request.prefill.clone(),
            placeholder: request.placeholder.clone(),
            timeout_ms: request.timeout_ms,
        })
        .collect()
}
