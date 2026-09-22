//! Framing tests.
//!
//! These matter more than they look: `get_available_models` returns a 1.58 MB
//! frame against `omp` v18.2.6, so reassembly runs for a normal command. A
//! mis-assembled frame would not crash — it would produce a plausible-looking
//! but wrong transcript, which is the worst failure mode this app can have.
//! Every rejection path below is therefore asserted explicitly rather than
//! assumed.

use base64::Engine as _;
use omp_transport::{FrameDecoder, FrameError};
use serde_json::json;

fn encode_segment(segment: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(segment)
}

fn chunk_frame(
    chunk_id: &str,
    index: usize,
    count: usize,
    byte_length: usize,
    segment: &[u8],
) -> String {
    json!({
        "type": "rpc_chunk",
        "chunkId": chunk_id,
        "index": index,
        "count": count,
        "byteLength": byte_length,
        "data": encode_segment(segment),
    })
    .to_string()
}

/// Split a payload into equal segments and render them as a chunk run.
fn encode_run(chunk_id: &str, payload: &[u8], chunk_size: usize) -> Vec<String> {
    let segments: Vec<&[u8]> = payload.chunks(chunk_size).collect();
    let count = segments.len();
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| chunk_frame(chunk_id, index, count, payload.len(), segment))
        .collect()
}

#[test]
fn plain_frame_passes_through_unchanged() {
    let mut decoder = FrameDecoder::default();
    let line =
        json!({ "type": "response", "command": "get_state", "id": "req_1", "success": true })
            .to_string();

    let frame = decoder
        .push_line(&line)
        .expect("plain frames decode")
        .expect("a plain frame is complete immediately");

    assert_eq!(frame["command"], "get_state");
    assert_eq!(decoder.reassembled_count(), 0);
    assert!(!decoder.is_reassembling());
}

#[test]
fn chunked_frame_reassembles_across_many_chunks() {
    let payload = json!({
        "type": "response",
        "command": "get_state",
        "id": "req_1",
        "success": true,
        "data": { "systemPrompt": ["x".repeat(500)] },
    })
    .to_string();

    let run = encode_run("rpc-1", payload.as_bytes(), 64);
    assert!(run.len() > 1, "the payload must actually be split");

    let mut decoder = FrameDecoder::default();
    let mut completed = None;
    for line in &run {
        // Every chunk but the last must buffer rather than emit.
        if let Some(frame) = decoder.push_line(line).expect("chunks decode") {
            completed = Some(frame);
        }
    }

    let frame = completed.expect("the final chunk completes the frame");
    assert_eq!(frame["command"], "get_state");
    assert_eq!(
        frame["data"]["systemPrompt"][0].as_str().unwrap().len(),
        500
    );
    assert_eq!(decoder.reassembled_count(), 1);
    assert!(!decoder.is_reassembling());
}

#[test]
fn multibyte_character_split_across_chunks_reassembles() {
    // A naive per-chunk UTF-8 decode would reject this; the protocol requires
    // decoding only after concatenation.
    let payload = json!({ "type": "notice", "text": "splitting … here" }).to_string();
    let bytes = payload.as_bytes();
    let first_byte_of_ellipsis = bytes
        .windows(3)
        .position(|window| window == "…".as_bytes())
        .expect("payload contains the multibyte character");

    // Split so the 3-byte sequence is divided across two segments.
    let run = encode_run("rpc-2", bytes, first_byte_of_ellipsis + 1);

    let mut decoder = FrameDecoder::default();
    let mut completed = None;
    for line in &run {
        if let Some(frame) = decoder.push_line(line).expect("chunks decode") {
            completed = Some(frame);
        }
    }

    assert_eq!(
        completed.expect("frame completes")["text"],
        "splitting … here"
    );
}

