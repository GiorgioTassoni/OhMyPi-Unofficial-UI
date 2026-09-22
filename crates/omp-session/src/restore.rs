//! Restoring a session's history over `get_messages_page`.
//!
//! This is the hardest read path in the app, because the engine pages a **live,
//! mutable** message list and can refuse twice, for two different reasons:
//!
//! | `code` | Meaning | Correct response |
//! | --- | --- | --- |
//! | `session_busy` | A turn is streaming or compacting | Wait, then retry the *same* cursor |
//! | `stale_cursor` | The session moved (session, branch leaf, or message count changed) | Discard the cursor and start over |
//!
//! Both are checked engine-side before any paging happens, both carry the request
//! `id` (unlike the unknown-command path, which cannot be correlated at all), and
//! both are reported as a structured `code` rather than prose, so no message-text
//! matching is involved.
//!
//! The engine also validates the cursor's embedded snapshot — `sessionId`,
//! `leafId`, `messageCount` — against the live session, which is why a new turn
//! mid-restore invalidates every outstanding cursor rather than shifting offsets.
//! That is what makes "start over" the only correct recovery.
//!
//! The state machine is generic over [`PageSource`] so its refusal handling is
//! tested deterministically (`tests/restore.rs`), not only against a live engine.

use std::future::Future;
use std::time::Duration;

use omp_transport::protocol::{self, commands};
use omp_transport::{ClientError, OmpClient};
use serde_json::Value;
use thiserror::Error;

use crate::messages::Message;

/// How hard to try before giving up on a history that will not sit still.
#[derive(Debug, Clone)]
pub struct RestorePolicy {
    /// Messages per request. The engine caps this at 256 and at 768 KiB of
    /// payload, so the default asks for the largest page that is always legal.
    pub page_limit: u32,
    /// How long to wait before retrying after `session_busy`.
    pub busy_backoff: Duration,
    /// Consecutive `session_busy` refusals tolerated for one page.
    pub max_busy_waits: u32,
    /// `stale_cursor` restarts tolerated for the whole restore.
    pub max_restarts: u32,
}

impl Default for RestorePolicy {
    fn default() -> Self {
        Self {
            page_limit: 256,
            busy_backoff: Duration::from_millis(250),
            // 250 ms × 480 ≈ two minutes of a running turn, which is longer than
            // a turn that would still be streaming while the user resumes.
            max_busy_waits: 480,
            max_restarts: 3,
        }
    }
}

/// Where pages come from.
///
/// Implemented for [`OmpClient`] for real use and by a scripted source in tests,
/// so the refusal handling can be exercised without an engine.
pub trait PageSource {
    fn page(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> impl Future<Output = Result<Value, ClientError>>;
}

impl PageSource for OmpClient {
    async fn page(&self, cursor: Option<&str>, limit: u32) -> Result<Value, ClientError> {
        self.request(commands::get_messages_page(cursor, Some(limit)), None)
            .await
    }
}

/// What a restore produced.
#[derive(Debug, Clone)]
pub struct RestoreOutcome {
    /// The session's messages, oldest first.
    pub messages: Vec<Message>,
    /// Requests that returned a page.
    pub pages: u32,
    /// Times the session was busy and we waited. Non-zero means a turn was
    /// running when the user opened this thread.
    pub busy_waits: u32,
    /// Times the history changed under us and paging restarted.
    pub restarts: u32,
}

/// Why a restore could not complete.
#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("history restore failed at the transport level: {0}")]
    Transport(#[from] ClientError),

    #[error("the session stayed busy for {busy_waits} consecutive attempts")]
    StillBusy { busy_waits: u32 },

    #[error("history kept changing while paging ({restarts} restarts); reopen the thread")]
    StillStale { restarts: u32 },

    #[error("the engine rejected the page request ({code:?}): {error}")]
    Rejected { code: Option<String>, error: String },

    #[error("a page declaring {reported} messages carried none")]
    MalformedPage { reported: u64 },

    #[error("the engine repeated cursor `{cursor}`, which paging could never advance")]
    RepeatedCursor { cursor: String },
}

/// Page the whole history, recovering from the engine's two documented refusals.
///
/// Finishes when a page reports no `nextCursor`, which is how the engine signals
/// the end — the last page is the only one without one.
pub async fn restore_history(
    source: &impl PageSource,
    policy: &RestorePolicy,
) -> Result<RestoreOutcome, RestoreError> {
    let mut messages: Vec<Message> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0u32;
    // Reported total, and the per-page budget. These differ: a healthy page
    // clears the budget but never the total, because "a turn was running when
    // you opened this thread" is the fact worth reporting.
    let mut busy_waits = 0u32;
    let mut consecutive_busy = 0u32;
    let mut restarts = 0u32;

    loop {
        let response = source.page(cursor.as_deref(), policy.page_limit).await?;

        if !protocol::is_success(&response) {
            match protocol::response_code(&response) {
                Some(protocol::CODE_SESSION_BUSY) => {
                    // A turn is running: the cursor is still valid, so wait and
                    // ask for the same page again.
                    busy_waits += 1;
                    consecutive_busy += 1;
                    if consecutive_busy > policy.max_busy_waits {
                        return Err(RestoreError::StillBusy { busy_waits });
                    }
                    tokio::time::sleep(policy.busy_backoff).await;
                    continue;
                }
                Some(protocol::CODE_STALE_CURSOR) => {
                    // The session moved. Every outstanding cursor is invalid, so
                    // discard the partial page set and start from the top.
                    restarts += 1;
                    if restarts > policy.max_restarts {
                        return Err(RestoreError::StillStale { restarts });
                    }
                    messages.clear();
                    cursor = None;
                    pages = 0;
                    consecutive_busy = 0;
                    continue;
                }
                code => {
                    return Err(RestoreError::Rejected {
                        code: code.map(ToString::to_string),
                        error: response
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
        }

        let data = response.get("data").cloned().unwrap_or(Value::Null);
        let Some(items) = data.get("messages").and_then(Value::as_array) else {
            return Err(RestoreError::MalformedPage {
                reported: data
                    .get("totalMessages")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
            });
        };

        messages.extend(items.iter().map(Message::decode));
        pages += 1;
        // Only *consecutive* refusals count against a page's budget.
        consecutive_busy = 0;

        match data.get("nextCursor").and_then(Value::as_str) {
            Some(next) => {
                // A cursor encodes the next offset, so a repeat means the engine
                // is not advancing. Guarding here keeps a paging bug from
                // becoming an unbounded append.
                if cursor.as_deref() == Some(next) {
                    return Err(RestoreError::RepeatedCursor {
                        cursor: next.to_string(),
                    });
                }
                cursor = Some(next.to_string());
            }
            None => break,
        }
    }

    Ok(RestoreOutcome {
        messages,
        pages,
        busy_waits,
        restarts,
    })
}
