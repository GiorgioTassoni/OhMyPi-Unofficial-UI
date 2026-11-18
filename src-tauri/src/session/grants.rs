#![allow(unused_imports)]
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omp_session::{restore, AgentRoster, Message, SessionControl, Subagent, TodoPhase, Transcript};
use omp_transport::palette::{self, AdvertisedCommand};
use omp_transport::protocol::ui::{self, UiResponse};
use omp_transport::protocol::{self, commands, ImageContent};
use omp_transport::{ClientOptions, OmpClient, SessionEvent, SidecarSpec};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use crate::dialogs::Dialogs;
use crate::dto::{
    ActivitySnapshot, AgentProgressSnapshot, AgentRetrySnapshot, AgentSnapshot, AttachmentSnapshot,
    ChromeEvent, CommandSnapshot, ContextSnapshot, ControlSnapshot, CounterSnapshot,
    JobDeliverySnapshot, ModelSnapshot, NotificationEvent, ReadySnapshot, RowPatch, RowSnapshot,
    SessionStatus, SubcommandSnapshot, ThreadEvent, ThreadSnapshot, ToolSnapshot,
    UiRequestSnapshot, ACTIVITY_EVENT, AGENTS_EVENT, CHROME_EVENT, COMMANDS_EVENT,
    NOTIFICATIONS_EVENT, ROWS_EVENT, THREADS_EVENT, UI_REQUESTS_EVENT,
};
use crate::notify::{Facts, Notifier};

use super::*;

pub struct Reducers {
    pub transcript: Arc<Mutex<Transcript>>,
    pub control: Arc<Mutex<SessionControl>>,
    pub dialogs: Arc<Dialogs>,
    pub granted_tools: Arc<Mutex<HashSet<String>>>,
    pub granted_commands: Arc<Mutex<HashSet<String>>>,
    pub counters: Arc<Counters>,
    pub commands: Arc<Mutex<Vec<AdvertisedCommand>>>,
    pub agents: Arc<Mutex<AgentRoster>>,
    pub title_generation_in_flight: Arc<AtomicBool>,
    pub last_slash_command: Arc<Mutex<Option<String>>>,
}

/// A tool grant answers only that tool's exact approval. A command grant answers only a
/// simple shell invocation of the selected executable; shell operators still require approval.
/// Extension dialogs and changed approval shapes still reach the user.
pub(super) fn auto_approval_frame(
    incoming: &ui::Incoming,
    granted_tools: &Mutex<HashSet<String>>,
    granted_commands: &Mutex<HashSet<String>>,
) -> Option<serde_json::Value> {
    let ui::Incoming::Request(request) = incoming else {
        return None;
    };
    if request.kind != ui::UiRequestKind::Select || request.options != ["Approve", "Deny"] {
        return None;
    }
    let tool = request
        .title
        .lines()
        .next()?
        .strip_prefix("Allow tool: ")?
        .trim();
    if crate::policy::validate_tool_name(tool).is_err() {
        return None;
    }

    let tool_granted = granted_tools.lock().ok()?.contains(tool);
    let command_granted = if matches!(tool, "bash" | "bash_interactive") {
        command_from_approval(&request.title)
            .and_then(grantable_command_programs)
            .is_some_and(|programs| {
                granted_commands
                    .lock()
                    .is_ok_and(|set| programs.iter().all(|program| set.contains(program)))
            })
    } else {
        false
    };
    if !tool_granted && !command_granted {
        return None;
    }
    UiResponse::Value("Approve".to_string()).frame(request.kind, &request.id)
}

/// The executable at the beginning of a command, normalized to its basename.
/// This is a display/grant identity only; it never executes or rewrites the command.
pub fn command_program(command: &str) -> Result<String, String> {
    let words = shell_words::split(command)
        .map_err(|_| "the command could not be parsed safely".to_string())?;
    let executable = words
        .first()
        .filter(|word| !word.is_empty())
        .ok_or_else(|| "the command has no executable".to_string())?;
    let program = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "the command executable has no readable name".to_string())?;
    crate::policy::validate_tool_name(program)?;
    Ok(program.to_string())
}

pub(super) fn command_from_approval(title: &str) -> Option<&str> {
    let mut lines = title.lines().skip(1);
    while let Some(line) = lines.next() {
        if let Some(command) = line.strip_prefix("Command:").map(str::trim) {
            // A multiline script must never be approved from just its first line.
            return (!command.is_empty() && lines.all(|line| line.trim().is_empty()))
                .then_some(command);
        }
    }
    None
}

/// A deliberately narrow shell subset: plain commands separated by `;`, with an
/// optional final stderr redirect to `/dev/null`. Unknown syntax keeps prompting.
/// In particular, substitutions, pipelines and other redirects cannot inherit a
/// grant for just one visible executable.
pub(super) fn grantable_command_programs(command: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (index, character) in command.char_indices() {
        match (quote, character) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, ';') => {
                parts.push(&command[start..index]);
                start = index + 1;
            }
            // Reject syntax whose execution cannot be accounted for by this parser.
            (
                None,
                '|' | '&' | '<' | '$' | '`' | '(' | ')' | '{' | '}' | '\n' | '\r' | '\\' | '#',
            )
            | (Some('"'), '$' | '`' | '\\') => return None,
            _ => {}
        }
    }
    if quote.is_some() {
        return None;
    }
    parts.push(&command[start..]);

    let mut programs = Vec::new();
    for part in parts {
        let mut part = part.trim();
        // This harmless redirect occurs in OMP's own inspection commands. Do not
        // generalize it to arbitrary output paths or other redirection forms.
        if let Some(before) = part.strip_suffix("2>/dev/null") {
            if !before.ends_with(char::is_whitespace) {
                return None;
            }
            part = before.trim_end();
        }
        if part.is_empty() || part.contains('>') {
            return None;
        }
        let words = shell_words::split(part).ok()?;
        let executable = words.first()?;
        if executable.contains(['*', '?', '[', ']']) {
            return None;
        }
        let program = command_program(part).ok()?;
        if !programs.contains(&program) {
            programs.push(program);
        }
    }
    Some(programs)
}

