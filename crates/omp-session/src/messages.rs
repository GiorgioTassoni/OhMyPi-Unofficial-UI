//! The domain view of a conversation message.
//!
//! The engine's `AgentMessage` carries far more than a desktop app renders:
//! provider replay payloads, signature blobs, per-feature capability flags, cost
//! accounting. This module models the **envelope and the renderable blocks** and
//! keeps the original frame, so nothing is lost while nothing unsupported breaks
//! rendering.
//!
//! # Rules
//!
//! 1. **Decoding never fails** — the same contract [`SessionEvent`] follows. A
//!    message is engine-authored history; if one field is shaped unexpectedly,
//!    the transcript must still render the rest rather than lose a whole turn.
//! 2. **Known blocks are typed, unknown blocks are preserved.** The renderable
//!    kinds (`text`, `thinking`, `image`, `toolCall`) are modelled; the dozen
//!    provider-internal kinds (`anthropicServerTool`, `openaiResponsesHistory`,
//!    `anthropicCompaction`, `redactedThinking`, …) become
//!    [`ContentBlock::Other`] carrying their raw JSON.
//! 3. **`raw` is retained.** Fields the app does not model — `retryRecovery`,
//!    `providerPayload`, `inputTransformations` — stay reachable for surfaces
//!    that need them, without this module having to mirror the upstream union.
//!
//! Naming follows [`omp_transport::events`]: a field is the wire name,
//! mechanically snake_cased, and open string unions stay strings.
//!
//! [`SessionEvent`]: omp_transport::SessionEvent

use serde_json::Value;

/// Who authored a message, with the role-specific fields an app branches on.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageKind {
    /// A user prompt. `synthetic` marks engine-injected continuations, which are
    /// not shown as if the user typed them.
    User { synthetic: bool, steering: bool },
    /// A developer-role message: engine-authored prompts that read as operator
    /// input (`.`, `c` continue shortcuts).
    Developer { synthetic: bool },
    /// An assistant turn. `stop_reason` and `error_message` decide whether the
    /// row renders as an answer or as a failure.
    Assistant {
        /// Open string union: `stop`, `length`, `toolUse`, `error`, `aborted`.
        stop_reason: Option<String>,
        /// Present when the turn failed; the UI must show it rather than an
        /// empty answer.
        error_message: Option<String>,
        model: Option<String>,
        provider: Option<String>,
        /// Token accounting for this turn, passed through opaquely.
        usage: Option<Value>,
    },
    /// The result of a tool call, correlated by `tool_call_id`.
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        is_error: bool,
    },
    /// A role this build does not model — session extensions add their own
    /// (for example hidden rewind reports).
    Other { role: String },
}

/// One block inside a message.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    /// An image attachment, with its bytes.
    ///
    /// Typed rather than kept opaque because a desktop view renders the image
    /// itself (`docs/12` §3.1 shows attachments as thumbnails): the engine's
    /// `ImageContent` carries the payload inline, so there is no reference for a
    /// host to resolve. `raw` is not retained — for an image it is the same
    /// base64 twice, and a duplicate of a screenshot is real memory.
    ///
    /// A block that does not carry both fields is not this variant: it decodes as
    /// [`ContentBlock::Other`] and stays reachable verbatim.
    Image {
        mime_type: String,
        data: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
        /// The harness-level intent the model attached, when it gave one.
        intent: Option<String>,
    },
    /// A block kind this build does not model, preserved verbatim.
    Other {
        kind: String,
        raw: Value,
    },
}

/// A message, decoded as far as this build understands it.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub kind: MessageKind,
    /// Unix milliseconds, as stamped by the engine. Optional because a message
    /// whose shape we half-recognise is still worth showing.
    pub timestamp: Option<u64>,
    /// The message's content blocks, in order. A bare-string `content` becomes a
    /// single [`ContentBlock::Text`], so renderers see one shape.
    pub blocks: Vec<ContentBlock>,
    /// The engine's own type for a message the app did not write, when it has one.
    ///
    /// `async-result` is the one that matters here: a background job's result arrives as a
    /// message of its own rather than as the call's answer, so this is how a reader tells a
    /// delivered job apart from something a model said (`docs/12` §9).
    pub custom_type: Option<String>,
    /// The jobs a delivery accounts for. Empty for every other message.
    pub jobs: Vec<JobDelivery>,
    /// The frame this was decoded from, unmodified.
    pub raw: Value,
}

