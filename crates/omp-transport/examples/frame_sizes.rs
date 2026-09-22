//! Protocol instrument: report per-command response sizes and whether each
//! response needed v2 chunk reassembly.
//!
//! Run against a real sidecar:
//!
//! ```text
//! cargo run -p omp-transport --example frame_sizes
//! ```
//!
//! Why this exists: chunk reassembly is not a rare path, but *which* commands
//! cross the physical frame cap is not obvious from the docs — and getting it
//! wrong means building the transport around a case that never happens while
//! missing the one that does. This prints the truth for the pinned version.

use std::time::Duration;

use omp_transport::protocol::commands;
use omp_transport::{ClientOptions, OmpClient, SidecarSpec};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::args()
        .nth(1)
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
    let spec = SidecarSpec::omp(&cwd).ephemeral();
    let client = OmpClient::connect(&spec, ClientOptions::default()).await?;

    if let Some(ready) = client.ready() {
        println!(
            "ready: protocol v{} (supports {:?}), physical cap {} B, reassembly ceiling {} B",
            ready.protocol_version,
            ready.supported_protocol_versions,
            ready.max_frame_bytes,
            ready.max_reassembled_frame_bytes
        );
    }
    println!();
    println!("{:<26} {:>12} {:>8}  outcome", "command", "bytes", "chunks");
    println!("{}", "-".repeat(70));

    let probes: Vec<(&str, serde_json::Value)> = vec![
        ("get_state", commands::get_state()),
        ("get_available_commands", commands::get_available_commands()),
        ("get_login_providers", commands::get_login_providers()),
        ("get_subagents", commands::get_subagents()),
        ("get_branch_messages", commands::get_branch_messages()),
        (
            "get_last_assistant_text",
            commands::get_last_assistant_text(),
        ),
        ("get_messages", commands::get_messages()),
        ("get_session_stats", commands::get_session_stats()),
        // The model catalogue is the suspected chunked response: it can be
        // megabytes, and it is network-bound, so give it room.
        ("get_available_models", commands::get_available_models()),
    ];

    let mut total_chunks = 0u64;
    for (name, command) in probes {
        let before = client.reassembled_frames();
        match client.request(command, Some(Duration::from_secs(30))).await {
            Ok(response) => {
                let chunks = client.reassembled_frames() - before;
                total_chunks += chunks;
                let bytes = serde_json::to_string(&response)?.len();
                let outcome = if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
                    "ok"
                } else {
                    "reported failure"
                };
                println!("{name:<26} {bytes:>12} {chunks:>8}  {outcome}");
            }
            Err(error) => {
                let first_line = error.to_string().lines().next().unwrap_or("").to_string();
                println!("{name:<26} {:>12} {:>8}  {first_line}", "-", "-");
                // Placeholder row: the command did not answer, so there are no
                // sizes to report. The reason is in the final column.
            }
        }
    }

    println!("{}", "-".repeat(70));
    println!("total frames reassembled from chunks: {total_chunks}");

    client.shutdown(Duration::from_secs(10)).await?;
    Ok(())
}
