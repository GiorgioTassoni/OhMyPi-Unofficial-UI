//! The palette's two frames: the commands the engine advertises, and a builtin
//! command's output.
//!
//! These ride the *frame* stream rather than the session-event stream (`docs/rpc.md`,
//! "Outbound frame categories" §8 and §11), which is why they are not in [`crate::events`]:
//! that module decodes `AgentSessionEvent`, and these are the engine's builtin
//! slash-command side channels. Both are classified in [`crate::protocol::classify`];
//! this module is what turns them into something a caller can use.
//!
//! # Measured at v18.2.6
//!
//! * `get_available_commands` answers `{ data: { commands: [...] } }` — 45 entries here:
//!   `builtin` 41, `custom` 2, `extension` 1, `file` 1. The keys present are `name`,
//!   `source`, `aliases`, `description`, `input` and `subcommands`; **`input` carries
//!   exactly one field, `hint`** (no `required` flag to branch on).
//! * `available_commands_update` pushes the same array, but it is emitted **during
//!   startup** — before a subscriber can attach to the frame stream — so a palette has to
//!   *fetch* its list and treat the push as a refresh, not as the first source.
//! * `command_output` is `{ "type": "command_output", "text": "…" }`: the whole payload is
//!   one string. `/model` produces one and answers `agentInvoked: false`; `/help` produces
//!   neither, because its output is the TUI's own screen — so a palette may offer a
//!   command whose result never reaches this channel.
//!
//! # Rules
//!
//! The ones from [`crate::events`] hold here: decoding never panics, open string unions
//! stay strings, and only what we branch on is typed.

use serde_json::Value;

/// One command the engine advertises to a palette.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdvertisedCommand {
    pub name: String,
    /// `builtin`, `custom`, `extension` or `file` — open, so a source this build has not
    /// seen groups under its own name rather than failing to decode.
    pub source: String,
    /// Other names the same command answers to.
    ///
    /// For **matching only**, never for insertion: `model` answers to `models`, and
    /// `force`'s alias is literally `force:`, so putting an alias in the message would
    /// send something the engine may not expand.
    pub aliases: Vec<String>,
    pub description: Option<String>,
    /// What the command wants after its name (`input.hint`).
    ///
    /// Also the palette's signal that this one must not be dispatched on a single
    /// keypress: the user has something to type before it means anything.
    pub hint: Option<String>,
    pub subcommands: Vec<AdvertisedSubcommand>,
}

/// A second level under a command that has one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdvertisedSubcommand {
    pub name: String,
    pub description: Option<String>,
}

impl AdvertisedCommand {
    /// Whether anything has to follow the name — the difference between "this runs when
    /// you press Enter" and "finish the sentence first".
    pub fn takes_input(&self) -> bool {
        self.hint.is_some() || !self.subcommands.is_empty()
    }
}

/// Read the command array out of whichever frame carried it.
///
/// The two sources differ only in nesting — `{ data: { commands } }` on the response,
/// `{ commands }` on the update — so one reader serves both, and a caller never has to
/// know which it is holding.
pub fn commands_from(value: &Value) -> Vec<AdvertisedCommand> {
    let list = value
        .get("commands")
        .and_then(Value::as_array)
        .or_else(|| value.get("data")?.get("commands")?.as_array());

    list.map(|entries| {
        entries
            .iter()
            .filter_map(command_from)
            .collect::<Vec<AdvertisedCommand>>()
    })
    .unwrap_or_default()
}

/// One entry, or `None` for anything that is not one.
///
/// An entry without a `name` is not a command — the palette keys on the name, and
/// inventing one would offer the user something that cannot be dispatched. One bad entry
/// is dropped and the rest of the list survives.
fn command_from(value: &Value) -> Option<AdvertisedCommand> {
    let name = value.get("name").and_then(Value::as_str)?;

    Some(AdvertisedCommand {
        name: name.to_string(),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        aliases: strings_at(value, "aliases"),
        description: string_at(value, "description"),
        hint: value
            .get("input")
            .and_then(|input| string_at(input, "hint")),
        subcommands: value
            .get("subcommands")
            .and_then(Value::as_array)
            .map(|entries| entries.iter().filter_map(subcommand_from).collect())
            .unwrap_or_default(),
    })
}