/// One job a delivery message reports on.
///
/// The engine writes these into the message's `details.jobs` (`async-job-delivery.ts`), and
/// the `jobId` is the same string the tool card that started the job carries in its
/// `details.async.jobId` — which is the whole point: it is the join that lets a job row close
/// itself when its result lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDelivery {
    pub job_id: String,
    /// `bash` | `eval` | `task`, as the engine labels the job.
    pub kind: String,
    pub duration_ms: u64,
    pub label: Option<String>,
}

/// Decode the deliveries a message carries, tolerantly.
fn decode_job_deliveries(raw: &Value) -> Vec<JobDelivery> {
    let Some(jobs) = raw.get("details").and_then(|details| details.get("jobs")) else {
        return Vec::new();
    };

    jobs.as_array()
        .map(|jobs| {
            jobs.iter()
                .filter_map(|job| {
                    Some(JobDelivery {
                        job_id: job.get("jobId").and_then(Value::as_str)?.to_string(),
                        kind: job
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        duration_ms: job
                            .get("durationMs")
                            .and_then(Value::as_u64)
                            .unwrap_or_default(),
                        label: job.get("label").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Message {
    /// Decode a message frame. Never fails; unrecognised parts are preserved.
    pub fn decode(raw: &Value) -> Self {
        let role = raw.get("role").and_then(Value::as_str).unwrap_or_default();
        let kind = decode_kind(role, raw);

        Self {
            kind,
            timestamp: raw.get("timestamp").and_then(Value::as_u64),
            blocks: decode_blocks(raw.get("content")),
            custom_type: raw
                .get("customType")
                .and_then(Value::as_str)
                .map(str::to_string),
            jobs: decode_job_deliveries(raw),
            raw: raw.clone(),
        }
    }

    /// The wire role string.
    pub fn role(&self) -> &str {
        match &self.kind {
            MessageKind::User { .. } => "user",
            MessageKind::Developer { .. } => "developer",
            MessageKind::Assistant { .. } => "assistant",
            MessageKind::ToolResult { .. } => "toolResult",
            MessageKind::Other { role } => role,
        }
    }

    /// The message's text, concatenated across its text blocks.
    ///
    /// For previews, search indexing and thread titles. Thinking is excluded on
    /// purpose: it is not part of what the assistant said.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The message's reasoning, concatenated across its thinking blocks.
    ///
    /// Deliberately separate from [`Self::text`]: `docs/12` §3.1 renders thinking
    /// collapsed and tinted, so a renderer forced to split it back out of the text
    /// would be guessing where the model stopped thinking.
    pub fn thinking(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Thinking { thinking } => Some(thinking.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Every image attached to this message, in order.
    ///
    /// `docs/12` §3.1 shows a user message's attachments as thumbnails. The bytes
    /// are inline on the block, so a caller renders them directly.
    pub fn images(&self) -> impl Iterator<Item = (&str, &str)> {
        self.blocks.iter().filter_map(|block| match block {
            ContentBlock::Image { mime_type, data } => Some((mime_type.as_str(), data.as_str())),
            _ => None,
        })
    }

    /// Every tool call this message requested, in order.
    ///
    /// A restored assistant turn is the only place a tool call is named; the
    /// result arrives later as a separate message, which is why a card is built
    /// from both.
    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.blocks.iter().filter_map(|block| match block {
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => Some((id.as_str(), name.as_str(), arguments)),
            _ => None,
        })
    }
}

fn decode_kind(role: &str, raw: &Value) -> MessageKind {
    let flag = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(false);
    let text = |key: &str| {
        raw.get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    };

    match role {
        "user" => MessageKind::User {
            synthetic: flag("synthetic"),
            steering: flag("steering"),
        },
        "developer" => MessageKind::Developer {
            synthetic: flag("synthetic"),
        },
        "assistant" => MessageKind::Assistant {
            stop_reason: text("stopReason"),
            error_message: text("errorMessage"),
            model: text("model"),
            provider: text("provider"),
            usage: raw.get("usage").cloned(),
        },
        "toolResult" => MessageKind::ToolResult {
            tool_call_id: text("toolCallId").unwrap_or_default(),
            tool_name: text("toolName").unwrap_or_default(),
            is_error: flag("isError"),
        },
        _ => MessageKind::Other {
            role: role.to_string(),
        },
    }
}

fn decode_blocks(content: Option<&Value>) -> Vec<ContentBlock> {
    match content {
        // A user turn may carry a bare string rather than blocks.
        Some(Value::String(text)) => vec![ContentBlock::Text { text: text.clone() }],
        Some(Value::Array(items)) => items.iter().map(ContentBlock::decode).collect(),
        _ => Vec::new(),
    }
}

impl ContentBlock {
    /// Decode one content block. Never fails: an unmodelled kind is preserved.
    ///
    /// Public because a streaming toolcall arrives as a bare block inside a
    /// delta rather than as part of a message (`MessageDelta::ToolCallEnd`).
    pub fn decode(raw: &Value) -> Self {
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or_default();
        let string = |key: &str| {
            raw.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };

        match kind {
            "text" => ContentBlock::Text {
                text: string("text"),
            },
            "thinking" => ContentBlock::Thinking {
                thinking: string("thinking"),
            },
            // Both fields or nothing: a block missing its bytes is preserved
            // verbatim rather than shown as a broken thumbnail.
            "image" => match image_payload(raw) {
                Some((mime_type, data)) => ContentBlock::Image { mime_type, data },
                None => ContentBlock::Other {
                    kind: kind.to_string(),
                    raw: raw.clone(),
                },
            },
            "toolCall" => ContentBlock::ToolCall {
                id: string("id"),
                name: string("name"),
                arguments: raw.get("arguments").cloned().unwrap_or(Value::Null),
                intent: raw
                    .get("intent")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            _ => ContentBlock::Other {
                kind: kind.to_string(),
                raw: raw.clone(),
            },
        }
    }
}

/// An image block's `mimeType` and base64 `data`, when it carries both.
///
/// The engine's `ImageContent` names these two fields directly; anything else
/// shaped like an image is left to [`ContentBlock::Other`].
fn image_payload(raw: &Value) -> Option<(String, String)> {
    let mime_type = raw.get("mimeType").and_then(Value::as_str)?;
    let data = raw.get("data").and_then(Value::as_str)?;

    (!data.is_empty()).then(|| (mime_type.to_string(), data.to_string()))
}

/// The displayable payload of a finished tool call.
///
/// Both paths into a card — the live `tool_execution_end` event and a restored
/// `toolResult` message — carry the engine's `content` and `details`, so they
/// normalise onto this one shape instead of the renderer branching on origin.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultContent {
    /// The tool's displayable output, passed through opaquely.
    pub content: Value,
    /// The tool-specific structured payload the engine attached.
    pub details: Option<Value>,
    pub is_error: bool,
}

impl ToolResultContent {
    /// The result as displayable text.
    ///
    /// `content` is a string for some tools and a list of content blocks for
    /// others, so callers get one shape. Blocks that carry no `text` (an image,
    /// say) contribute nothing: inventing a placeholder here would put it in
    /// search results and thread titles too, and a renderer that wants one can
    /// still see the block itself.
    pub fn text(&self) -> String {
        match &self.content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A delivered background job, shaped as the engine writes it (`async-job-delivery.ts`):
    /// a `custom` message carrying `customType: "async-result"` and the jobs it accounts for
    /// in `details.jobs`.
    fn delivery() -> Value {
        json!({
            "role": "custom",
            "customType": "async-result",
            "display": true,
            "attribution": "agent",
            "content": "<system-notice>\nBackground job bg_2 has completed. …",
            "details": {
                "meta": { "source": { "type": "report", "value": "background job delivery" } },
                "jobs": [
                    { "jobId": "bg_2", "type": "bash", "durationMs": 25_004 }
                ]
            },
            "timestamp": 1_789_909_000_000u64
        })
    }

    #[test]
    fn a_job_delivery_carries_its_type_and_its_job_ids() {
        let message = Message::decode(&delivery());

        assert_eq!(message.custom_type.as_deref(), Some("async-result"));
        assert_eq!(message.jobs.len(), 1);
        assert_eq!(message.jobs[0].job_id, "bg_2");
        assert_eq!(message.jobs[0].kind, "bash");
        assert_eq!(message.jobs[0].duration_ms, 25_004);
        assert_eq!(message.jobs[0].label, None);
    }

    /// Every other message says it accounts for nothing, which is what makes a non-empty
    /// job list and the delivery type the same claim.
    #[test]
    fn an_ordinary_message_accounts_for_no_jobs() {
        let message = Message::decode(&json!({
            "role": "assistant",
            "content": "done",
            "timestamp": 1
        }));

        assert_eq!(message.custom_type, None);
        assert!(message.jobs.is_empty());
    }
}
