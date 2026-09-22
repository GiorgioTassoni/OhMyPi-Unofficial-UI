//! The composer's turn controls, proved without a window.
//!
//! `docs/12` §5.2 gives `Enter` two meanings (send when idle, steer when streaming)
//! and §3.4 adds `Stop` and `Stop and send`. What a window cannot show is whether
//! those commands actually reach a running turn — and until this test existed, the
//! only send path was `prompt`, which the engine **rejects** while a turn is
//! streaming. That was a dead end for anyone who typed while the agent worked.
//!
//! Both tests park the run on a pending approval dialog first: an unanswered dialog
//! blocks the turn `docs/12` §10 describes, which makes "a turn is in flight" a fact
//! rather than a race against how fast the model answers.
//!
//! Ignored by default: they spawn a real agent and need provider credentials.
//!
//! ```text
//! cargo test -p omp-desktop --test composer -- --ignored --nocapture
//! ```

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::{self, commands, ImageContent};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{process_exists, registry, wait_for, RecordingSink, ONE_PIXEL_PNG};

/// A prompt that parks the run on an approval dialog, and why it is phrased this way.
///
/// Same shape the other live suites measured: an explicit tool, an exact command,
/// and a short expected answer, so the turn ends instead of wandering.
fn blocking_prompt(file: &Path) -> String {
    format!(
        "Use the bash tool to run exactly `touch {}`, then reply with exactly: done",
        file.display()
    )
}

async fn open_session(workspace: &Path) -> (Arc<LiveSession>, RecordingSink) {
    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(workspace).ephemeral();
    let live = open(
        &spec,
        ClientOptions {
            request_timeout: Duration::from_secs(30),
            ..Default::default()
        },
        sink.clone(),
        registry(),
    )
    .await
    .expect("the session opens");

    (live, sink)
}

/// A prompt whose answer streams for long enough to steer and stop mid-turn.
///
/// The window matters: both tests below are about a message that arrives *while* the
/// model is writing, so the answer has to take tens of seconds rather than one. No
/// tools, so nothing parks on an approval and the turn streams uninterrupted.
const LONG_ANSWER: &str =
    "Write a detailed 600-word essay about the history of lighthouses. Do not use any tools.";