/// A subcommand entry. `{ name, description? }`, and the name is the same contract.
fn subcommand_from(value: &Value) -> Option<AdvertisedSubcommand> {
    Some(AdvertisedSubcommand {
        name: value.get("name").and_then(Value::as_str)?.to_string(),
        description: string_at(value, "description"),
    })
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn strings_at(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A builtin command's output, when the frame is one.
///
/// The payload is a single string, so this returns a borrowed slice of it rather than a
/// struct with one field. A frame with no text is not output — rendering an empty notice
/// row would claim the command said something.
pub fn command_output(value: &Value) -> Option<&str> {
    let text = value.get("text")?.as_str()?;

    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Real entries, copied out of a live `get_available_commands` on v18.2.6.
    fn model_entry() -> Value {
        json!({
            "aliases": ["models"],
            "description": "Show current model selection",
            "name": "model",
            "source": "builtin"
        })
    }

    fn security_entry() -> Value {
        json!({
            "description": "Plan, run, inspect, import, and compare OMP-native security scans",
            "input": { "hint": "<plan|scan|status|cancel|scans|show|import|export|validate|compare|disposition>" },
            "name": "security",
            "source": "builtin",
            "subcommands": [
                { "description": "Create an immutable security scan plan", "name": "plan" },
                { "description": "Start a planned or newly planned native scan", "name": "scan" }
            ]
        })
    }

    #[test]
    fn a_real_entry_keeps_its_wire_names() {
        let parsed = commands_from(&json!({ "data": { "commands": [model_entry()] } }));

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "model");
        assert_eq!(parsed[0].source, "builtin");
        assert_eq!(parsed[0].aliases, vec!["models".to_string()]);
        assert_eq!(
            parsed[0].description.as_deref(),
            Some("Show current model selection")
        );
    }

    #[test]
    fn a_command_that_wants_arguments_says_so() {
        // The palette's one dispatch rule: a hint or a subcommand means the name alone
        // is not the command yet.
        let parsed = commands_from(&json!({ "data": { "commands": [security_entry()] } }));

        assert!(parsed[0].takes_input());
        assert_eq!(
            parsed[0].hint.as_deref(),
            Some("<plan|scan|status|cancel|scans|show|import|export|validate|compare|disposition>")
        );
        assert_eq!(parsed[0].subcommands.len(), 2);
        assert_eq!(parsed[0].subcommands[0].name, "plan");
        assert_eq!(
            parsed[0].subcommands[0].description.as_deref(),
            Some("Create an immutable security scan plan")
        );

        let plain = commands_from(&json!({ "data": { "commands": [model_entry()] } }));
        assert!(
            !plain[0].takes_input(),
            "a hintless command runs as it stands"
        );
    }

    #[test]
    fn the_response_and_the_push_read_the_same_way() {
        // The list arrives two ways — asked for, and pushed when metadata changes. A
        // reader that only understood one would leave the palette empty on the other.
        let asked = json!({ "command": "get_available_commands", "success": true, "data": { "commands": [model_entry()] } });
        let pushed = json!({ "type": "available_commands_update", "commands": [model_entry()] });

        assert_eq!(commands_from(&asked), commands_from(&pushed));
        assert_eq!(commands_from(&asked).len(), 1);
    }

    #[test]
    fn a_sparse_entry_is_a_command_and_a_nameless_one_is_not() {
        // The engine adds fields between releases, so an entry with only the two fields
        // the palette truly needs must survive; and one without a name cannot be offered,
        // because there would be nothing to dispatch.
        let parsed = commands_from(&json!({
            "data": { "commands": [
                { "name": "compact", "source": "builtin" },
                { "source": "builtin", "description": "no name" },
                { "name": "init", "source": "file" },
            ] }
        }));

        assert_eq!(
            parsed
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["compact", "init"]
        );
        assert_eq!(parsed[0].aliases, Vec::<String>::new());
        assert_eq!(parsed[0].hint, None);
        assert!(!parsed[0].takes_input());
    }

    #[test]
    fn an_unknown_source_is_kept_rather_than_dropped() {
        // Open union: a fourth source from a newer engine groups under its own name
        // instead of rendering as "unknown" on every row.
        let parsed = commands_from(&json!({
            "data": { "commands": [{ "name": "future", "source": "workspace" }] }
        }));

        assert_eq!(parsed[0].source, "workspace");
    }

    #[test]
    fn a_frame_with_no_commands_reads_as_an_empty_list() {
        // Not an error: the palette is empty, and a decode failure here would be a
        // crash on a legitimate answer.
        assert!(commands_from(&json!({})).is_empty());
        assert!(commands_from(&json!({ "data": {} })).is_empty());
        assert!(commands_from(&json!({ "data": { "commands": "soon" } })).is_empty());
    }

    #[test]
    fn command_output_is_its_text() {
        // The whole payload, measured: one string.
        assert_eq!(
            command_output(&json!({
                "text": "Current model: commandcode/deepseek/deepseek-v4.1-flash",
                "type": "command_output"
            })),
            Some("Current model: commandcode/deepseek/deepseek-v4.1-flash")
        );

        // An empty one is not output: a notice row saying nothing is worse than no row.
        assert_eq!(
            command_output(&json!({ "text": "", "type": "command_output" })),
            None
        );
        assert_eq!(command_output(&json!({ "type": "command_output" })), None);
        assert_eq!(command_output(&json!({ "type": "response" })), None);
    }
}