#[test]
fn non_chunk_frame_during_a_run_is_rejected() {
    let payload = json!({ "type": "response", "padding": "y".repeat(200) }).to_string();
    let run = encode_run("rpc-3", payload.as_bytes(), 32);
    assert!(run.len() > 1);

    let mut decoder = FrameDecoder::default();
    assert!(decoder
        .push_line(&run[0])
        .expect("first chunk buffers")
        .is_none());

    let error = decoder
        .push_line(&json!({ "type": "notice", "text": "hello" }).to_string())
        .expect_err("interrupting a run must fail");

    assert!(
        matches!(error, FrameError::InterruptedSequence { .. }),
        "got {error:?}"
    );
    assert!(
        decoder.is_reassembling(),
        "the run is left intact for the caller to abandon"
    );
}

#[test]
fn interleaved_chunk_ids_are_rejected() {
    let payload = json!({ "type": "response", "padding": "z".repeat(200) }).to_string();
    let first = encode_run("rpc-4", payload.as_bytes(), 32);
    let second = encode_run("rpc-5", payload.as_bytes(), 32);

    let mut decoder = FrameDecoder::default();
    assert!(decoder
        .push_line(&first[0])
        .expect("first chunk buffers")
        .is_none());

    let error = decoder
        .push_line(&second[0])
        .expect_err("a new chunkId mid-run must fail");

    assert!(
        matches!(error, FrameError::InterleavedSequences { .. }),
        "got {error:?}"
    );
}

