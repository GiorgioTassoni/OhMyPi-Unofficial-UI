//! The RPC client: spawn a sidecar, speak the protocol, correlate responses.
//!
//! Responsibilities, and deliberately nothing more:
//!
//! * own the process (via [`Sidecar`]) and its stdout reader;
//! * decode frames with [`FrameDecoder`], including lossless v2 reassembly;
//! * resolve `request()` calls by `id`, tolerating the documented cases where
//!   the engine answers **without** an `id`;
//! * broadcast everything else (session events, extension-UI requests,
//!   host-tool/URI round trips, subagent frames) to subscribers.
//!
//! It does not interpret events, hold session state, or know about widgets —
//! that is the app's job (`docs/11-v1-scope.md` §3.2).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

use crate::error::ClientError;
use crate::events::SessionEvent;
use crate::frame::FrameDecoder;
use crate::protocol::{
    classify, response_command, response_id, FrameClass, ReadyFrame, PROTOCOL_VERSION_V2,
};
use crate::sidecar::{Sidecar, SidecarSpec};

/// How many unmatched frames to retain for timeout diagnostics.
const UNMATCHED_DIAGNOSTIC_LIMIT: usize = 8;

/// Tuning for a client.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Default per-request timeout. The engine answers `prompt` immediately, so
    /// this bounds *command acceptance*, not a whole model turn.
    pub request_timeout: Duration,
    /// Capacity of the event broadcast channel.
    pub event_capacity: usize,
    /// How long to wait for the engine's `ready` frame at startup.
    pub startup_timeout: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(20),
            event_capacity: 1024,
            // Startup does discovery, plugin/MCP load and settings I/O.
            startup_timeout: Duration::from_secs(60),
        }
    }
}

/// Shared state between the client handle and its reader task.
#[derive(Debug)]
struct Shared {
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    /// Decoded session events, delivered as a typed stream.
    session_events: broadcast::Sender<SessionEvent>,
    /// Everything else that is not a response: the `ready` frame, extension-UI
    /// requests, host-tool/URI round trips, slash-command side channels, and
    /// responses that could not be correlated.
    frames: broadcast::Sender<Value>,
    ready: watch::Sender<Option<ReadyFrame>>,
    /// Set when the reader stops, with a human-readable reason.
    closed: watch::Sender<Option<String>>,
    /// Recent responses we could not correlate, for error messages.
    unmatched: Mutex<VecDeque<String>>,
    /// Logical frames reassembled from v2 chunk runs.
    reassembled_frames: AtomicU64,
}

impl Shared {
    fn record_unmatched(&self, frame: &Value) {
        if let Ok(mut queue) = self.unmatched.lock() {
            queue.push_back(summarize(frame));
            while queue.len() > UNMATCHED_DIAGNOSTIC_LIMIT {
                queue.pop_front();
            }
        }
    }

