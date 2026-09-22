//! Error types for the `omp` transport.
//!
//! Every failure the protocol can produce has a distinct variant, because the
//! wrapper's rule is that no failure is silent: each one must be renderable to
//! the user with a specific cause. See `docs/14-build-plan.md` §7.6.

use thiserror::Error;

/// Failure while decoding a stdout frame.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame exceeds the {limit} byte physical limit (got {actual} bytes)")]
    FrameTooLarge { limit: usize, actual: usize },

    #[error("frame is not valid JSON: {0}")]
    InvalidJson(String),

    #[error("frame is not a JSON object")]
    NotAnObject,

    #[error("rpc_chunk is missing or has a non-string `chunkId`")]
    MissingChunkId,

    #[error("rpc_chunk is missing or has a non-numeric `{0}`")]
    MissingChunkField(&'static str),

    #[error("rpc_chunk has an invalid `index` ({index}) for `count` ({count})")]
    InvalidChunkIndex { index: u64, count: u64 },

    #[error("rpc_chunk declares `count` {0}, which is not a positive chunk count")]
    InvalidChunkCount(u64),

    #[error("rpc_chunk has an invalid `byteLength`")]
    InvalidByteLength,

    #[error("rpc_chunk `data` is not valid base64: {0}")]
    InvalidBase64(String),

    #[error(
        "chunk sequence interrupted: expected chunk `{expected}` of `{chunk_id}` but received a `{found}` frame"
    )]
    InterruptedSequence {
        chunk_id: String,
        expected: u64,
        found: String,
    },

    #[error("chunk `{expected_id}` was still in flight when chunk `{found_id}` began")]
    InterleavedSequences {
        expected_id: String,
        found_id: String,
    },

    #[error("chunk `{chunk_id}` arrived out of order: expected index {expected}, got {actual}")]
    OutOfOrderChunk {
        chunk_id: String,
        expected: u64,
        actual: u64,
    },

    #[error(
        "chunk `{chunk_id}` reassembled to {actual} bytes but declared `byteLength` {declared}"
    )]
    ByteLengthMismatch {
        chunk_id: String,
        declared: usize,
        actual: usize,
    },

    #[error("reassembled frame exceeds the {limit} byte reassembly ceiling (declared {declared})")]
    ReassemblyCeilingExceeded { limit: usize, declared: usize },

    #[error("reassembled chunk `{chunk_id}` is not valid UTF-8")]
    InvalidUtf8 { chunk_id: String },

    #[error("reassembled chunk `{chunk_id}` is not valid JSON: {reason}")]
    ReassembledInvalidJson { chunk_id: String, reason: String },
}

/// Failure while talking to a sidecar.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("failed to spawn the sidecar `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("sidecar stdin is closed")]
    StdinClosed,

    #[error("failed to write a command to the sidecar: {0}")]
    Write(#[source] std::io::Error),

    #[error(
        "timed out after {timeout_ms} ms waiting for a `{command}` response. \
         Unmatched frames received in the meantime: {unmatched:?}. \
         Note: the engine answers unknown or malformed commands WITHOUT an `id`, \
         so such responses cannot be correlated (docs/rpc.md)."
    )]
    Timeout {
        command: String,
        timeout_ms: u64,
        unmatched: Vec<String>,
    },

    #[error("the sidecar closed its stdout before answering `{command}`")]
    Closed { command: String },

    #[error("frame decode failed: {0}")]
    Frame(#[from] FrameError),

    #[error(
        "refusing to send a {bytes}-byte command: the engine advertises a {limit}-byte \
         physical frame, and inbound commands are never chunked (`docs/rpc.md`). \
         A payload that big — an image attachment, a host-URI result — has to be \
         shrunk or paged before it is sent."
    )]
    FrameTooLarge { bytes: usize, limit: usize },
}
