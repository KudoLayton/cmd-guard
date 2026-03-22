mod allowlist;
mod parser;
mod preset;
mod types;

use std::io::Read;

use types::HookOutput;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Handle subcommands
    if !args.is_empty() {
        match args[0].as_str() {
            "init" => {
                let force = args.iter().any(|a| a == "--force");
                preset::init_presets(force);
                return;
            }
            "-h" | "--help" => {
                preset::print_help();
                return;
            }
            _ => {
                eprintln!("Unknown command: {}", args[0]);
                eprintln!("Run 'cmd-guard --help' for usage information.");
                return;
            }
        }
    }

    // Default: hook mode (stdin JSON → stdout JSON)
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        // Cannot read stdin - let Claude Code handle normally
        return;
    }

    let hook_input: types::HookInput = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return, // Invalid JSON - exit silently
    };

    // Only process Bash tool calls
    if hook_input.tool_name != "Bash" {
        return;
    }

    let command = &hook_input.tool_input.command;

    // Parse command into individual commands
    let commands = parser::extract_commands(command);

    if commands.is_empty() {
        // Could not extract any commands - ask user
        let output = HookOutput::ask("Failed to parse command".to_string());
        print_json(&output);
        return;
    }

    // Check against allowlist
    let config = allowlist::load_config();
    match allowlist::check_commands(&commands, &config) {
        None => {
            // All commands allowed
            let output = HookOutput::allow();
            print_json(&output);
        }
        Some(denied) => {
            let reason = format!("Commands not in allowlist: {}", denied.join(", "));
            let output = HookOutput::ask(reason);
            print_json(&output);
        }
    }
}

fn print_json(output: &HookOutput) {
    if let Ok(json) = serde_json::to_string(output) {
        println!("{}", json);
    }
}
