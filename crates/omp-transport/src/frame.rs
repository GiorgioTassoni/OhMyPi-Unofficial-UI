//! stdout frame decoding for the `omp --mode rpc` JSONL protocol.
//!
//! The protocol has two transports for the same frames (`docs/rpc.md`
//! § "Transport and Framing"):
//!
//! * **v1** — one JSON object per line, physically capped at 1 MiB.
//! * **v2** — opt-in after `negotiate_protocol`. Objects too large for the
//!   physical cap are emitted losslessly as an uninterrupted run of
//!   `rpc_chunk` frames, each carrying a base64 slice of the original UTF-8
//!   JSON object.
//!
//! v2 is not an edge case: `get_available_models` returns a 1.58 MB frame at
//! v18.2.6 — 1 `rpc_chunk` run — so this decoder is on the critical path for a
//! normal command, not only for unusual ones. It must reject malformed
//! sequences loudly rather than degrade: a silently mis-assembled frame would
//! surface as a plausible-looking transcript that is quietly wrong.

use base64::Engine as _;
use serde_json::Value;

use crate::error::FrameError;

/// Default physical stdout frame cap advertised by the engine (1 MiB).
pub const DEFAULT_MAX_FRAME_BYTES: usize = 1_048_576;
/// Default reassembly ceiling advertised by the engine (64 MiB).
pub const DEFAULT_MAX_REASSEMBLED_BYTES: usize = 67_108_864;

/// A chunk sequence being reassembled.
#[derive(Debug)]
struct InFlight {
    chunk_id: String,
    count: u64,
    byte_length: usize,
    next_index: u64,
    received_bytes: usize,
    segments: Vec<Vec<u8>>,
}

/// Decodes physical stdout lines into protocol frames.
///
/// Feed it one line at a time (without the trailing newline). It returns
/// `Some(frame)` when a logical frame is complete, and `None` while a chunk
/// sequence is still being reassembled.
#[derive(Debug)]
pub struct FrameDecoder {
    max_frame_bytes: usize,
    max_reassembled_bytes: usize,
    in_flight: Option<InFlight>,
    reassembled: u64,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_REASSEMBLED_BYTES)
    }
}

impl FrameDecoder {
    /// Build a decoder with explicit transport limits, normally taken from the
    /// `ready` frame the engine advertises.
    pub fn new(max_frame_bytes: usize, max_reassembled_bytes: usize) -> Self {
        Self {
            max_frame_bytes,
            max_reassembled_bytes,
            in_flight: None,
            reassembled: 0,
        }
    }

    /// How many logical frames have been reassembled from chunk runs.
    ///
    /// Exposed because v2 chunking is exercised by ordinary commands, so the
    /// wrapper needs to be able to show whether that path is live rather than
    /// assume it.
    pub fn reassembled_count(&self) -> u64 {
        self.reassembled
    }

    /// Adopt the limits advertised by a `ready` frame.
    pub fn apply_advertised_limits(&mut self, max_frame_bytes: u64, max_reassembled_bytes: u64) {
        if let Ok(limit) = usize::try_from(max_frame_bytes) {
            self.max_frame_bytes = limit;
        }
        if let Ok(limit) = usize::try_from(max_reassembled_bytes) {
            self.max_reassembled_bytes = limit;
        }
    }

