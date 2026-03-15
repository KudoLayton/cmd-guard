use std::path::PathBuf;

use regex::Regex;

use crate::types::{AllowEntry, AllowlistConfig, ParsedCommand};

/// Load allowlist config from the first available location:
/// 1. Project `.claude/cmd-guard.toml`
/// 2. User `~/.claude/cmd-guard.toml`
pub fn load_config() -> AllowlistConfig {
    let candidates = config_paths();
    for path in candidates {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(config) = toml::from_str::<AllowlistConfig>(&content) {
                    return config;
                }
            }
        }
    }
    AllowlistConfig::default()
}

fn config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".claude").join("cmd-guard.toml"));
    }

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".claude").join("cmd-guard.toml"));
    }

    paths
}

/// Check if all commands are allowed by the config.
/// Returns None if all allowed, Some(denied_descriptions) if any are not allowed.
pub fn check_commands(
    commands: &[ParsedCommand],
    config: &AllowlistConfig,
) -> Option<Vec<String>> {
    let not_allowed: Vec<String> = commands
        .iter()
        .filter_map(|cmd| is_not_allowed(cmd, config))
        .collect();

    if not_allowed.is_empty() {
        None
    } else {
        Some(not_allowed)
    }
}

fn is_not_allowed(cmd: &ParsedCommand, config: &AllowlistConfig) -> Option<String> {
    let Some(entry) = find_entry(cmd, config) else {
        return Some(cmd.to_string());
    };

    let args_str = cmd.args_string();

    // deny_pattern takes priority
    for pattern in &entry.deny_pattern {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(&args_str) {
                return Some(format!("{} (denied: {})", cmd, pattern));
            }
        }
    }

    // Empty sub = all subcommands allowed
    if entry.sub.is_empty() {
        return None;
    }

    // Check first arg against allowed subcommands
    let Some(first_arg) = cmd.args.first() else {
        return Some(cmd.to_string());
    };

    if entry
        .sub
        .iter()
        .any(|s| s.eq_ignore_ascii_case(first_arg))
    {
        None
    } else {
        Some(cmd.to_string())
    }
}

fn find_entry<'a>(cmd: &ParsedCommand, config: &'a AllowlistConfig) -> Option<&'a AllowEntry> {
    config
        .allow
        .get(&cmd.name)
        .or_else(|| config.allow.get(&cmd.name.to_lowercase()))
        .or_else(|| {
            config
                .allow
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&cmd.name))
                .map(|(_, v)| v)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AllowlistConfig {
        let toml_str = r#"
[allow.ls]

[allow.grep]

[allow.echo]

[allow.git]
sub = ["diff", "log", "status", "push"]
deny_pattern = ['push\s.*--force', 'push\s.*-f']

[allow.npm]
sub = ["install", "run"]
deny_pattern = ['install\s.*--global', 'install\s.*-g']
"#;
        toml::from_str(toml_str).unwrap()
    }

    fn cmd(name: &str, args: &[&str]) -> ParsedCommand {
        ParsedCommand {
            name: name.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn simple_command_allowed() {
        let c = config();
        assert!(check_commands(&[cmd("ls", &["-la"])], &c).is_none());
    }

    #[test]
    fn simple_command_all_args_allowed() {
        let c = config();
        assert!(check_commands(&[cmd("ls", &["-la", "/tmp"])], &c).is_none());
    }

    #[test]
    fn unknown_command_not_allowed() {
        let c = config();
        let denied = check_commands(&[cmd("rm", &["-rf", "/"])], &c).unwrap();
        assert_eq!(denied, vec!["rm -rf"]);
    }

    #[test]
    fn git_allowed_subcommand() {
        let c = config();
        assert!(check_commands(&[cmd("git", &["diff", "--stat"])], &c).is_none());
        assert!(check_commands(&[cmd("git", &["log", "--oneline"])], &c).is_none());
        assert!(check_commands(&[cmd("git", &["status"])], &c).is_none());
    }

    #[test]
    fn git_denied_subcommand() {
        let c = config();
        let denied = check_commands(&[cmd("git", &["commit", "-m", "msg"])], &c).unwrap();
        assert_eq!(denied, vec!["git commit"]);
    }

    #[test]
    fn git_push_allowed() {
        let c = config();
        assert!(check_commands(&[cmd("git", &["push", "origin", "main"])], &c).is_none());
    }

    #[test]
    fn git_push_force_denied() {
        let c = config();
        let denied = check_commands(&[cmd("git", &["push", "--force"])], &c).unwrap();
        assert!(denied[0].contains("denied"));
    }

    #[test]
    fn git_push_force_with_remote_denied() {
        let c = config();
        let denied =
            check_commands(&[cmd("git", &["push", "origin", "--force"])], &c).unwrap();
        assert!(denied[0].contains("denied"));
    }

    #[test]
    fn git_push_f_short_denied() {
        let c = config();
        let denied = check_commands(&[cmd("git", &["push", "-f"])], &c).unwrap();
        assert!(denied[0].contains("denied"));
    }

    #[test]
    fn npm_install_allowed() {
        let c = config();
        assert!(check_commands(&[cmd("npm", &["install", "express"])], &c).is_none());
    }

    #[test]
    fn npm_install_global_denied() {
        let c = config();
        let denied =
            check_commands(&[cmd("npm", &["install", "--global", "pkg"])], &c).unwrap();
        assert!(denied[0].contains("denied"));
    }

    #[test]
    fn npm_install_g_denied() {
        let c = config();
        let denied = check_commands(&[cmd("npm", &["install", "-g", "pkg"])], &c).unwrap();
        assert!(denied[0].contains("denied"));
    }

    #[test]
    fn case_insensitive() {
        let c = config();
        assert!(check_commands(&[cmd("LS", &[])], &c).is_none());
        assert!(check_commands(&[cmd("Git", &["Diff"])], &c).is_none());
    }

    #[test]
    fn git_bare_not_allowed_by_subcommand_rule() {
        let c = config();
        let denied = check_commands(&[cmd("git", &[])], &c).unwrap();
        assert_eq!(denied, vec!["git"]);
    }

    #[test]
    fn empty_commands_allowed() {
        let c = config();
        assert!(check_commands(&[], &c).is_none());
    }

    #[test]
    fn empty_config_denies_all() {
        let c = AllowlistConfig::default();
        let denied = check_commands(&[cmd("ls", &[])], &c).unwrap();
        assert_eq!(denied, vec!["ls"]);
    }

    #[test]
    fn mixed_allowed_and_denied() {
        let c = config();
        let cmds = vec![
            cmd("ls", &[]),
            cmd("git", &["commit", "-m", "msg"]),
            cmd("grep", &["foo"]),
        ];
        let denied = check_commands(&cmds, &c).unwrap();
        assert_eq!(denied, vec!["git commit"]);
    }

    #[test]
    fn toml_deserialization() {
        let c = config();
        assert!(c.allow.contains_key("ls"));
        assert!(c.allow.contains_key("git"));

        let git = c.allow.get("git").unwrap();
        assert!(git.sub.contains(&"push".to_string()));
        assert!(!git.deny_pattern.is_empty());

        let ls = c.allow.get("ls").unwrap();
        assert!(ls.sub.is_empty());
        assert!(ls.deny_pattern.is_empty());
    }
}