#[test]
fn out_of_order_chunk_is_rejected() {
    let payload = json!({ "type": "response", "padding": "q".repeat(200) }).to_string();
    let run = encode_run("rpc-6", payload.as_bytes(), 32);

    let mut decoder = FrameDecoder::default();
    // Skipping index 0 entirely is out of order from the start.
    let error = decoder
        .push_line(&run[1])
        .expect_err("a run must begin at index 0");
    assert!(
        matches!(
            error,
            FrameError::OutOfOrderChunk {
                expected: 0,
                actual: 1,
                ..
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn byte_length_mismatch_is_detected_at_completion() {
    let payload = json!({ "type": "response", "padding": "w".repeat(100) }).to_string();
    let bytes = payload.as_bytes();
    let segments: Vec<&[u8]> = bytes.chunks(32).collect();
    let count = segments.len();

    // Every chunk lies about the total by one byte.
    let mut decoder = FrameDecoder::default();
    let mut error = None;
    for (index, segment) in segments.iter().enumerate() {
        let line = chunk_frame("rpc-7", index, count, payload.len() + 1, segment);
        match decoder.push_line(&line) {
            Ok(_) => continue,
            Err(failure) => {
                error = Some(failure);
                break;
            }
        }
    }

    assert!(
        matches!(error, Some(FrameError::ByteLengthMismatch { .. })),
        "got {error:?}"
    );
}

#[test]
fn invalid_base64_is_rejected() {
    let mut decoder = FrameDecoder::default();
    let line = json!({
        "type": "rpc_chunk",
        "chunkId": "rpc-8",
        "index": 0,
        "count": 1,
        "byteLength": 4,
        "data": "not base64 !!",
    })
    .to_string();

    let error = decoder.push_line(&line).expect_err("bad base64 must fail");
    assert!(
        matches!(error, FrameError::InvalidBase64(_)),
        "got {error:?}"
    );
}

#[test]
fn reassembly_ceiling_is_enforced_from_the_declaration() {
    let mut decoder = FrameDecoder::new(1024, 16);
    let line = json!({
        "type": "rpc_chunk",
        "chunkId": "rpc-9",
        "index": 0,
        "count": 1,
        "byteLength": 4096,
        "data": encode_segment(b"{}"),
    })
    .to_string();

    let error = decoder
        .push_line(&line)
        .expect_err("an oversized declaration must be refused before buffering");
    assert!(
        matches!(
            error,
            FrameError::ReassemblyCeilingExceeded {
                limit: 16,
                declared: 4096
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn oversized_physical_line_is_rejected() {
    let mut decoder = FrameDecoder::new(64, DEFAULT_CEILING);
    let line = json!({ "type": "notice", "text": "a".repeat(200) }).to_string();

    let error = decoder
        .push_line(&line)
        .expect_err("oversized line must fail");
    assert!(
        matches!(error, FrameError::FrameTooLarge { limit: 64, .. }),
        "got {error:?}"
    );
}

const DEFAULT_CEILING: usize = 1 << 20;

#[test]
fn reassembled_payload_must_be_json() {
    let run = encode_run("rpc-10", b"this is not json", 4);

    let mut decoder = FrameDecoder::default();
    let mut error = None;
    for line in &run {
        if let Err(failure) = decoder.push_line(line) {
            error = Some(failure);
            break;
        }
    }

    assert!(
        matches!(error, Some(FrameError::ReassembledInvalidJson { .. })),
        "got {error:?}"
    );
}

#[test]
fn reassembled_payload_must_be_utf8() {
    let invalid = [0xff, 0xfe, 0xfd, 0xfc];
    let line = chunk_frame("rpc-11", 0, 1, invalid.len(), &invalid);

    let mut decoder = FrameDecoder::default();
    let error = decoder
        .push_line(&line)
        .expect_err("invalid UTF-8 must fail after concatenation");
    assert!(
        matches!(error, FrameError::InvalidUtf8 { .. }),
        "got {error:?}"
    );
}

#[test]
fn malformed_chunk_fields_are_rejected() {
    let cases = [
        json!({ "type": "rpc_chunk", "index": 0, "count": 1, "byteLength": 2, "data": "e30=" }),
        json!({ "type": "rpc_chunk", "chunkId": "c", "count": 1, "byteLength": 2, "data": "e30=" }),
        json!({ "type": "rpc_chunk", "chunkId": "c", "index": 0, "byteLength": 2, "data": "e30=" }),
        json!({ "type": "rpc_chunk", "chunkId": "c", "index": 0, "count": 1, "data": "e30=" }),
        json!({ "type": "rpc_chunk", "chunkId": "c", "index": 5, "count": 1, "byteLength": 2, "data": "e30=" }),
        json!({ "type": "rpc_chunk", "chunkId": "c", "index": 0, "count": 0, "byteLength": 2, "data": "e30=" }),
    ];

    for case in cases {
        let mut decoder = FrameDecoder::default();
        let error = decoder
            .push_line(&case.to_string())
            .expect_err(&format!("{case} must be rejected"));
        assert!(
            matches!(
                error,
                FrameError::MissingChunkId
                    | FrameError::MissingChunkField(_)
                    | FrameError::InvalidChunkIndex { .. }
                    | FrameError::InvalidChunkCount(_)
            ),
            "unexpected error for {case}: {error:?}"
        );
    }
}

#[test]
fn consecutive_runs_each_complete() {
    let first = json!({ "type": "notice", "text": "1".repeat(120) }).to_string();
    let second = json!({ "type": "notice", "text": "2".repeat(120) }).to_string();

    let mut decoder = FrameDecoder::default();
    let mut completed = Vec::new();
    for (chunk_id, payload) in [("a", first), ("b", second)] {
        for line in encode_run(chunk_id, payload.as_bytes(), 32) {
            if let Some(frame) = decoder.push_line(&line).expect("chunks decode") {
                completed.push(frame);
            }
        }
    }

    assert_eq!(completed.len(), 2);
    assert_eq!(completed[0]["text"].as_str().unwrap().len(), 120);
    assert_eq!(completed[1]["text"].as_str().unwrap().len(), 120);
    assert_eq!(decoder.reassembled_count(), 2);
}

#[test]
fn advertised_limits_are_adopted() {
    let mut decoder = FrameDecoder::default();
    // Shrink the physical limit, then prove it is enforced.
    decoder.apply_advertised_limits(32, DEFAULT_CEILING as u64);
    let line = json!({ "type": "notice", "text": "b".repeat(80) }).to_string();

    let error = decoder.push_line(&line).expect_err("new limit applies");
    assert!(
        matches!(error, FrameError::FrameTooLarge { limit: 32, .. }),
        "got {error:?}"
    );
}
