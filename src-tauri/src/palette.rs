//! Desktop built-in commands and palette augmentations.

use omp_session::Transcript;
use omp_transport::palette::AdvertisedCommand;

pub const HELP_TEXT: &str = r#"Available Commands:
  /tools              List available tools and capabilities
  /context            Show context window usage and breakdown
  /model              Display the current model selection
  /compact [focus]    Compact conversation context (options: soft, remote, snapcompact)
  /fast [on|off]      Toggle or inspect fast response mode
  /usage              Display token usage and costs
  /stats              Display session and performance statistics
  /browser            Configure or inspect browser automation (headless|visible)
  /plugins            List, enable, or disable plugins
  /advisor            Inspect or configure advisor model settings
  /todo               Manage session tasks and todo items
  /mcp                Manage MCP servers and tools
  /ssh                Configure and manage SSH connections
  /memory             View and manage persistent memory models
  /rename [title]     Rename session (generates auto-title if blank)
  /clear              Clear conversation transcript
  /help               Show this help message

Keyboard Shortcuts:
  Enter               Run selected command / send message
  Tab                 Complete command or subcommand into composer
  ↑ / ↓               Navigate command palette
  Esc                 Close command palette / clear composer"#;

/// Augment the advertised command list with desktop-owned builtins.
pub fn with_desktop_builtins(mut list: Vec<AdvertisedCommand>) -> Vec<AdvertisedCommand> {
    if !list.iter().any(|c| c.name == "help") {
        list.push(AdvertisedCommand {
            name: "help".to_string(),
            source: "builtin".to_string(),
            aliases: vec!["?".to_string()],
            description: Some("Show available commands, options, and shortcuts".to_string()),
            hint: None,
            subcommands: Vec::new(),
        });
    }
    if !list.iter().any(|c| c.name == "clear") {
        list.push(AdvertisedCommand {
            name: "clear".to_string(),
            source: "builtin".to_string(),
            aliases: vec!["cls".to_string()],
            description: Some("Clear conversation transcript".to_string()),
            hint: None,
            subcommands: Vec::new(),
        });
    }
    list
}

/// Dispatch desktop-handled slash commands directly to the transcript if matched.
/// Returns `true` if the command was completely handled locally and should not be sent to the sidecar.
pub fn handle_desktop_command(transcript: &mut Transcript, message: &str) -> bool {
    let trimmed = message.trim();
    if trimmed == "/help" || trimmed == "/?" {
        transcript.apply_command_output_with_command(HELP_TEXT, Some("/help"));
        return true;
    }
    if trimmed == "/clear" || trimmed == "/cls" {
        transcript.clear();
        transcript.apply_command_output_with_command(
            "Conversation transcript cleared. (Use New Thread to start an entirely fresh session)",
            Some("/clear"),
        );
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_builtins_are_added() {
        let list = with_desktop_builtins(Vec::new());
        assert!(list.iter().any(|c| c.name == "help"));
        assert!(list.iter().any(|c| c.name == "clear"));
    }

    #[test]
    fn handle_desktop_command_dispatches_help_and_clear() {
        let mut transcript = Transcript::new();
        assert!(handle_desktop_command(&mut transcript, "/help"));
        assert!(handle_desktop_command(&mut transcript, "/clear"));
        assert!(!handle_desktop_command(&mut transcript, "/other"));
    }
}
