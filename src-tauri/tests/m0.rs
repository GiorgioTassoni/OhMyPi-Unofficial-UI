//! M0, verified without a window.
//!
//! The milestone (`docs/14` §2) is "app launches a sidecar, negotiates v2, prints
//! the `ready` frame fields". A screenshot proves the pane drew; this proves the
//! thing behind it works, and it can fail for the right reasons in CI.
//!
//! Ignored by default: it spawns a real agent and, for the prompt, needs provider
//! credentials.
//!
//! ```text
//! cargo test -p omp-desktop --test m0 -- --ignored --nocapture
//! ```

mod support;

use std::time::Duration;

use omp_desktop::dto::RowSnapshot;
use omp_desktop::session::open;
use omp_transport::protocol::{self, commands, ImageContent};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{process_exists, registry, wait_for, RecordingSink, ONE_PIXEL_PNG};

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar (and needs credentials for the prompt)"]
async fn the_shell_opens_a_session_negotiates_v2_and_streams_a_turn() {
    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();

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

    // ---- the ready handshake, as the pane shows it ------------------------
    let status = live.status().expect("status is readable");
    assert_eq!(status.ready.protocol_version, 1);
    assert!(status.ready.negotiated_v2, "v2 is not optional");
    assert_eq!(status.ready.max_frame_bytes, 1_048_576);
    assert_eq!(status.ready.max_reassembled_frame_bytes, 67_108_864);
    assert!(status.sidecar_pid.is_some(), "the agent is running");
    assert!(
        status.binary.contains("omp"),
        "the pane reports which binary it launched: {}",
        status.binary
    );

    // ---- the starting state ----------------------------------------------
    assert!(!status.control.session_id.is_empty());
    assert!(
        status.control.model.is_some(),
        "the model chip needs an identity to render"
    );
    assert_eq!(status.control.message_count, 0, "a fresh session is empty");
    assert_eq!(status.control.transcript_rows, 0);
    assert!(!status.control.is_streaming);

    // ---- one turn, reduced end to end ------------------------------------
    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt("Reply with exactly one word: pong", &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(protocol::is_success(&accepted));

    let rows = wait_for("the turn to finish", || {
        let status = live.status().ok()?;
        if status.control.is_streaming || status.counters.events_seen == 0 {
            return None;
        }
        live.rows().ok().filter(|rows| rows.len() >= 2)
    })
    .await;

    assert_eq!(rows[0].role, "user");
    assert_eq!(rows[0].text, "Reply with exactly one word: pong");
    assert_eq!(rows[1].role, "assistant");
    assert!(
        rows[1].text.to_lowercase().contains("pong"),
        "the transcript must carry the answer, got {:?}",
        rows[1].text
    );
    assert!(!rows[1].streaming, "a settled turn is not streaming");

    // ---- the pump is live -------------------------------------------------
    let counters = live.status().expect("status").counters;
    println!("counters: {counters:?}");
    assert!(counters.events_seen > 0, "events were reduced");
    assert!(
        counters.event_kinds >= 3,
        "a real turn emits several kinds, saw {}",
        counters.event_kinds
    );
    assert_eq!(counters.unknown_events, 0, "nothing should be unfamiliar");
    assert_eq!(counters.malformed_events, 0, "nothing should be unreadable");
    assert_eq!(
        counters.blocking_ui_requests, 0,
        "a text-only turn asks the host for nothing"
    );
    assert_eq!(counters.lagged_events, 0, "the consumer kept up");

    let seen = sink.kinds();
    assert!(
        seen.contains(&"agent_start".to_string()) && seen.contains(&"agent_end".to_string()),
        "the pane's event tail must have seen the turn: {seen:?}"
    );

    // ---- the conversation reached the window as patches --------------------
    // The initial read is a different path from the update path, so both are
    // checked: a viewer that only worked on load would look fine here and be blank
    // in a real session.
    //
    // Waited for rather than asserted immediately, because the publisher coalesces
    // on a tick by design — reading as soon as the rows exist would be a race, not
    // a check.
    //
    // The wait is for the *settled* turn, not for two rows: the assistant row is
    // published the moment it starts streaming, so a row count is satisfied mid-answer
    // and the assertion below would then race the model rather than check the replay.
    let folded = wait_for("the conversation to arrive as patches", || {
        let mut folded: Vec<RowSnapshot> = Vec::new();
        for patch in sink.patches() {
            assert!(
                patch.from <= folded.len(),
                "a patch must never skip rows: from {} with {} already held",
                patch.from,
                folded.len()
            );
            folded.truncate(patch.from);
            folded.extend(patch.rows);
        }
        folded
            .iter()
            .any(|row| row.role == "assistant" && !row.streaming)
            .then_some(folded)
    })
    .await;

    assert_eq!(folded[0].role, "user");
    let answer = folded
        .iter()
        .find(|row| row.role == "assistant")
        .expect("replaying every patch must rebuild the settled turn");
    assert!(!answer.streaming, "a settled turn is not streaming");
    assert!(
        answer.text.to_lowercase().contains("pong"),
        "the replayed turn must carry the answer itself, got {:?}",
        answer.text
    );

    // ---- closing leaves nothing behind ------------------------------------
    let pid = status.sidecar_pid.expect("a pid to watch");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;
}

/// An attached image reaches the row as the engine kept it, and never as text.
///
/// The attachment story in one test, because a fixture cannot answer these:
/// whether a prompt image arrives on the user message at all, whether it is
/// re-encoded on the way (the engine normalizes per model — measured: a PNG went
/// in, `image/webp` came out), and whether any of it leaked into the prompt text.
#[tokio::test]
#[ignore = "spawns a real `omp` sidecar (and needs credentials for the prompt)"]
async fn a_prompt_image_reaches_the_row_and_stays_out_of_the_text() {
    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();

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

    let message = "Reply with the single word: seen";
    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt(
                message,
                &[ImageContent::new("image/png", ONE_PIXEL_PNG)],
                None,
            ),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(protocol::is_success(&accepted));

    let rows = wait_for("the turn to finish", || {
        let status = live.status().ok()?;
        if status.control.is_streaming || status.counters.events_seen == 0 {
            return None;
        }
        live.rows().ok().filter(|rows| rows.len() >= 2)
    })
    .await;

    // ---- the attachment survived the trip ---------------------------------
    let user = &rows[0];
    assert_eq!(user.role, "user");
    assert_eq!(
        user.attachments.len(),
        1,
        "one image sent, one attachment expected: {user:?}"
    );

    let attachment = &user.attachments[0];
    assert!(
        attachment.mime_type.starts_with("image/"),
        "the engine decides the encoding, so the type is not necessarily the one \
         we sent (measured: png in, webp out). Got {:?}",
        attachment.mime_type
    );
    assert!(
        !attachment.data.is_empty(),
        "the thumbnail needs bytes to render"
    );

    // ---- and the text is the text -----------------------------------------
    assert_eq!(user.text, message);
    assert!(
        !user.text.contains(&ONE_PIXEL_PNG[..32]),
        "base64 must never be folded into the prompt text"
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.text.contains(&ONE_PIXEL_PNG[..32])),
        "and never into any other row either"
    );

    // ---- the vision path ran, which is the point of attaching -------------
    assert_eq!(rows[1].role, "assistant");
    assert!(
        rows[1].text.to_lowercase().contains("seen"),
        "the model answered about the image, got {:?}",
        rows[1].text
    );
}
