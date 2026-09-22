//! Protocol constants, the `ready` handshake, frame classification, and typed
//! constructors for every `RpcCommand` v1 uses.
//!
//! The wire contract is defined by `docs/rpc.md` and
//! `packages/coding-agent/src/modes/rpc/rpc-types.ts` at `omp` v18.2.6. Keeping
//! the constructors here — rather than spraying `serde_json::json!` through the
//! app — is what lets the app layer stay transport-agnostic.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Protocol version we speak by default. The engine advertises v1 in its
/// `ready` frame and accepts an opt-in upgrade.
pub const PROTOCOL_VERSION_V2: u64 = 2;

/// The command that opts into lossless chunked transport.
pub const NEGOTIATE_PROTOCOL: &str = "negotiate_protocol";

/// The engine's startup handshake.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyFrame {
    pub protocol_version: u64,
    pub supported_protocol_versions: Vec<u64>,
    pub max_frame_bytes: u64,
    pub max_reassembled_frame_bytes: u64,
}

impl ReadyFrame {
    /// Parse a `ready` frame, or `None` if this is not one.
    pub fn from_value(value: &Value) -> Option<Self> {
        if value.get("type").and_then(Value::as_str) != Some("ready") {
            return None;
        }
        serde_json::from_value(value.clone()).ok()
    }

    /// Whether the engine accepts the v2 upgrade we rely on.
    pub fn supports_v2(&self) -> bool {
        self.supported_protocol_versions
            .contains(&PROTOCOL_VERSION_V2)
    }
}

/// An image attached to a prompt.
///
/// The engine's `ImageContent`, reduced to the three fields a host sets. It is a
/// **field of the command**, never text appended to the message: `docs/rpc.md`
/// defines `prompt{images}` for exactly this, and the base64 costs image tokens
/// rather than text tokens once the engine has normalized it per model
/// (`images.autoResize`, `images.blockImages`).
///
/// The engine re-encodes what it receives — measured: a PNG went in and
/// `image/webp` came back on the user message — so a caller sends the original
/// bytes and lets the engine decide, rather than pre-processing against a budget
/// it does not know.
///
/// Note what this type does **not** solve: the whole command has to fit the
/// engine's advertised physical frame (1 MiB at v18.2.6), and base64 inflates
/// bytes by a third. Preparing an image to fit is the caller's job, and
/// [`crate::client::OmpClient::send`] refuses a frame that would not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    /// Base64 payload, without a `data:` prefix.
    pub data: String,
    /// `image/png`, `image/jpeg`, … as the caller's source named it.
    pub mime_type: String,
}

impl ImageContent {
    pub fn new(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            data: data.into(),
        }
    }

    /// The wire object: `{ "type": "image", "data": …, "mimeType": … }`.
    fn to_value(&self) -> Value {
        json!({
            "type": "image",
            "data": self.data,
            "mimeType": self.mime_type,
        })
    }
}

/// How the app should route an inbound stdout frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameClass {
    /// The startup handshake.
    Ready,
    /// A correlated command result.
    Response,
    /// A streamed `AgentSessionEvent`.
    Event,
    /// A dialog/message the host must render (select, confirm, input, editor,
    /// notify, setStatus, setWidget, setTitle, set_editor_text, cancel, open_url).
    ExtensionUiRequest,
    /// The engine wants a host-owned tool executed.
    HostToolCall,
    /// A pending host tool call was aborted.
    HostToolCancel,
    /// The engine wants a host-owned URI read/written.
    HostUriRequest,
    /// A pending host URI request was aborted.
    HostUriCancel,
    /// Output from a slash command that produced no agent turn.
    CommandOutput,
    /// Slash-command metadata, emitted at startup and on change.
    AvailableCommandsUpdate,
    /// A deferred local-only prompt resolved.
    PromptResult,
    /// Builtin slash-command side channels.
    SessionInfoUpdate,
    /// Builtin slash-command side channels.
    ConfigUpdate,
    /// Forwarded subagent lifecycle/progress/event frames.
    Subagent,
    /// An extension runner error.
    ExtensionError,
    /// A frame type this version of the client does not know.
    Unknown,
}

/// Classify an inbound frame by its `type` discriminants.
///
/// `rpc_chunk` never reaches here: the decoder consumes chunk frames and emits
/// only reassembled logical frames.
pub fn classify(value: &Value) -> FrameClass {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return FrameClass::Unknown;
    };
    match kind {
        "ready" => FrameClass::Ready,
        "response" => FrameClass::Response,
        "extension_ui_request" => FrameClass::ExtensionUiRequest,
        "host_tool_call" => FrameClass::HostToolCall,
        "host_tool_cancel" => FrameClass::HostToolCancel,
        "host_uri_request" => FrameClass::HostUriRequest,
        "host_uri_cancel" => FrameClass::HostUriCancel,
        "command_output" => FrameClass::CommandOutput,
        "available_commands_update" => FrameClass::AvailableCommandsUpdate,
        "prompt_result" => FrameClass::PromptResult,
        "session_info_update" => FrameClass::SessionInfoUpdate,
        "config_update" => FrameClass::ConfigUpdate,
        "extension_error" => FrameClass::ExtensionError,
        "subagent_lifecycle" | "subagent_progress" | "subagent_event" => FrameClass::Subagent,
        // Everything else on stdout is an AgentSessionEvent; the app decodes the
        // ones it renders and ignores the rest.
        _ => FrameClass::Event,
    }
}

