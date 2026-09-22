//! The palette, end to end, against a real engine.
//!
//! Two things only a live session can answer, and both were open questions before this
//! test existed: whether a fresh session's palette is *populated* (the engine pushes the
//! command array at startup, but measured, that push is emitted before any subscriber can
//! attach — so the list has to be fetched), and whether a local-only command's output
//! reaches the transcript (it arrives as a `command_output` **frame**, not a session
//! event, so nothing the transcript reducer knows about would have delivered it).
//!
//! Ignored by default: they spawn a real agent, and the second one needs credentials.
//!
//! ```text
//! cargo test -p omp-desktop --test palette -- --ignored --nocapture
//! ```

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::commands;
use omp_transport::{ClientOptions, SidecarSpec};
use support::{registry, wait_for, RecordingSink};

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

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn the_palette_answers_with_the_commands_a_palette_renders() {
    let (live, _sink) = open_session(&std::env::temp_dir()).await;

    let first = live.commands().await.expect("the palette list");
    assert!(
        first.len() > 20,
        "a palette of {} commands is not a palette — the list is probably parsed wrong",
        first.len()
    );

    // Measured at v18.2.6: 45 commands, four sources, and only `input.hint` inside `input`.
    let sources: std::collections::BTreeSet<&str> = first
        .iter()
        .map(|command| command.source.as_str())
        .collect();
    assert!(
        sources.contains("builtin"),
        "a builtin source is the floor: {sources:?}"
    );
    assert!(
        first.iter().all(|command| !command.name.is_empty()),
        "every row has a name to show and to dispatch"
    );

    // The one field a palette branches on: a command that wants arguments must say so,
    // because otherwise it gets dispatched on a single keypress.
    let with_subcommands = first
        .iter()
        .find(|command| !command.subcommands.is_empty())
        .expect("at least one command has subcommands");
    assert!(
        with_subcommands.hint.is_some() || !with_subcommands.subcommands.is_empty(),
        "{} takes input",
        with_subcommands.name
    );
    assert!(
        with_subcommands
            .subcommands
            .iter()
            .all(|sub| !sub.name.is_empty()),
        "a subcommand without a name cannot be offered"
    );

    // `model` carries an alias in the live list (`models`), which is why aliases are
    // read at all: they match, they are never inserted.
    let aliased = first.iter().find(|command| !command.aliases.is_empty());
    if let Some(command) = aliased {
        println!("aliased: {} → {:?}", command.name, command.aliases);
    }

    // The second ask is the cache, not another round trip: the array is 14 KB and the
    // palette opens constantly.
    let second = live.commands().await.expect("the cached palette list");
    assert_eq!(first, second, "the cache must answer the same list");

    live.shutdown(Duration::from_secs(10)).await;
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_local_command_answers_without_a_turn_and_its_output_becomes_a_row() {
    let (live, _sink) = open_session(&std::env::temp_dir()).await;

    // `/model` is local-only: measured, it answers `agentInvoked: false` and sends its
    // text on `command_output` instead of running a turn. It is also why the palette can
    // offer a command that produces output but no assistant message.
    let response = live
        .client()
        .expect("the session is open")
        .request(
            commands::prompt("/model", &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the engine answers");

    assert!(
        omp_transport::is_success(&response),
        "a local command is accepted: {response}"
    );
    assert_eq!(
        response["data"]["agentInvoked"],
        serde_json::json!(false),
        "a local-only command must not start a turn: {response}"
    );

    // The frame reaches the transcript as a notice row — the path that did not exist
    // before `apply_command_output`: `command_output` is a frame, so the events stream
    // never carried it and the row would silently never appear.
    let row = wait_for("the command's output to become a row", || {
        live.rows()
            .ok()?
            .into_iter()
            .find(|row| row.text.contains("Current model"))
    })
    .await;

    assert!(
        row.role.starts_with("notice"),
        "output is a notice, not an assistant turn: {row:?}"
    );
    assert!(
        row.text.contains("deepseek") || row.text.contains('/'),
        "the row carries the engine's own text: {}",
        row.text
    );

    // And the session is immediately usable again: a local command leaves no turn behind.
    let status = live.status().expect("status");
    assert!(
        !status.control.is_streaming,
        "a local command must not leave the session streaming"
    );

    live.shutdown(Duration::from_secs(10)).await;
}
