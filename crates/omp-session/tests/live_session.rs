//! End-to-end session state against a **real** `omp` sidecar.
//!
//! Ignored by default: these need the binary, provider credentials, and network.
//!
//! ```text
//! cargo test -p omp-session --test live_session -- --ignored --nocapture
//! ```
//!
//! What is worth proving here, and cannot be proved with fixtures:
//!
//! 1. the live stream and the restored history **agree** on the same turn;
//! 2. the engine really does answer a stale cursor with `code: "stale_cursor"`
//!    (the refusal [`omp_session::restore`] is built around);
//! 3. a real tool result carries the `content`/`details` the card model reads.

use std::time::Duration;

use omp_session::restore::{restore_history, RestorePolicy};
use omp_session::{Row, SessionControl, ToolOutcome, Transcript};
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, OmpClient, SessionEvent, SidecarSpec};
use serde_json::{json, Value};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(30),
        ..Default::default()
    }
}

/// Reduce the event stream until the run settles, exactly as the app will.
///
/// Returns how many tool-approval prompts the host had to answer.
///
/// # Why this answers approvals
///
/// Measured at v18.2.6: a `bash` call **blocks** until the host answers an
/// `extension_ui_request` carrying `method: "select"`,
/// `options: ["Approve","Deny"]` and a title naming the tool and command. There is
/// no fallback — with no answer the run simply never continues. So a live test
/// that ignores frames cannot exercise a tool call at all.
///
/// The real approval surface is `docs/14` step 7 (`rpc::ui` plus the dialog).
/// Until it exists this helper stands in for it, which also proves the response
/// wire format works.
async fn drain_until_settled(
    client: &OmpClient,
    transcript: &mut Transcript,
    control: &mut SessionControl,
    timeout: Duration,
) -> usize {
    use tokio::sync::broadcast::error::RecvError;

    let mut events = client.subscribe_events();
    let mut frames = client.subscribe_frames();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut seen_events: Vec<String> = Vec::new();
    let mut seen_frames: Vec<String> = Vec::new();
    let mut approvals = 0usize;

    loop {
        let event = tokio::select! {
            received = events.recv() => match received {
                Ok(event) => event,
                // A lagging subscriber is a test artefact, not a protocol event.
                Err(RecvError::Lagged(missed)) => {
                    println!("event stream lagged, dropped {missed}");
                    continue;
                }
                Err(RecvError::Closed) => panic!("event stream closed"),
            },
            received = frames.recv() => {
                match received {
                    Ok(frame) => {
                        let kind = frame
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("<untyped>")
                            .to_string();

                        if kind == "extension_ui_request" {
                            let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
                            if method == "select" {
                                let id = frame.get("id").and_then(Value::as_str).unwrap_or("");
                                approvals += 1;
                                client
                                    .send(&json!({
                                        "type": "extension_ui_response",
                                        "id": id,
                                        "value": "Approve",
                                    }))
                                    .await
                                    .expect("the approval response is written");
                            }
                        }

                        if !seen_frames.iter().any(|seen| seen.starts_with(&kind)) {
                            // The payload matters: an approval request and a
                            // widget update both arrive as frames but only one
                            // blocks the run.
                            let detail = frame.to_string();
                            let detail = &detail[..detail.len().min(400)];
                            seen_frames.push(format!("{kind}: {detail}"));
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => panic!("frame stream closed"),
                }
                continue;
            }
            () = tokio::time::sleep_until(deadline) => {
                panic!(
                    "no terminal agent_end within {timeout:?}\n  events: {seen_events:?}\n  frames: {seen_frames:?}"
                );
            }
        };

        // Both reducers see the same stream: the transcript renders it, the
        // control state tracks liveness.
        let stale = control.apply(&event);
        transcript.apply(&event);

        if matches!(event, SessionEvent::AgentStart) {
            assert!(
                control.is_streaming,
                "agent_start must flip the streaming flag before any poll"
            );
        }
        if stale {
            panic!("a payloadless notification arrived; the app would re-read state here");
        }

        if !seen_events.contains(&event.kind().to_string()) {
            seen_events.push(event.kind().to_string());
        }

        if event.ends_run() {
            return approvals;
        }
    }
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn the_live_stream_and_the_restored_history_agree() {
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();
    let client = OmpClient::connect(&spec, options())
        .await
        .expect("spawns, reports ready, and negotiates v2");

    let state = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state answers");
    let mut control = SessionControl::decode(&state["data"]).expect("state decodes");
    let mut transcript = Transcript::new();

    let accepted = client
        .request(
            commands::prompt("Reply with exactly one word: pong", &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("prompt is accepted");
    assert!(
        protocol::is_success(&accepted),
        "prompt rejected: {accepted}"
    );

    let approvals = drain_until_settled(
        &client,
        &mut transcript,
        &mut control,
        Duration::from_secs(150),
    )
    .await;
    assert_eq!(
        approvals, 0,
        "a text-only turn asks the host for nothing, so no approval may appear"
    );

    assert!(
        !control.is_streaming,
        "a terminal agent_end clears the flag"
    );

    // ---- the live transcript ---------------------------------------------
    assert_eq!(
        transcript.len(),
        2,
        "one prompt and one answer: {:?}",
        transcript.rows()
    );
    let live_text = match &transcript.rows()[1] {
        Row::Assistant { message, streaming } => {
            assert!(!streaming, "message_end must end the streaming state");
            message.text()
        }
        other => panic!("expected an assistant row, got {other:?}"),
    };
    assert!(!live_text.trim().is_empty());

    // ---- the restored history --------------------------------------------
    let restored = restore_history(&client, &RestorePolicy::default())
        .await
        .expect("paging a real session succeeds");

    println!(
        "pages: {}, busy waits: {}, restarts: {}",
        restored.pages, restored.busy_waits, restored.restarts
    );
    assert_eq!(
        restored.messages.len(),
        2,
        "the engine stores what we just streamed"
    );
    assert_eq!(restored.messages[0].role(), "user");
    assert_eq!(restored.messages[1].role(), "assistant");

    // The property the whole module exists to hold: watching a turn and
    // resuming it must show the same assistant text.
    assert_eq!(
        restored.messages[1].text(),
        live_text,
        "the restored answer must match the streamed one"
    );

    // ---- a real stale cursor ---------------------------------------------
    // One message per page leaves a cursor on; a second turn then moves the
    // session past it, which is exactly the condition the engine refuses.
    let first_page = client
        .request(commands::get_messages_page(None, Some(1)), None)
        .await
        .expect("first page answers");
    assert!(protocol::is_success(&first_page), "got {first_page}");
    let cursor = first_page["data"]["nextCursor"]
        .as_str()
        .expect("a 1-of-2 page must hand back a cursor")
        .to_string();

    let second = client
        .request(
            commands::prompt("Reply with exactly one word: again", &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("second prompt is accepted");
    assert!(protocol::is_success(&second));
    let approvals = drain_until_settled(
        &client,
        &mut transcript,
        &mut control,
        Duration::from_secs(150),
    )
    .await;
    assert_eq!(approvals, 0, "this turn is text-only too");

    let stale = client
        .request(commands::get_messages_page(Some(&cursor), Some(256)), None)
        .await
        .expect("the stale request still answers");
    assert!(
        !protocol::is_success(&stale),
        "a cursor from before the second turn must be refused, got {stale}"
    );
    assert_eq!(
        protocol::response_code(&stale),
        Some("stale_cursor"),
        "the refusal must be machine-readable, got {stale}"
    );

    // And recovery works: a fresh restore sees all four messages.
    let after = restore_history(&client, &RestorePolicy::default())
        .await
        .expect("restores again");
    assert_eq!(
        after.messages.len(),
        4,
        "two turns must be four messages after the restart"
    );

    let exit = client
        .shutdown(Duration::from_secs(10))
        .await
        .expect("shutdown completes");
    assert_eq!(exit, Some(0), "the engine exits 0 after stdin close");
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_real_tool_call_produces_a_card_with_the_engines_result() {
    // `docs/12` §3.2 claims bash streams its output and that every card can read
    // the engine's `content`/`details`. Both are load-bearing for the tool-card
    // renderers, so they are checked against a real call rather than assumed.
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();
    let client = OmpClient::connect(&spec, options())
        .await
        .expect("spawns, reports ready, and negotiates v2");

    let state = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state answers");
    let mut control = SessionControl::decode(&state["data"]).expect("state decodes");
    let mut transcript = Transcript::new();

    let accepted = client
        .request(
            commands::prompt(
                "Use the bash tool to run exactly `echo hello-from-live-test`, then reply with exactly: done",
                &[],
                None,
            ),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("prompt is accepted");
    assert!(protocol::is_success(&accepted));

    let approvals = drain_until_settled(
        &client,
        &mut transcript,
        &mut control,
        Duration::from_secs(150),
    )
    .await;
    assert!(
        approvals >= 1,
        "a tool call must be approved by the host before it can run"
    );

    let cards: Vec<_> = transcript
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Tool(card) => Some(card),
            _ => None,
        })
        .collect();

    assert!(
        !cards.is_empty(),
        "the model was told to call bash; rows: {:?}",
        transcript.rows()
    );

    let card = cards
        .iter()
        .find(|card| card.tool_name == "bash")
        .expect("a bash card");
    assert!(card.is_finished(), "the card must settle: {card:?}");
    println!(
        "bash card: args={}, streamed chunks={}, partial={:?}",
        card.args,
        card.streamed.len(),
        card.partial.is_some()
    );

    let text = match &card.outcome {
        ToolOutcome::Done(result) => {
            assert!(!result.is_error, "the echo must succeed: {result:?}");
            render_content(&result.content)
        }
        other => panic!("expected a finished card, got {other:?}"),
    };
    assert!(
        text.contains("hello-from-live-test"),
        "the card must show the tool's output, got {text:?}"
    );

    let exit = client
        .shutdown(Duration::from_secs(10))
        .await
        .expect("shutdown completes");
    assert_eq!(exit, Some(0));
}

/// Flatten a tool result's content into text, the way a card renderer does.
///
/// Tolerant on purpose: the shape differs by tool, and this only needs to prove
/// the output is reachable.
fn render_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| block.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}