/// The `id` echoed on a response, when the engine supplied one.
///
/// `None` is meaningful: parse failures and unknown commands are answered
/// **without** an id, so they cannot be correlated (see `error::ClientError`).
pub fn response_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str)
}

/// The command name echoed on a response.
pub fn response_command(value: &Value) -> Option<&str> {
    value.get("command").and_then(Value::as_str)
}

/// Whether a response reports success.
pub fn is_success(value: &Value) -> bool {
    value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The machine-readable `code` on a failed response, when the engine supplied one.
///
/// Failures that clients must react to differently — rather than merely display —
/// carry one: `session_busy` and `stale_cursor` on `get_messages_page`, for
/// instance, need a retry and a restart respectively. Matching on prose would
/// break the moment upstream rewords a message, so this is the only supported way
/// to tell those apart.
pub fn response_code(value: &Value) -> Option<&str> {
    value.get("code").and_then(Value::as_str)
}

/// `get_messages_page` refused because a turn is streaming or compacting.
pub const CODE_SESSION_BUSY: &str = "session_busy";

/// `get_messages_page` refused because the cursor's session snapshot is stale.
pub const CODE_STALE_CURSOR: &str = "stale_cursor";

fn base(kind: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("type".into(), Value::String(kind.into()));
    map
}

fn command(kind: &str) -> Value {
    Value::Object(base(kind))
}

/// Typed constructors for the `RpcCommand` set.
///
/// One function per command; `id` is assigned by the client, not here.
pub mod commands {
    use super::*;

    // ---- prompting -------------------------------------------------------

    /// A command whose payload is a message and the images travelling with it.
    ///
    /// `docs/rpc.md` gives `images` to all four message-carrying commands, and the
    /// composer can send an attachment on any of them — a picture pasted while the
    /// agent is mid-turn steers, it does not wait. One builder rather than four
    /// copies, so a fifth command (or a change to the field name) cannot drift from
    /// the others.
    fn message_command(name: &str, message: impl Into<String>, images: &[ImageContent]) -> Value {
        let mut value = command(name);
        let object = value.as_object_mut().expect("command builds an object");
        object.insert("message".into(), Value::String(message.into()));
        if !images.is_empty() {
            object.insert(
                "images".into(),
                Value::Array(images.iter().map(ImageContent::to_value).collect()),
            );
        }
        value
    }

    /// Send a prompt. `streaming_behavior` is **required** while a turn is
    /// streaming: the engine fails the command if it is omitted.
    ///
    /// `images` ride in their own field — see [`ImageContent`] for why they are
    /// never folded into `message`.
    pub fn prompt(
        message: impl Into<String>,
        images: &[ImageContent],
        streaming_behavior: Option<&str>,
    ) -> Value {
        let mut value = message_command("prompt", message, images);
        if let Some(behavior) = streaming_behavior {
            value
                .as_object_mut()
                .expect("command builds an object")
                .insert("streamingBehavior".into(), Value::String(behavior.into()));
        }
        value
    }

    /// Inject a steering message (interrupt path).
    pub fn steer(message: impl Into<String>, images: &[ImageContent]) -> Value {
        message_command("steer", message, images)
    }

    /// Queue a follow-up message (post-turn path).
    pub fn follow_up(message: impl Into<String>, images: &[ImageContent]) -> Value {
        message_command("follow_up", message, images)
    }

    pub fn abort() -> Value {
        command("abort")
    }

    pub fn abort_and_prompt(message: impl Into<String>, images: &[ImageContent]) -> Value {
        message_command("abort_and_prompt", message, images)
    }

    pub fn new_session(parent_session: Option<&str>) -> Value {
        let mut value = command("new_session");
        if let Some(parent) = parent_session {
            value
                .as_object_mut()
                .expect("command builds an object")
                .insert("parentSession".into(), Value::String(parent.into()));
        }
        value
    }

    // ---- protocol / state ------------------------------------------------

    pub fn negotiate_protocol(protocol_version: u64) -> Value {
        let mut value = command(NEGOTIATE_PROTOCOL);
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("protocolVersion".into(), Value::from(protocol_version));
        value
    }

    pub fn get_state() -> Value {
        command("get_state")
    }

    pub fn get_available_commands() -> Value {
        command("get_available_commands")
    }

    pub fn set_fast_mode(enabled: bool) -> Value {
        let mut value = command("set_fast_mode");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("enabled".into(), Value::Bool(enabled));
        value
    }

    /// Replace the session todo list (useful to pre-seed a plan).
    pub fn set_todos(phases: Value) -> Value {
        let mut value = command("set_todos");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("phases".into(), phases);
        value
    }

    /// Register host-owned tools the engine may call back into.
    pub fn set_host_tools(tools: Value) -> Value {
        let mut value = command("set_host_tools");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("tools".into(), tools);
        value
    }

    /// Register host-owned URI schemes.
    pub fn set_host_uri_schemes(schemes: Value) -> Value {
        let mut value = command("set_host_uri_schemes");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("schemes".into(), schemes);
        value
    }

    pub fn set_subagent_subscription(level: &str) -> Value {
        let mut value = command("set_subagent_subscription");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("level".into(), Value::String(level.into()));
        value
    }

    pub fn get_subagents() -> Value {
        command("get_subagents")
    }

    pub fn get_subagent_messages(
        subagent_id: Option<&str>,
        session_file: Option<&str>,
        from_byte: Option<u64>,
    ) -> Value {
        let mut value = command("get_subagent_messages");
        let object = value.as_object_mut().expect("command builds an object");
        if let Some(id) = subagent_id {
            object.insert("subagentId".into(), Value::String(id.into()));
        }
        if let Some(file) = session_file {
            object.insert("sessionFile".into(), Value::String(file.into()));
        }
        if let Some(offset) = from_byte {
            object.insert("fromByte".into(), Value::from(offset));
        }
        value
    }

    // ---- model & thinking ------------------------------------------------

    pub fn set_model(provider: &str, model_id: &str) -> Value {
        let mut value = command("set_model");
        let object = value.as_object_mut().expect("command builds an object");
        object.insert("provider".into(), Value::String(provider.into()));
        object.insert("modelId".into(), Value::String(model_id.into()));
        value
    }

    pub fn cycle_model() -> Value {
        command("cycle_model")
    }

    pub fn get_available_models() -> Value {
        command("get_available_models")
    }

    pub fn set_thinking_level(level: &str) -> Value {
        let mut value = command("set_thinking_level");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("level".into(), Value::String(level.into()));
        value
    }

    pub fn cycle_thinking_level() -> Value {
        command("cycle_thinking_level")
    }

    // ---- queue policy ----------------------------------------------------

    pub fn set_steering_mode(mode: &str) -> Value {
        mode_command("set_steering_mode", mode)
    }

    pub fn set_follow_up_mode(mode: &str) -> Value {
        mode_command("set_follow_up_mode", mode)
    }

    pub fn set_interrupt_mode(mode: &str) -> Value {
        mode_command("set_interrupt_mode", mode)
    }

    fn mode_command(kind: &str, mode: &str) -> Value {
        let mut value = command(kind);
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("mode".into(), Value::String(mode.into()));
        value
    }

    // ---- compaction & retry ----------------------------------------------

    pub fn compact(custom_instructions: Option<&str>) -> Value {
        let mut value = command("compact");
        if let Some(instructions) = custom_instructions {
            value
                .as_object_mut()
                .expect("command builds an object")
                .insert(
                    "customInstructions".into(),
                    Value::String(instructions.into()),
                );
        }
        value
    }

    pub fn set_auto_compaction(enabled: bool) -> Value {
        flag_command("set_auto_compaction", enabled)
    }

    pub fn set_auto_retry(enabled: bool) -> Value {
        flag_command("set_auto_retry", enabled)
    }

    pub fn abort_retry() -> Value {
        command("abort_retry")
    }

    fn flag_command(kind: &str, enabled: bool) -> Value {
        let mut value = command(kind);
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("enabled".into(), Value::Bool(enabled));
        value
    }

    // ---- bash ------------------------------------------------------------

    pub fn bash(command_text: impl Into<String>) -> Value {
        let mut value = command("bash");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("command".into(), Value::String(command_text.into()));
        value
    }

    pub fn abort_bash() -> Value {
        command("abort_bash")
    }

    // ---- session ---------------------------------------------------------

    pub fn get_session_stats() -> Value {
        command("get_session_stats")
    }

    pub fn export_html(output_path: Option<&str>) -> Value {
        let mut value = command("export_html");
        if let Some(path) = output_path {
            value
                .as_object_mut()
                .expect("command builds an object")
                .insert("outputPath".into(), Value::String(path.into()));
        }
        value
    }

    pub fn switch_session(session_path: &str) -> Value {
        let mut value = command("switch_session");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("sessionPath".into(), Value::String(session_path.into()));
        value
    }

    pub fn branch(entry_id: &str) -> Value {
        let mut value = command("branch");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("entryId".into(), Value::String(entry_id.into()));
        value
    }

    pub fn get_branch_messages() -> Value {
        command("get_branch_messages")
    }

    pub fn get_last_assistant_text() -> Value {
        command("get_last_assistant_text")
    }

    pub fn set_session_name(name: &str) -> Value {
        let mut value = command("set_session_name");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("name".into(), Value::String(name.into()));
        value
    }

    pub fn handoff(custom_instructions: Option<&str>) -> Value {
        let mut value = command("handoff");
        if let Some(instructions) = custom_instructions {
            value
                .as_object_mut()
                .expect("command builds an object")
                .insert(
                    "customInstructions".into(),
                    Value::String(instructions.into()),
                );
        }
        value
    }

    // ---- messages --------------------------------------------------------

    pub fn get_messages() -> Value {
        command("get_messages")
    }

    /// Paged transcript read. The engine may answer `session_busy` or
    /// `stale_cursor`; retry after the turn settles rather than mixing
    /// snapshots.
    pub fn get_messages_page(cursor: Option<&str>, limit: Option<u32>) -> Value {
        let mut value = command("get_messages_page");
        let object = value.as_object_mut().expect("command builds an object");
        if let Some(cursor) = cursor {
            object.insert("cursor".into(), Value::String(cursor.into()));
        }
        if let Some(limit) = limit {
            object.insert("limit".into(), Value::from(limit));
        }
        value
    }

    // ---- login -----------------------------------------------------------

    pub fn get_login_providers() -> Value {
        command("get_login_providers")
    }

    pub fn login(provider_id: &str) -> Value {
        let mut value = command("login");
        value
            .as_object_mut()
            .expect("command builds an object")
            .insert("providerId".into(), Value::String(provider_id.into()));
        value
    }

    // ---- host-owned round trips ------------------------------------------

    /// Answer an `extension_ui_request` that expects a value.
    pub fn extension_ui_value(id: &str, value: impl Into<String>) -> Value {
        json!({ "type": "extension_ui_response", "id": id, "value": value.into() })
    }

    /// Answer an `extension_ui_request` that expects a confirmation.
    pub fn extension_ui_confirmed(id: &str, confirmed: bool) -> Value {
        json!({ "type": "extension_ui_response", "id": id, "confirmed": confirmed })
    }

    /// Answer an `extension_ui_request` by cancelling it.
    pub fn extension_ui_cancelled(id: &str, timed_out: bool) -> Value {
        json!({
            "type": "extension_ui_response",
            "id": id,
            "cancelled": true,
            "timedOut": timed_out,
        })
    }

    /// Stream progress for a host-owned tool call.
    pub fn host_tool_update(id: &str, partial_result: Value) -> Value {
        json!({ "type": "host_tool_update", "id": id, "partialResult": partial_result })
    }

    /// Complete a host-owned tool call. Set top-level `isError: true` to reject it.
    pub fn host_tool_result(id: &str, result: Value) -> Value {
        json!({ "type": "host_tool_result", "id": id, "result": result })
    }

    /// Complete a host-owned URI read.
    pub fn host_uri_content(
        id: &str,
        content: impl Into<String>,
        content_type: Option<&str>,
        immutable: bool,
    ) -> Value {
        let mut value = json!({
            "type": "host_uri_result",
            "id": id,
            "content": content.into(),
            "immutable": immutable,
        });
        if let Some(content_type) = content_type {
            value
                .as_object_mut()
                .expect("json! builds an object")
                .insert("contentType".into(), Value::String(content_type.into()));
        }
        value
    }

    /// Acknowledge a host-owned URI write.
    pub fn host_uri_write_ack(id: &str) -> Value {
        json!({ "type": "host_uri_result", "id": id })
    }

    /// Reject a host-owned URI request.
    pub fn host_uri_error(id: &str, error: impl Into<String>) -> Value {
        json!({ "type": "host_uri_result", "id": id, "isError": true, "error": error.into() })
    }
}

/// The extension-UI sub-protocol: the dialogs the engine waits on.
///
/// `extension_ui_request` is the one family of frames that is a *request* rather
/// than a notification, and a subset of it stops the run dead: `select`,
/// `confirm`, `input` and `editor` are answered by the host or the agent never
/// continues (`docs/02` §7). Everything else in the family — `notify`,
/// `setStatus`, `setWidget`, `setTitle`, `set_editor_text`, `open_url` — is
/// fire-and-forget chrome and deliberately decodes to `None` here: this module
/// answers "what must the host act on", not "what arrived".
///
/// The vocabulary sits beside its response constructors so no layer above has to
/// know the wire spelling of a dialog.
pub mod ui {
    use super::commands;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// A dialog method that blocks the run until the host answers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UiRequestKind {
        Select,
        Confirm,
        Input,
        Editor,
    }

    impl UiRequestKind {
        /// The blocking method named by `method`, or `None` if it is not one.
        pub fn from_method(method: &str) -> Option<Self> {
            match method {
                "select" => Some(Self::Select),
                "confirm" => Some(Self::Confirm),
                "input" => Some(Self::Input),
                "editor" => Some(Self::Editor),
                _ => None,
            }
        }

        /// The wire name, for diagnostics and the UI's own copy.
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Select => "select",
                Self::Confirm => "confirm",
                Self::Input => "input",
                Self::Editor => "editor",
            }
        }
    }

    /// A dialog the engine is waiting on.
    #[derive(Debug, Clone, PartialEq)]
    pub struct UiRequest {
        pub id: String,
        pub kind: UiRequestKind,
        pub title: String,
        /// The body of a `confirm`; empty for the other kinds.
        pub message: String,
        /// `select` labels, in presentation order.
        pub options: Vec<String>,
        /// The starting text of an `editor`; `None` for the other kinds.
        pub prefill: Option<String>,
        /// The hint shown inside an `input`'s empty field.
        pub placeholder: Option<String>,
        /// The engine's deadline for an answer, in milliseconds.
        ///
        /// Units are the engine's own: it arms `setTimeout(…, opts.timeout)`
        /// (`src/modes/rpc/rpc-mode.ts:757`), and its longest observed dialog is
        /// `600_000` — ten minutes. When it expires the engine resolves the dialog
        /// itself to a default and withdraws it, so the UI shows the deadline
        /// rather than deciding anything (`docs/12` §10).
        pub timeout_ms: Option<u64>,
    }

    /// What an `extension_ui_request` frame told the host to do.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Incoming {
        /// Present this dialog and answer it.
        Request(UiRequest),
        /// The engine is no longer waiting: it sent `method:"cancel"`.
        ///
        /// The referenced dialog must be dropped **without** being answered. A
        /// response to a withdrawn request is not a harmless race — the engine
        /// resolves the dialog itself (a timeout, or an abort) and a late answer
        /// would be matched against whatever replaced it.
        Withdrawn { target_id: String },
    }

    /// Decode one frame, if it asks the host for something.
    pub fn decode(frame: &Value) -> Option<Incoming> {
        if frame.get("type").and_then(Value::as_str) != Some("extension_ui_request") {
            return None;
        }

        let method = frame.get("method").and_then(Value::as_str)?;
        if method == "cancel" {
            return Some(Incoming::Withdrawn {
                target_id: frame.get("targetId").and_then(Value::as_str)?.to_string(),
            });
        }

        let kind = UiRequestKind::from_method(method)?;
        Some(Incoming::Request(UiRequest {
            id: frame.get("id").and_then(Value::as_str)?.to_string(),
            kind,
            title: text(frame, "title"),
            message: text(frame, "message"),
            options: frame
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            prefill: frame
                .get("prefill")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            placeholder: frame
                .get("placeholder")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            timeout_ms: frame.get("timeout").and_then(Value::as_u64),
        }))
    }

    fn text(frame: &Value, key: &str) -> String {
        frame
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    /// A fire-and-forget request from the engine: chrome the host draws, never answers.
    ///
    /// The mirror of [`Incoming`], and deliberately a separate type: a variant here that
    /// could hold a blocking dialog would invite a caller to treat one as chrome, which is
    /// exactly the confusion [`decode`]'s contract exists to prevent. The two decoders
    /// therefore never both fire on one frame.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "kebab-case")]
    pub enum ChromeOp {
        /// A toast, with the engine's own severity when it named one.
        Notify {
            message: String,
            level: Option<String>,
        },
        /// The engine's status slot. `None` clears it.
        Status {
            text: Option<String>,
        },
        /// The engine's widget lines. `None` clears them.
        Widget {
            lines: Option<Vec<String>>,
        },
        Title {
            title: String,
        },
        EditorText {
            text: String,
        },
        OpenUrl {
            url: String,
        },
    }

    /// Decode one frame, if it is fire-and-forget chrome.
    ///
    /// The wire names are the engine's own (`RpcExtensionUIContext`, `rpc-mode.ts:935-1010`):
    /// `notifyType`, `statusText`, `widgetLines`, `title`, `text`, `url`. Two things it does
    /// **not** ask for are dropped on the way through: the slot keys (`statusKey`,
    /// `widgetKey`), because the app draws one status line and one widget so a key would be a
    /// field no renderer can use, and `open_url`'s `launchUrl`/`instructions`, because the
    /// only thing anyone can do with the frame is open `url`.
    ///
    /// A `setWidget` whose `widgetLines` is absent (the engine omits `undefined`) and an
    /// `setStatus` whose `statusText` is absent both mean *clear*, which is why they stay
    /// `None` rather than collapsing to an empty string.
    pub fn chrome(frame: &Value) -> Option<ChromeOp> {
        if frame.get("type").and_then(Value::as_str) != Some("extension_ui_request") {
            return None;
        }

        match frame.get("method").and_then(Value::as_str)? {
            "notify" => Some(ChromeOp::Notify {
                message: text(frame, "message"),
                level: optional_text(frame, "notifyType"),
            }),
            "setStatus" => Some(ChromeOp::Status {
                text: optional_text(frame, "statusText"),
            }),
            "setWidget" => Some(ChromeOp::Widget {
                lines: frame
                    .get("widgetLines")
                    .and_then(Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect()
                    }),
            }),
            "setTitle" => Some(ChromeOp::Title {
                title: text(frame, "title"),
            }),
            "set_editor_text" => Some(ChromeOp::EditorText {
                text: text(frame, "text"),
            }),
            "open_url" => Some(ChromeOp::OpenUrl {
                url: text(frame, "url"),
            }),
            _ => None,
        }
    }

    fn optional_text(frame: &Value, key: &str) -> Option<String> {
        frame
            .get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }

    /// How the host answers a dialog.
    #[derive(Debug, Clone, PartialEq)]
    pub enum UiResponse {
        /// A chosen label (`select`) or entered text (`input`, `editor`).
        Value(String),
        /// A yes/no answer (`confirm`).
        Confirmed(bool),
        /// The host dismissed the dialog. `timed_out` distinguishes the engine's
        /// deadline expiring from the user closing it.
        Cancelled { timed_out: bool },
    }

    impl UiResponse {
        /// The frame answering `kind`, or `None` if the pairing is invalid.
        ///
        /// Pairing is checked here rather than trusted, because the two mistakes
        /// it catches — `confirmed` sent to a `select`, `value` sent to a
        /// `confirm` — both leave the engine waiting forever.
        pub fn frame(&self, kind: UiRequestKind, id: &str) -> Option<Value> {
            match (kind, self) {
                (_, Self::Cancelled { timed_out }) => {
                    Some(commands::extension_ui_cancelled(id, *timed_out))
                }
                (UiRequestKind::Confirm, Self::Confirmed(confirmed)) => {
                    Some(commands::extension_ui_confirmed(id, *confirmed))
                }
                (
                    UiRequestKind::Select | UiRequestKind::Input | UiRequestKind::Editor,
                    Self::Value(value),
                ) => Some(commands::extension_ui_value(id, value.clone())),
                _ => None,
            }
        }

        /// The wire field this answer uses, for diagnostics.
        pub fn name(&self) -> &'static str {
            match self {
                Self::Value(_) => "value",
                Self::Confirmed(_) => "confirmed",
                Self::Cancelled { .. } => "cancelled",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_frame_parses_and_reports_v2_support() {
        let raw = json!({
            "type": "ready",
            "protocolVersion": 1,
            "supportedProtocolVersions": [1, 2],
            "maxFrameBytes": 1048576,
            "maxReassembledFrameBytes": 67108864,
        });
        let ready = ReadyFrame::from_value(&raw).expect("parses");
        assert_eq!(ready.protocol_version, 1);
        assert!(ready.supports_v2());
        assert_eq!(ready.max_frame_bytes, 1_048_576);
    }

    #[test]
    fn ready_frame_rejects_non_ready_frames() {
        assert!(ReadyFrame::from_value(&json!({ "type": "response" })).is_none());
    }

    #[test]
    fn classify_routes_every_documented_frame_type() {
        assert_eq!(classify(&json!({ "type": "ready" })), FrameClass::Ready);
        assert_eq!(
            classify(&json!({ "type": "response" })),
            FrameClass::Response
        );
        assert_eq!(
            classify(&json!({ "type": "extension_ui_request" })),
            FrameClass::ExtensionUiRequest
        );
        assert_eq!(
            classify(&json!({ "type": "host_tool_call" })),
            FrameClass::HostToolCall
        );
        assert_eq!(
            classify(&json!({ "type": "available_commands_update" })),
            FrameClass::AvailableCommandsUpdate
        );
        assert_eq!(
            classify(&json!({ "type": "subagent_progress" })),
            FrameClass::Subagent
        );
        // Session events fall through to Event rather than Unknown.
        assert_eq!(
            classify(&json!({ "type": "agent_start" })),
            FrameClass::Event
        );
        assert_eq!(
            classify(&json!({ "type": "message_update" })),
            FrameClass::Event
        );
        assert_eq!(classify(&json!({})), FrameClass::Unknown);
    }

    #[test]
    fn an_image_rides_in_its_own_field_under_the_engines_key_names() {
        // `type`/`data`/`mimeType` are the engine's `ImageContent` contract. A host
        // that spelled it `mime_type` would send an attachment the engine ignores —
        // silently, since an unknown field is not an error.
        assert_eq!(
            commands::prompt("look", &[ImageContent::new("image/png", "AAAA")], None),
            json!({
                "type": "prompt",
                "message": "look",
                "images": [{ "type": "image", "data": "AAAA", "mimeType": "image/png" }],
            })
        );
    }

    #[test]
    fn images_keep_the_order_they_were_given() {
        // The engine numbers a message's attachments `[Image #N]` in this order, so
        // a reordering here would attach the wrong picture to the wrong sentence.
        let value = commands::prompt(
            "compare",
            &[
                ImageContent::new("image/png", "FIRST"),
                ImageContent::new("image/webp", "SECOND"),
            ],
            None,
        );

        assert_eq!(value["images"][0]["data"], "FIRST");
        assert_eq!(value["images"][1]["data"], "SECOND");
    }

    #[test]
    fn every_message_command_carries_images_the_same_way() {
        // `docs/rpc.md` gives `images` to all four of these. An attachment pasted
        // while the agent is mid-turn rides on `steer`, so a builder that dropped it
        // would send the words without the picture and nothing would say so.
        let images = [ImageContent::new("image/webp", "BYTES")];
        let wire = json!([{ "type": "image", "data": "BYTES", "mimeType": "image/webp" }]);

        for value in [
            commands::prompt("look", &images, None),
            commands::steer("look", &images),
            commands::follow_up("look", &images),
            commands::abort_and_prompt("look", &images),
        ] {
            assert_eq!(value["images"], wire, "in {value}");
            assert_eq!(value["message"], "look", "in {value}");
        }
    }

    #[test]
    fn commands_carry_only_the_fields_they_need() {
        assert_eq!(commands::get_state(), json!({ "type": "get_state" }));
        assert_eq!(
            commands::set_model("anthropic", "claude-sonnet-4-5"),
            json!({ "type": "set_model", "provider": "anthropic", "modelId": "claude-sonnet-4-5" })
        );
        // streamingBehavior is omitted when not supplied, since the engine
        // rejects an explicit null.
        assert_eq!(
            commands::prompt("hi", &[], None),
            json!({ "type": "prompt", "message": "hi" })
        );
        assert_eq!(
            commands::prompt("hi", &[], Some("steer")),
            json!({ "type": "prompt", "message": "hi", "streamingBehavior": "steer" })
        );
        assert_eq!(
            commands::negotiate_protocol(PROTOCOL_VERSION_V2),
            json!({ "type": "negotiate_protocol", "protocolVersion": 2 })
        );
        // No attachment, no `images` key — not an empty array. `docs/rpc.md` types it
        // as an array, and a turn with nothing attached is the common case.
        assert_eq!(
            commands::steer("hi", &[]),
            json!({ "type": "steer", "message": "hi" })
        );
        assert_eq!(
            commands::follow_up("hi", &[]),
            json!({ "type": "follow_up", "message": "hi" })
        );
        assert_eq!(
            commands::abort_and_prompt("hi", &[]),
            json!({ "type": "abort_and_prompt", "message": "hi" })
        );
    }

    /// The frame is the one captured from a live `bash` call (`docs/02` §2), so
    /// the fixture is evidence rather than a guess at the shape.
    #[test]
    fn an_approval_select_decodes_into_a_dialog() {
        let frame = json!({
            "type": "extension_ui_request",
            "id": "158654c2b54fed09",
            "method": "select",
            "options": ["Approve", "Deny"],
            "title": "Allow tool: bash\nCommand: echo hello-from-live-test",
        });

        let Some(ui::Incoming::Request(request)) = ui::decode(&frame) else {
            panic!("an approval select must decode to a request");
        };
        assert_eq!(request.kind, ui::UiRequestKind::Select);
        assert_eq!(request.options, ["Approve", "Deny"]);
        assert!(request.title.contains("echo hello-from-live-test"));
        assert_eq!(request.timeout_ms, None);
    }

    /// The distinction this module exists for: only four methods stop the run, and
    /// an extension pushing a widget at startup must not look like a stall.
    #[test]
    fn fire_and_forget_methods_are_not_host_obligations() {
        for method in [
            "notify",
            "setStatus",
            "setWidget",
            "setTitle",
            "set_editor_text",
            "open_url",
        ] {
            let frame = json!({ "type": "extension_ui_request", "id": "x", "method": method });
            assert_eq!(
                ui::decode(&frame),
                None,
                "{method} must not ask the host for anything"
            );
        }
    }

    /// The chrome half of the vocabulary, with the engine's own field names
    /// (`docs/02` §7: `notify` is "message + type").
    #[test]
    fn a_notify_frame_decodes_as_chrome_with_the_engines_severity() {
        let frame = json!({
            "type": "extension_ui_request",
            "id": "ui_9",
            "method": "notify",
            "message": "Logged in as gioviale",
            "notifyType": "info",
        });

        assert_eq!(
            ui::chrome(&frame),
            Some(ui::ChromeOp::Notify {
                message: "Logged in as gioviale".to_string(),
                level: Some("info".to_string()),
            })
        );
        assert_eq!(ui::decode(&frame), None, "chrome is not a host obligation");
    }

    /// Two of the six clear their slot by omitting the value, so an absent field is a
    /// deliberate `None` rather than an empty string a renderer would draw.
    #[test]
    fn a_cleared_status_and_widget_keep_their_absence() {
        let status = json!({ "type": "extension_ui_request", "id": "s", "method": "setStatus", "statusKey": "build", "statusText": null });
        let widget = json!({ "type": "extension_ui_request", "id": "w", "method": "setWidget", "widgetKey": "autoresearch" });

        assert_eq!(
            ui::chrome(&status),
            Some(ui::ChromeOp::Status { text: None })
        );
        assert_eq!(
            ui::chrome(&widget),
            Some(ui::ChromeOp::Widget { lines: None })
        );

        let lines = json!({
            "type": "extension_ui_request",
            "id": "w2",
            "method": "setWidget",
            "widgetKey": "autoresearch",
            "widgetLines": ["researching 3/7"],
        });
        assert_eq!(
            ui::chrome(&lines),
            Some(ui::ChromeOp::Widget {
                lines: Some(vec!["researching 3/7".to_string()]),
            })
        );
    }

    /// The three remaining chrome kinds, and the one frame that is neither: an extension
    /// error is not an `extension_ui_request` at all.
    #[test]
    fn the_rest_of_the_chrome_vocabulary_decodes() {
        let cases = [
            (
                json!({ "type": "extension_ui_request", "id": "t", "method": "setTitle", "title": "omp — builder" }),
                ui::ChromeOp::Title {
                    title: "omp — builder".to_string(),
                },
            ),
            (
                json!({ "type": "extension_ui_request", "id": "e", "method": "set_editor_text", "text": "/review" }),
                ui::ChromeOp::EditorText {
                    text: "/review".to_string(),
                },
            ),
            (
                json!({ "type": "extension_ui_request", "id": "o", "method": "open_url", "url": "https://example.com/auth", "launchUrl": "https://example.com/x" }),
                ui::ChromeOp::OpenUrl {
                    url: "https://example.com/auth".to_string(),
                },
            ),
        ];

        for (frame, expected) in cases {
            assert_eq!(ui::chrome(&frame), Some(expected));
        }

        assert_eq!(
            ui::chrome(&json!({ "type": "extension_error", "method": "notify" })),
            None
        );
        assert_eq!(
            ui::chrome(
                &json!({ "type": "extension_ui_request", "id": "u", "method": "unknown_future" })
            ),
            None
        );
    }

    /// The one invariant that keeps the two halves apart: a well-formed frame is a dialog the
    /// host must answer or chrome it only draws, never both — the four blocking kinds must
    /// not have grown a chrome spelling (`docs/12` §13).
    #[test]
    fn the_two_decoders_never_both_fire() {
        for frame in [
            json!({ "type": "extension_ui_request", "id": "x", "method": "select", "title": "Approve" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "confirm", "title": "Sure?" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "input", "title": "Name" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "editor", "title": "Text" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "cancel", "targetId": "x" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "notify", "message": "hi" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "setStatus", "statusText": "ok" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "setWidget", "widgetLines": ["a"] }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "setTitle", "title": "t" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "set_editor_text", "text": "t" }),
            json!({ "type": "extension_ui_request", "id": "x", "method": "open_url", "url": "https://x" }),
        ] {
            let method = frame["method"].as_str().unwrap_or_default();

            assert_ne!(
                ui::decode(&frame).is_some(),
                ui::chrome(&frame).is_some(),
                "{method} must decode as exactly one of the two halves"
            );
        }

        // A frame neither half can use stays undecoded: a blocking method with no usable id
        // is the case the pump reports rather than drops in silence (`docs/14` §7 #6).
        let unusable = json!({ "type": "extension_ui_request", "method": "select", "title": "?" });
        assert_eq!(ui::decode(&unusable), None);
        assert_eq!(ui::chrome(&unusable), None);
    }

    #[test]
    fn a_cancel_withdraws_its_target() {
        let frame = json!({
            "type": "extension_ui_request",
            "id": "later",
            "method": "cancel",
            "targetId": "158654c2b54fed09",
        });

        assert_eq!(
            ui::decode(&frame),
            Some(ui::Incoming::Withdrawn {
                target_id: "158654c2b54fed09".to_string(),
            })
        );
    }

    /// A mismatched answer leaves the engine waiting forever, so the pairing is
    /// refused rather than written.
    #[test]
    fn an_answer_only_fits_the_kind_it_belongs_to() {
        let select = ui::UiRequestKind::Select;
        let confirm = ui::UiRequestKind::Confirm;

        assert!(ui::UiResponse::Value("Approve".into())
            .frame(select, "id")
            .is_some());
        assert!(ui::UiResponse::Confirmed(true)
            .frame(confirm, "id")
            .is_some());
        assert_eq!(ui::UiResponse::Confirmed(true).frame(select, "id"), None);
        assert_eq!(
            ui::UiResponse::Value("Approve".into()).frame(confirm, "id"),
            None
        );

        // Dismissing is legal for every kind.
        for kind in [select, confirm] {
            let cancelled = ui::UiResponse::Cancelled { timed_out: false };
            let frame = cancelled.frame(kind, "id").expect("cancel fits");
            assert_eq!(frame["cancelled"], json!(true));
            assert_eq!(frame["timedOut"], json!(false));
        }
    }

    #[test]
    fn a_dialog_deadline_decodes_as_milliseconds() {
        let frame = json!({
            "type": "extension_ui_request",
            "id": "ui_7",
            "method": "input",
            "title": "Branch name",
            "placeholder": "feature/",
            "timeout": 600_000,
        });

        let Some(ui::Incoming::Request(request)) = ui::decode(&frame) else {
            panic!("an input dialog must decode to a request");
        };
        assert_eq!(request.timeout_ms, Some(600_000));
        assert_eq!(request.kind, ui::UiRequestKind::Input);
        assert_eq!(request.placeholder.as_deref(), Some("feature/"));
    }
}
