//! Integration against a **real** `omp` sidecar.
//!
//! Ignored by default because it needs the binary (and, for anything beyond
//! state inspection, credentials). Run it explicitly:
//!
//! ```text
//! cargo test -p omp-transport --test live_sidecar -- --ignored --nocapture
//! OMP_BIN=/path/to/omp cargo test -p omp-transport --test live_sidecar -- --ignored
//! ```
//!
//! Everything asserted here was observed on `omp` v18.2.6. If the engine
//! changes, this test is the earliest signal — the `ready` frame and the
//! chunking behaviour are the first things a protocol change touches.

use std::time::Duration;

use omp_transport::protocol::{self, commands};
use omp_transport::{
    ClientError, ClientOptions, MessageDelta, OmpClient, SessionEvent, SidecarSpec,
};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(30),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "requires a real `omp` binary on PATH (or OMP_BIN)"]
async fn handshake_state_commands_and_the_correlation_gap() {
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();
    let client = OmpClient::connect(&spec, options())
        .await
        .expect("spawns, reports ready, and negotiates v2");

    // ---- the ready frame advertises exactly what docs/rpc.md documents -----
    let ready = client.ready().expect("ready frame captured during connect");
    assert_eq!(ready.protocol_version, 1, "engine still advertises v1");
    assert!(ready.supports_v2(), "v2 must be negotiable");
    assert_eq!(ready.max_frame_bytes, 1_048_576);
    assert_eq!(ready.max_reassembled_frame_bytes, 67_108_864);

    // ---- get_state is the control-panel source of truth -------------------
    let state_response = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state answers");
    assert!(
        protocol::is_success(&state_response),
        "get_state failed: {state_response}"
    );
    let state = &state_response["data"];
    assert_eq!(state["isStreaming"], serde_json::json!(false));
    assert!(state["dumpTools"].is_array(), "tool roster is exposed");
    assert!(
        state["systemPrompt"].is_array(),
        "prompt sections are exposed"
    );
    assert!(
        state["contextUsage"]["contextWindow"].is_number(),
        "context meter has a window to render against"
    );
    assert!(state["todoPhases"].is_array());
    assert!(state["thinkingLevel"].is_string());

    // ---- the chunked transport: exercise the command that actually chunks ---
    // `get_available_models` is the response that crosses the 1 MiB physical
    // cap (measured at 1.58 MB on v18.2.6). Deterministic coverage of the
    // reassembly algorithm lives in `tests/frame_decoder.rs`; this proves the
    // wiring end to end, since a broken reassembler could never deliver this
    // frame at all.
    let models_response = client
        .request(
            commands::get_available_models(),
            Some(Duration::from_secs(60)),
        )
        .await
        .expect("the model catalogue must be deliverable (it is chunked)");
    assert!(
        protocol::is_success(&models_response),
        "get_available_models failed: {models_response}"
    );

    let catalogue_bytes = serde_json::to_string(&models_response)
        .expect("a received frame re-serializes")
        .len();
    let cap = ready.max_frame_bytes as usize;
    let reassembled = client.reassembled_frames();
    println!("model catalogue: {catalogue_bytes} bytes, {reassembled} frames reassembled");

    // Necessarily true: a frame larger than the physical cap cannot have
    // arrived without reassembly, so this catches a silently bypassed path.
    if catalogue_bytes > cap {
        assert!(
            reassembled >= 1,
            "a {catalogue_bytes}-byte frame exceeds the {cap}-byte physical cap, \
             so it can only have arrived via chunk reassembly"
        );
    }

    // ---- slash-command palette source ------------------------------------
    let commands_response = client
        .request(commands::get_available_commands(), None)
        .await
        .expect("get_available_commands answers");
    assert!(protocol::is_success(&commands_response));
    let list = commands_response["data"]["commands"]
        .as_array()
        .expect("commands array");
    assert!(!list.is_empty(), "palette must not be empty");
    assert!(
        list.iter()
            .all(|entry| entry["name"].is_string() && entry["source"].is_string()),
        "every advertised command carries a name and a source"
    );
    println!("advertised commands: {}", list.len());

    // ---- the documented correlation gap ----------------------------------
    // The engine answers an unknown command WITHOUT echoing the request id, so
    // the response can never resolve our pending request. We must not hang
    // silently: the timeout has to carry evidence of what arrived.
    let error = client
        .request(
            serde_json::json!({ "type": "definitely_not_a_command" }),
            Some(Duration::from_secs(3)),
        )
        .await
        .expect_err("an uncorrelatable response cannot resolve the request");

    match error {
        ClientError::Timeout { unmatched, .. } => {
            assert!(
                !unmatched.is_empty(),
                "the timeout must report the uncorrelated frames it observed"
            );
            println!("uncorrelated frames: {unmatched:?}");
        }
        other => panic!("expected Timeout, got {other:?}"),
    }

    // ---- clean lifecycle --------------------------------------------------
    let exit = client
        .shutdown(Duration::from_secs(10))
        .await
        .expect("shutdown completes");
    assert_eq!(exit, Some(0), "the engine exits 0 after stdin close");
}