    /// True while a chunk sequence is mid-flight.
    pub fn is_reassembling(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Decode one physical line.
    pub fn push_line(&mut self, line: &str) -> Result<Option<Value>, FrameError> {
        if line.len() > self.max_frame_bytes {
            return Err(FrameError::FrameTooLarge {
                limit: self.max_frame_bytes,
                actual: line.len(),
            });
        }

        let value: Value = serde_json::from_str(line)
            .map_err(|error| FrameError::InvalidJson(error.to_string()))?;
        let object = value.as_object().ok_or(FrameError::NotAnObject)?;

        if object.get("type").and_then(Value::as_str) == Some("rpc_chunk") {
            return self.push_chunk(object);
        }

        // A non-chunk frame may never appear inside a chunk run: the protocol
        // requires an uninterrupted sequence, and interleaving means the
        // logical frame we would emit next is not the one the engine sent.
        if let Some(in_flight) = &self.in_flight {
            return Err(FrameError::InterruptedSequence {
                chunk_id: in_flight.chunk_id.clone(),
                expected: in_flight.next_index,
                found: object
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<untyped>")
                    .to_string(),
            });
        }

        Ok(Some(value))
    }

    /// Buffer one `rpc_chunk`; returns the frame once the run completes.
    fn push_chunk(
        &mut self,
        chunk: &serde_json::Map<String, Value>,
    ) -> Result<Option<Value>, FrameError> {
        let chunk_id = chunk
            .get("chunkId")
            .and_then(Value::as_str)
            .ok_or(FrameError::MissingChunkId)?
            .to_string();

        let count = chunk
            .get("count")
            .and_then(Value::as_u64)
            .ok_or(FrameError::MissingChunkField("count"))?;
        if count == 0 {
            return Err(FrameError::InvalidChunkCount(count));
        }

        let index = chunk
            .get("index")
            .and_then(Value::as_u64)
            .ok_or(FrameError::MissingChunkField("index"))?;
        if index >= count {
            return Err(FrameError::InvalidChunkIndex { index, count });
        }

        let byte_length = chunk
            .get("byteLength")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(FrameError::MissingChunkField("byteLength"))?;

        // Checked from the declaration up front, so we refuse to buffer an
        // oversized payload instead of discovering it after the fact.
        if byte_length > self.max_reassembled_bytes {
            return Err(FrameError::ReassemblyCeilingExceeded {
                limit: self.max_reassembled_bytes,
                declared: byte_length,
            });
        }

        let encoded = chunk.get("data").and_then(Value::as_str).unwrap_or("");
        let segment = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| FrameError::InvalidBase64(error.to_string()))?;

        match self.in_flight.as_mut() {
            None => {
                if index != 0 {
                    return Err(FrameError::OutOfOrderChunk {
                        chunk_id,
                        expected: 0,
                        actual: index,
                    });
                }
                self.in_flight = Some(InFlight {
                    chunk_id,
                    count,
                    byte_length,
                    next_index: 1,
                    received_bytes: segment.len(),
                    segments: vec![segment],
                });
            }
            Some(in_flight) => {
                if in_flight.chunk_id != chunk_id {
                    return Err(FrameError::InterleavedSequences {
                        expected_id: in_flight.chunk_id.clone(),
                        found_id: chunk_id,
                    });
                }
                if index != in_flight.next_index {
                    return Err(FrameError::OutOfOrderChunk {
                        chunk_id: in_flight.chunk_id.clone(),
                        expected: in_flight.next_index,
                        actual: index,
                    });
                }
                // Same id but a different shape: the engine never does this, so
                // treat it as corruption rather than trust either copy.
                if in_flight.count != count || in_flight.byte_length != byte_length {
                    return Err(FrameError::ByteLengthMismatch {
                        chunk_id: in_flight.chunk_id.clone(),
                        declared: in_flight.byte_length,
                        actual: byte_length,
                    });
                }
                in_flight.received_bytes += segment.len();
                in_flight.next_index += 1;
                in_flight.segments.push(segment);
            }
        }

        let complete = self
            .in_flight
            .as_ref()
            .is_some_and(|in_flight| in_flight.next_index == in_flight.count);

        if complete {
            let frame = self.finish_sequence()?;
            return Ok(Some(frame));
        }

        Ok(None)
    }

    /// Concatenate, validate and parse a completed chunk run.
    fn finish_sequence(&mut self) -> Result<Value, FrameError> {
        let in_flight = self
            .in_flight
            .take()
            .expect("finish_sequence is only called with a sequence in flight");

        if in_flight.received_bytes != in_flight.byte_length {
            return Err(FrameError::ByteLengthMismatch {
                chunk_id: in_flight.chunk_id,
                declared: in_flight.byte_length,
                actual: in_flight.received_bytes,
            });
        }

        let mut bytes = Vec::with_capacity(in_flight.byte_length);
        for segment in in_flight.segments {
            bytes.extend_from_slice(&segment);
        }

        // Strict UTF-8: the payload is a JSON object, so an invalid sequence is
        // corruption rather than something to replace lossily.
        let text = std::str::from_utf8(&bytes).map_err(|_| FrameError::InvalidUtf8 {
            chunk_id: in_flight.chunk_id.clone(),
        })?;

        serde_json::from_str(text)
            .inspect(|_| {
                self.reassembled += 1;
            })
            .map_err(|error| FrameError::ReassembledInvalidJson {
                chunk_id: in_flight.chunk_id,
                reason: error.to_string(),
            })
    }
}
