//! Transport for driving the Oh My Pi (`omp`) coding agent.
//!
//! This crate is the only place that knows the wire protocol. Everything above
//! it — the Tauri shell, the frontend state, the renderers — consumes typed
//! frames and commands, so an upstream protocol change lands here and nowhere
//! else (`docs/11-v1-scope.md` §3.2).
//!
//! ```no_run
//! use omp_transport::{ClientOptions, OmpClient, SidecarSpec};
//! use omp_transport::protocol::commands;
//!
//! # async fn run() -> Result<(), omp_transport::ClientError> {
//! let spec = SidecarSpec::omp("/path/to/project").ephemeral();
//! let client = OmpClient::connect(&spec, ClientOptions::default()).await?;
//!
//! let response = client.request(commands::get_state(), None).await?;
//! println!("state: {response}");
//!
//! client.shutdown(std::time::Duration::from_secs(5)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Layout
//!
//! * [`sidecar`] — spawn, supervise, and shut down the child process.
//! * [`frame`] — physical line to logical frame, including v2 chunk reassembly.
//! * [`protocol`] — the `ready` handshake, frame classification, and one
//!   constructor per `RpcCommand`.
//! * [`events`] — typed decoding of the streamed session events, tolerant of
//!   event types this build does not know.
//! * [`palette`] — the command list and a builtin command's output: the two
//!   slash-command frames, which arrive beside the events rather than in them.
//! * [`client`] — request/response correlation, typed event broadcast.
//! * [`error`] — one variant per distinguishable failure.

pub mod client;
pub mod error;
pub mod events;
pub mod frame;
pub mod palette;
pub mod protocol;
pub mod sidecar;

pub use client::{ClientOptions, OmpClient};
pub use error::{ClientError, FrameError};
pub use events::{MessageDelta, SessionEvent};
pub use frame::{FrameDecoder, DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_REASSEMBLED_BYTES};
pub use palette::{AdvertisedCommand, AdvertisedSubcommand};
pub use protocol::{
    classify, is_success, response_code, FrameClass, ReadyFrame, CODE_SESSION_BUSY,
    CODE_STALE_CURSOR,
};
pub use sidecar::{resolve_omp_binary, Sidecar, SidecarSpec};