/// Wait until the model is mid-answer: an assistant row that exists and is still
/// growing. That is "a turn is in flight" in the only sense the composer acts on.
async fn wait_for_streaming_text(live: &LiveSession) -> String {
    let rows = wait_for("the answer to start streaming", || {
        let rows = live.rows().ok()?;
        rows.iter()
            .any(|row| row.role == "assistant" && row.streaming && !row.text.is_empty())
            .then_some(rows)
    })
    .await;

    rows.iter()
        .find(|row| row.role == "assistant")
        .map(|row| row.text.clone())
        .expect("an assistant row")
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_message_sent_mid_turn_is_accepted_and_reaches_the_transcript() {
    // The dead end this test exists for: `prompt` is **rejected** by the engine
    // while a turn is streaming, so before steering existed, typing mid-turn could
    // only fail.
    let workspace = std::env::temp_dir();
    let (live, _sink) = open_session(&workspace).await;

    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::prompt(LONG_ANSWER, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(
        protocol::is_success(&accepted),
        "prompt rejected: {accepted}"
    );

    let streaming = wait_for_streaming_text(&live).await;
    println!("streaming so far: {} chars", streaming.len());

    let steering = "Reply with exactly one word: steered";
    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::steer(steering, &[]),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the steering message is accepted");
    assert!(
        protocol::is_success(&accepted),
        "steering while a turn streams must be accepted: {accepted}"
    );

    // It reaches the conversation rather than being acknowledged and dropped.
    wait_for("the steering message to appear in the transcript", || {
        live.rows()
            .ok()?
            .iter()
            .any(|row| row.role == "user" && row.text.contains("steered"))
            .then_some(())
    })
    .await;

    let pid = live.status().expect("status").sidecar_pid.expect("a pid");
    live.stop_turn().await.expect("the turn stops");
    wait_for("the turn to end", || {
        let status = live.status().ok()?;
        (!status.control.is_streaming).then_some(())
    })
    .await;

    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;
}

/// An attachment steered into a running turn rides with the message.
///
/// `docs/rpc.md` gives `images` to `steer` as well as `prompt`, and 8d's composer takes
/// it at its word: a picture pasted while the agent is writing steers, it does not wait
/// for the turn to end. Documented is not verified, though — an engine that ignored the
/// field would still accept the words and drop the picture **silently**, which is the
/// failure this test exists to rule out.
#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn an_image_steered_mid_turn_arrives_with_the_message() {
    let workspace = std::env::temp_dir();
    let (live, _sink) = open_session(&workspace).await;

    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::prompt(LONG_ANSWER, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(
        protocol::is_success(&accepted),
        "prompt rejected: {accepted}"
    );

    wait_for_streaming_text(&live).await;

    let steering = "Reply with exactly one word: seen";
    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::steer(steering, &[ImageContent::new("image/png", ONE_PIXEL_PNG)]),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the steering message is accepted");
    assert!(
        protocol::is_success(&accepted),
        "steering with an image must be accepted: {accepted}"
    );

    // Matched on its own words, not on a substring the essay could also contain.
    let row = wait_for("the steered message to appear", || {
        live.rows()
            .ok()?
            .into_iter()
            .find(|row| row.role == "user" && row.text.contains("Reply with exactly one word"))
    })
    .await;

    assert_eq!(
        row.attachments.len(),
        1,
        "one image steered, one attachment expected: {row:?}"
    );
    assert!(
        row.attachments[0].mime_type.starts_with("image/"),
        "the engine decides the encoding (measured: png in, webp out). Got {:?}",
        row.attachments[0].mime_type
    );
    assert!(
        !row.text.contains(&ONE_PIXEL_PNG[..32]),
        "the bytes must not leak into the prompt text: {}",
        row.text
    );

    let pid = live.status().expect("status").sidecar_pid.expect("a pid");
    live.stop_turn().await.expect("the turn stops");
    wait_for("the turn to end", || {
        let status = live.status().ok()?;
        (!status.control.is_streaming).then_some(())
    })
    .await;

    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn stopping_a_streaming_turn_ends_it() {
    let workspace = std::env::temp_dir();
    let (live, _sink) = open_session(&workspace).await;

    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::prompt(LONG_ANSWER, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(protocol::is_success(&accepted));

    let streaming = wait_for_streaming_text(&live).await;
    assert!(!streaming.is_empty(), "the answer was streaming");

    live.stop_turn().await.expect("the stop is accepted");

    wait_for("the turn to end", || {
        let status = live.status().ok()?;
        (!status.control.is_streaming).then_some(())
    })
    .await;

    // The settled row is what the card shows, so it has to exist and have stopped.
    let rows = live.rows().expect("rows");
    assert!(
        rows.iter()
            .any(|row| row.role == "assistant" && !row.streaming),
        "an aborted turn leaves the assistant row the engine settled: {rows:#?}"
    );

    let pid = live.status().expect("status").sidecar_pid.expect("a pid");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn stop_ends_a_run_that_is_parked_on_an_approval() {
    // **Measured**: `abort` on its own does not. A turn waiting on an approval is
    // blocked inside the tool call, and the engine's `abort` handler waits for the
    // turn to stop before answering — 30 s with no response, dialog still on screen.
    // So stopping refuses what is pending first, which is what this pins: without
    // it, the stop button is dead in exactly the case where a user reaches for it.
    let workspace = std::env::temp_dir();
    let (live, sink) = open_session(&workspace).await;

    let accepted = live
        .client()
        .expect("the session is open")
        .request(
            commands::prompt(
                blocking_prompt(&workspace.join("omp-desktop-parked.txt")),
                &[],
                None,
            ),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(protocol::is_success(&accepted));

    let dialog = wait_for("the agent to ask for approval", || {
        sink.dialogs().into_iter().next()
    })
    .await;
    assert!(
        live.status().expect("status").control.is_streaming,
        "a run parked on a dialog is a turn in flight"
    );

    live.stop_turn().await.expect("the stop is accepted");

    wait_for("the turn to end", || {
        let status = live.status().ok()?;
        (!status.control.is_streaming).then_some(())
    })
    .await;

    // And the dialog it was waiting on is gone: its tool call no longer exists, so
    // an answer to it could never take effect.
    let pending = wait_for("the dialog to be withdrawn", || {
        live.pending_ui_requests()
            .ok()
            .filter(|pending| pending.is_empty())
    })
    .await;
    assert!(pending.is_empty(), "got {pending:?}");
    println!("refused dialog: {dialog:?}");

    let pid = live.status().expect("status").sidecar_pid.expect("a pid");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;
}