/// Drive a real turn and assert the streamed event sequence.
///
/// This is the end-to-end proof for `docs/14-build-plan.md` step 4: the typed
/// event layer is only worth having if a real run produces a well-formed,
/// fully-understood sequence. It needs credentials and network, which is why it
/// is separate from the read-only test above.
#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_prompt_streams_a_typed_event_sequence() {
    let spec = SidecarSpec::omp(std::env::temp_dir()).ephemeral();
    let client = OmpClient::connect(&spec, options())
        .await
        .expect("spawns, reports ready, and negotiates v2");

    // Subscribe before prompting: the run starts emitting immediately, and the
    // broadcast channel only delivers from the point of subscription.
    let mut events = client.subscribe_events();

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

    let mut observed: Vec<String> = Vec::new();
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();
    let mut streamed = String::new();
    let mut ended = false;

    // A real turn against a real provider: generous, but still bounded.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while !ended {
        let event = match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(error)) => panic!("event stream ended before the run finished: {error}"),
            Err(_) => panic!("no terminal agent_end within the deadline; observed {observed:?}"),
        };

        match &event {
            SessionEvent::Unknown { kind, raw } => unknown.push(format!("{kind}: {raw}")),
            SessionEvent::Malformed { kind, reason, .. } => {
                malformed.push(format!("{kind}: {reason}"))
            }
            SessionEvent::MessageUpdate(update) => match &update.assistant_message_event {
                MessageDelta::TextDelta { delta, .. } => streamed.push_str(delta),
                MessageDelta::ThinkingDelta { delta, .. } => streamed.push_str(delta),
                _ => {}
            },
            _ => {}
        }

        *counts.entry(event.kind().to_string()).or_default() += 1;
        observed.push(event.kind().to_string());
        ended = event.ends_run();
    }

    println!("event counts: {counts:?}");
    println!("assistant text: {streamed:?}");

    // The whole point of the typed layer: a real run must contain nothing this
    // build failed to understand.
    assert!(
        unknown.is_empty(),
        "unknown event types at this pinned version: {unknown:?}"
    );
    assert!(
        malformed.is_empty(),
        "known event types whose payload did not decode: {malformed:?}"
    );

    // Lifecycle, in order. Individual kinds may repeat (one message per turn),
    // so assert sequencing rather than exact counts.
    for required in [
        "agent_start",
        "turn_start",
        "message_start",
        "message_end",
        "agent_end",
    ] {
        assert!(
            observed.iter().any(|seen| seen == required),
            "a real turn must emit `{required}`; observed {observed:?}"
        );
    }

    let first = |kind: &str| observed.iter().position(|seen| seen == kind);
    assert!(
        first("agent_start") < first("turn_start"),
        "turn_start must follow agent_start: {observed:?}"
    );
    assert!(
        first("message_start") < first("message_end"),
        "message_start must precede message_end: {observed:?}"
    );
    assert_eq!(
        observed.last().map(String::as_str),
        Some("agent_end"),
        "agent_end must be last: {observed:?}"
    );

    // Deltas are the reason the delta layer exists: content must arrive
    // incrementally, not only in the final message.
    assert!(
        counts.get("message_update").copied().unwrap_or(0) > 0,
        "a real turn must stream message updates; observed {observed:?}"
    );
    assert!(
        !streamed.trim().is_empty(),
        "streamed deltas must carry text, got {streamed:?}"
    );

    let exit = client
        .shutdown(Duration::from_secs(10))
        .await
        .expect("shutdown completes");
    assert_eq!(exit, Some(0), "the engine exits 0 after stdin close");
}
