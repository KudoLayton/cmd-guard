use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    pub tool_input: ToolInput,
}

#[derive(Debug, Deserialize)]
pub struct ToolInput {
    pub command: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub hook_event_name: String,
    pub permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
}

impl HookOutput {
    pub fn allow() -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                permission_decision: "allow".to_string(),
                permission_decision_reason: None,
            },
        }
    }

    pub fn ask(reason: String) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                permission_decision: "ask".to_string(),
                permission_decision_reason: Some(reason),
            },
        }
    }
}

// --- Parsed command from tree-sitter ---

#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

impl ParsedCommand {
    pub fn args_string(&self) -> String {
        self.args.join(" ")
    }
}

impl fmt::Display for ParsedCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{} {}", self.name, self.args[0])
        }
    }
}

// --- Allowlist config (TOML) ---
//
// [allow.ls]
//
// [allow.git]
// sub = ["push", "diff", "log"]
// deny_pattern = ["push\\s.*--force"]

#[derive(Debug, Deserialize, Default)]
pub struct AllowlistConfig {
    #[serde(default)]
    pub allow: HashMap<String, AllowEntry>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AllowEntry {
    #[serde(default)]
    pub sub: Vec<String>,
    #[serde(default)]
    pub deny_sub: Vec<String>,
    #[serde(default)]
    pub deny_pattern: Vec<String>,
}
