pub mod bash;
pub mod nushell;
pub mod powershell;

use crate::types::ParsedCommand;

/// Extract all commands from a shell command string.
/// Uses bash parser as the top-level parser, then delegates to
/// powershell/nushell parsers when `pwsh -c` or `nu -c` patterns are detected.
pub fn extract_commands(command: &str) -> Vec<ParsedCommand> {
    bash::extract_commands(command)
}