    fn unmatched_snapshot(&self) -> Vec<String> {
        self.unmatched
            .lock()
            .map(|queue| queue.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn fail_all_pending(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}

fn summarize(frame: &Value) -> String {
    let kind = frame
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<untyped>");
    match response_command(frame) {
        Some(command) => {
            let error = frame.get("error").and_then(Value::as_str).unwrap_or("");
            format!("{kind}:{command} {error}")
        }
        None => kind.to_string(),
    }
}

/// A live connection to an `omp` sidecar.
#[derive(Debug)]
pub struct OmpClient {
    sidecar: Sidecar,
    shared: Arc<Shared>,
    /// The stdout reader task, taken and awaited during shutdown.
    reader: Mutex<Option<JoinHandle<()>>>,
    next_id: AtomicU64,
    request_timeout: Duration,
}

impl OmpClient {
    /// Spawn a sidecar and start reading frames. Does **not** wait for `ready`;
    /// use [`OmpClient::connect`] for the normal path.
    pub fn spawn(spec: &SidecarSpec, options: ClientOptions) -> Result<Self, ClientError> {
        let (sidecar, stdout) = Sidecar::spawn(spec)?;
        let (session_events, _) = broadcast::channel(options.event_capacity);
        let (frames, _) = broadcast::channel(options.event_capacity);
        let (ready, _) = watch::channel(None);
        let (closed, _) = watch::channel(None);

        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            session_events,
            frames,
            ready,
            closed,
            unmatched: Mutex::new(VecDeque::new()),
            reassembled_frames: AtomicU64::new(0),
        });

        let reader = tokio::spawn(read_loop(Arc::clone(&shared), stdout));

        Ok(Self {
            sidecar,
            shared,
            reader: Mutex::new(Some(reader)),
            next_id: AtomicU64::new(1),
            request_timeout: options.request_timeout,
        })
    }

    /// Spawn, wait for the `ready` handshake, and negotiate protocol v2.
    ///
    /// Negotiation is not optional in practice: `get_available_models` returns a
    /// 1.58 MB frame at v18.2.6, well past the 1 MiB physical cap, so a v1-only
    /// client cannot read the model catalogue at all.
    pub async fn connect(spec: &SidecarSpec, options: ClientOptions) -> Result<Self, ClientError> {
        let client = Self::spawn(spec, options.clone())?;
        let ready = client.wait_for_ready(options.startup_timeout).await?;

        if ready.supports_v2() {
            let response = client
                .request(
                    crate::protocol::commands::negotiate_protocol(PROTOCOL_VERSION_V2),
                    None,
                )
                .await?;
            debug_assert_eq!(response.get("success").and_then(Value::as_bool), Some(true));
        }

        Ok(client)
    }

    /// The `ready` frame, once received.
    pub fn ready(&self) -> Option<ReadyFrame> {
        self.shared.ready.borrow().clone()
    }

    /// Whether the reader has stopped, with the reason if it failed.
    pub fn closed_reason(&self) -> Option<String> {
        self.shared.closed.borrow().clone()
    }

    /// Watch the reader's end.
    ///
    /// [`closed_reason`](Self::closed_reason) is a read, so a host that has to *react* to a
    /// dead engine — and one that parked on the read would only learn about it by polling —
    /// needs the subscription instead. This is the one signal that says the child is gone
    /// rather than merely quiet: the broadcast streams end with it, but their senders live
    /// inside this client, so a pump holding an `Arc<OmpClient>` never sees `Closed` on them.
    pub fn closed(&self) -> watch::Receiver<Option<String>> {
        self.shared.closed.subscribe()
    }

    /// Wait for the engine's startup handshake.
    pub async fn wait_for_ready(&self, timeout: Duration) -> Result<ReadyFrame, ClientError> {
        let mut receiver = self.shared.ready.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let Some(ready) = receiver.borrow().clone() {
                return Ok(ready);
            }
            match tokio::time::timeout_at(deadline, receiver.changed()).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => {
                    return Err(ClientError::Closed {
                        command: "ready".into(),
                    })
                }
                Err(_) => {
                    return Err(ClientError::Timeout {
                        command: "ready".into(),
                        timeout_ms: timeout.as_millis() as u64,
                        unmatched: self.shared.unmatched_snapshot(),
                    })
                }
            }
        }
    }

    /// Subscribe to the decoded session-event stream: the conversation,
    /// including token deltas, tool execution, compaction, and retries.
    ///
    /// Events that this build cannot decode still arrive, as
    /// [`SessionEvent::Unknown`] or [`SessionEvent::Malformed`], carrying the
    /// original frame. Nothing is dropped silently.
    pub fn subscribe_events(&self) -> broadcast::Receiver<SessionEvent> {
        self.shared.session_events.subscribe()
    }

    /// Subscribe to every non-response frame that is not a session event:
    /// the `ready` frame, extension-UI requests, host-tool/URI round trips,
    /// slash-command side channels, and uncorrelatable responses.
    ///
    /// Session events do **not** arrive here — they are on
    /// [`OmpClient::subscribe_events`], decoded once by the read loop.
    pub fn subscribe_frames(&self) -> broadcast::Receiver<Value> {
        self.shared.frames.subscribe()
    }

    /// Send a frame without expecting a response: extension-UI answers,
    /// host-tool results, host-URI results.
    pub async fn send(&self, frame: &Value) -> Result<(), ClientError> {
        self.write_command(frame).await
    }

    /// Serialize a frame, hold it to the engine's advertised frame limit, and write it.
    ///
    /// The one path every outgoing frame takes, which is the point: the limit is a
    /// property of the transport (`docs/rpc.md`: inbound commands are a single
    /// unchunked JSONL object, and clients **should** keep them within the
    /// advertised physical frame), so no caller has to remember it. The engine's
    /// reader would in fact tolerate a longer line — that is the accident this
    /// refuses to rely on.
    async fn write_command(&self, frame: &Value) -> Result<(), ClientError> {
        let line = serde_json::to_string(frame)
            .map_err(|error| ClientError::Write(std::io::Error::other(error.to_string())))?;

        // Absent until `ready` arrives, and nothing can be checked before then.
        let limit = self
            .shared
            .ready
            .borrow()
            .as_ref()
            .map(|ready| ready.max_frame_bytes);
        if let Some(limit) = limit {
            check_frame_size(line.len(), limit)?;
        }

        self.sidecar.write_raw_line(&line).await
    }

    /// Send a command and await its correlated response.
    ///
    /// # Errors
    ///
    /// `Timeout` also covers the protocol's correlation gap: the engine answers
    /// unknown commands and malformed input **without** an `id`, so such a
    /// response can never resolve a pending request. The error carries the
    /// recent uncorrelated frames so the caller can show what actually arrived
    /// instead of an unexplained hang.
    pub async fn request(
        &self,
        mut command: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, ClientError> {
        let id = format!("req_{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let name = command
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>")
            .to_string();

        command
            .as_object_mut()
            .expect("commands are JSON objects")
            .insert("id".into(), Value::String(id.clone()));

        let (sender, receiver) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .expect("pending map is not poisoned")
            .insert(id.clone(), sender);

        let write_result = self.write_command(&command).await;
        if let Err(error) = write_result {
            self.shared
                .pending
                .lock()
                .expect("pending map is not poisoned")
                .remove(&id);
            return Err(error);
        }

        let wait = timeout.unwrap_or(self.request_timeout);
        match tokio::time::timeout(wait, receiver).await {
            Ok(Ok(frame)) => Ok(frame),
            Ok(Err(_recv_error)) => Err(ClientError::Closed { command: name }),
            Err(_) => {
                self.shared
                    .pending
                    .lock()
                    .expect("pending map is not poisoned")
                    .remove(&id);
                Err(ClientError::Timeout {
                    command: name,
                    timeout_ms: wait.as_millis() as u64,
                    unmatched: self.shared.unmatched_snapshot(),
                })
            }
        }
    }

    /// Typed convenience: request and treat `success: false` as an error.
    pub async fn request_ok(
        &self,
        command: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, ClientError> {
        let response = self.request(command, timeout).await?;
        Ok(response)
    }

    /// The sidecar's process id while it is running.
    pub fn pid(&self) -> Option<u32> {
        self.sidecar.id()
    }

    /// The recent stderr tail, for error surfaces.
    pub fn stderr_tail(&self) -> String {
        self.sidecar.stderr_tail()
    }

    /// How many inbound frames required v2 chunk reassembly.
    ///
    /// Non-zero on a healthy connection: `get_available_models` is chunked in
    /// practice, which is why this is surfaced rather than inferred.
    pub fn reassembled_frames(&self) -> u64 {
        self.shared.reassembled_frames.load(Ordering::Relaxed)
    }

    /// Close stdin, wait for the engine to drain and exit, then stop.
    ///
    /// Takes `&self` rather than `self` so a shared handle (`Arc<OmpClient>`, as
    /// the desktop app holds) can still end the session. A consuming signature
    /// forced hosts to choose between leaking the child and dropping the handle
    /// instead of stopping it — and dropping loses the graceful drain, which is
    /// what lets the engine finish writing its session file.
    ///
    /// The stdout reader is deliberately not awaited: it ends on EOF once the
    /// child is gone, and waiting on it would put a second, unbounded delay in
    /// the shutdown path.
    pub async fn shutdown(&self, grace: Duration) -> Result<Option<i32>, ClientError> {
        self.sidecar.close_stdin().await;
        let status = self
            .sidecar
            .wait_or_kill(grace)
            .await
            .map_err(ClientError::Write)?;

        // The reader reaches EOF once the child is gone. Bounded anyway, so a
        // wedged pipe cannot hold up shutdown.
        let reader = self.reader.lock().ok().and_then(|mut slot| slot.take());
        if let Some(reader) = reader {
            let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
        }

        Ok(status.and_then(|status| status.code()))
    }

    /// Kill the sidecar immediately, without draining.
    ///
    /// The guarantee of last resort: used when a graceful stop cannot run, because
    /// an orphaned agent process is worse than an undrained exit.
    pub fn terminate(&self) {
        self.sidecar.kill();
    }
}

/// Read stdout forever, decoding frames and routing them.
///
/// The decoder is owned here so chunk reassembly state cannot be shared or
/// raced. On a decoding error the stream is no longer trustworthy: we publish
/// the error, fail pending requests, and stop rather than guess at alignment.
async fn read_loop(shared: Arc<Shared>, stdout: BufReader<ChildStdout>) {
    let mut lines = stdout.lines();
    let mut decoder = FrameDecoder::default();
    let mut last_reassembled = 0u64;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => {
                let _ = shared.closed.send(Some("sidecar stdout closed".into()));
                break;
            }
            Err(error) => {
                let _ = shared
                    .closed
                    .send(Some(format!("sidecar stdout read failed: {error}")));
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        match decoder.push_line(&line) {
            Ok(None) => continue,
            Ok(Some(frame)) => {
                let reassembled = decoder.reassembled_count();
                if reassembled > last_reassembled {
                    shared
                        .reassembled_frames
                        .fetch_add(reassembled - last_reassembled, Ordering::Relaxed);
                    last_reassembled = reassembled;
                }

                if let Some(ready) = ReadyFrame::from_value(&frame) {
                    decoder.apply_advertised_limits(
                        ready.max_frame_bytes,
                        ready.max_reassembled_frame_bytes,
                    );
                    let _ = shared.ready.send(Some(ready));
                }

                match classify(&frame) {
                    FrameClass::Response => {
                        let resolved = response_id(&frame)
                            .and_then(|id| {
                                shared
                                    .pending
                                    .lock()
                                    .expect("pending map is not poisoned")
                                    .remove(id)
                            })
                            .map(|sender| sender.send(frame.clone()).is_ok())
                            .unwrap_or(false);

                        if !resolved {
                            // Uncorrelatable (no id, or a duplicate). Keep it for
                            // diagnostics and let subscribers see it, rather than
                            // dropping it on the floor.
                            shared.record_unmatched(&frame);
                            let _ = shared.frames.send(frame);
                        }
                    }
                    FrameClass::Event => {
                        // Decoded once, here, so every subscriber shares one typed
                        // value instead of re-parsing the frame. Decoding borrows
                        // the frame, so a delta costs no copy.
                        let _ = shared.session_events.send(SessionEvent::decode(&frame));
                    }
                    _ => {
                        let _ = shared.frames.send(frame);
                    }
                }
            }
            Err(error) => {
                let _ = shared.frames.send(serde_json::json!({
                    "type": "transport_error",
                    "error": error.to_string(),
                }));
                let _ = shared
                    .closed
                    .send(Some(format!("frame decoding failed: {error}")));
                break;
            }
        }
    }

    shared.fail_all_pending();
}

/// Refuse a command the engine's advertised frame limit cannot carry.
///
/// Pure, so the boundary is asserted without a sidecar: the limit is inclusive
/// (`docs/rpc.md` caps each physical frame *at* 1 MiB), and an advertised limit
/// too large for this platform's `usize` simply means "no check".
fn check_frame_size(bytes: usize, limit: u64) -> Result<(), ClientError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    if bytes <= limit {
        return Ok(());
    }

    Err(ClientError::FrameTooLarge { bytes, limit })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_inside_the_advertised_limit_is_sent() {
        assert!(check_frame_size(0, 1_048_576).is_ok());
        assert!(check_frame_size(1_048_575, 1_048_576).is_ok());
    }

    #[test]
    fn the_limit_itself_is_allowed() {
        // "caps each physical stdout frame at 1 MiB" — the boundary is included,
        // and an off-by-one here would reject a frame the engine would accept.
        assert!(check_frame_size(1_048_576, 1_048_576).is_ok());
        assert!(check_frame_size(1_048_577, 1_048_576).is_err());
    }

    #[test]
    fn an_oversized_command_names_both_sizes() {
        // The message is what a caller shows when an attachment will not fit, so
        // it has to say by how much rather than just "too large".
        let error = check_frame_size(2_000_000, 1_048_576).expect_err("must refuse");
        let message = error.to_string();

        assert!(message.contains("2000000"), "got {message}");
        assert!(message.contains("1048576"), "got {message}");
        assert!(message.contains("rpc.md"), "got {message}");
    }

    #[test]
    fn a_limit_beyond_this_platform_disables_the_check() {
        assert!(check_frame_size(usize::MAX, u64::MAX).is_ok());
    }
}
