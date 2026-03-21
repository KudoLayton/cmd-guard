use std::collections::HashSet;
use std::path::PathBuf;

use regex::Regex;

use crate::types::{AllowEntry, AllowlistConfig, ParsedCommand};

/// Load and merge allowlist configs from both locations:
/// - User `~/.claude/cmd-guard.toml` (global base)
/// - Project `.claude/cmd-guard.toml` (project override/restriction)
///
/// Merge strategy: field-level union for overlapping command keys.
/// `deny_sub` allows project config to block subcommands allowed by user config.
pub fn load_config() -> AllowlistConfig {
    let (project_path, user_path) = config_paths();
    let project = load_from_path(project_path);
    let user = load_from_path(user_path);
    merge_configs(project, user)
}

fn config_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    let project = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".claude").join("cmd-guard.toml"));
    let user = dirs::home_dir().map(|home| home.join(".claude").join("cmd-guard.toml"));
    (project, user)
}

fn load_from_path(path: Option<PathBuf>) -> Option<AllowlistConfig> {
    let path = path?;
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

fn merge_configs(
    project: Option<AllowlistConfig>,
    user: Option<AllowlistConfig>,
) -> AllowlistConfig {
    match (project, user) {
        (None, None) => AllowlistConfig::default(),
        (Some(c), None) | (None, Some(c)) => c,
        (Some(project), Some(user)) => {
            let all_keys: HashSet<&String> =
                project.allow.keys().chain(user.allow.keys()).collect();
            let mut merged = std::collections::HashMap::new();
            for key in all_keys {
                let entry = match (project.allow.get(key), user.allow.get(key)) {
                    (Some(p), Some(u)) => merge_entries(p, u),
                    (Some(e), None) | (None, Some(e)) => e.clone(),
                    (None, None) => unreachable!(),
                };
                merged.insert(key.clone(), entry);
            }
            AllowlistConfig { allow: merged }
        }
    }
}

fn merge_entries(a: &AllowEntry, b: &AllowEntry) -> AllowEntry {
    AllowEntry {
        sub: union(&a.sub, &b.sub),
        deny_sub: union(&a.deny_sub, &b.deny_sub),
        deny_pattern: union(&a.deny_pattern, &b.deny_pattern),
    }
}

fn union(a: &[String], b: &[String]) -> Vec<String> {
    let mut set: HashSet<String> = a.iter().cloned().collect();
    set.extend(b.iter().cloned());
    let mut result: Vec<String> = set.into_iter().collect();
    result.sort();
    result
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

    // deny_sub blocks specific subcommands even if listed in sub
    if let Some(first_arg) = cmd.args.first() {
        if entry
            .deny_sub
            .iter()
            .any(|s| s.eq_ignore_ascii_case(first_arg))
        {
            return Some(format!("{} (denied sub)", cmd));
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

    // --- merge_configs tests ---

    fn parse_config(toml_str: &str) -> AllowlistConfig {
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn merge_both_none() {
        let result = merge_configs(None, None);
        assert!(result.allow.is_empty());
    }

    #[test]
    fn merge_project_only() {
        let project = parse_config("[allow.ls]\n");
        let result = merge_configs(Some(project), None);
        assert!(result.allow.contains_key("ls"));
        assert_eq!(result.allow.len(), 1);
    }

    #[test]
    fn merge_user_only() {
        let user = parse_config("[allow.cargo]\n");
        let result = merge_configs(None, Some(user));
        assert!(result.allow.contains_key("cargo"));
        assert_eq!(result.allow.len(), 1);
    }

    #[test]
    fn merge_disjoint_keys() {
        let project = parse_config("[allow.npm]\nsub = [\"install\"]\n");
        let user = parse_config("[allow.cargo]\n");
        let result = merge_configs(Some(project), Some(user));
        assert!(result.allow.contains_key("npm"));
        assert!(result.allow.contains_key("cargo"));
        assert_eq!(result.allow.len(), 2);
    }

    #[test]
    fn merge_overlapping_keys_union() {
        let project = parse_config(
            "[allow.git]\nsub = [\"diff\", \"log\"]\ndeny_pattern = ['push\\s.*--force']\n",
        );
        let user = parse_config("[allow.git]\nsub = [\"diff\", \"commit\"]\n");
        let result = merge_configs(Some(project), Some(user));
        let git = result.allow.get("git").unwrap();
        assert!(git.sub.contains(&"diff".to_string()));
        assert!(git.sub.contains(&"log".to_string()));
        assert!(git.sub.contains(&"commit".to_string()));
        assert!(git.deny_pattern.contains(&r"push\s.*--force".to_string()));
    }

    #[test]
    fn merge_deny_sub_union() {
        let project = parse_config("[allow.git]\ndeny_sub = [\"push\"]\n");
        let user = parse_config("[allow.git]\nsub = [\"diff\", \"push\"]\ndeny_sub = [\"reset\"]\n");
        let result = merge_configs(Some(project), Some(user));
        let git = result.allow.get("git").unwrap();
        assert!(git.deny_sub.contains(&"push".to_string()));
        assert!(git.deny_sub.contains(&"reset".to_string()));
        assert!(git.sub.contains(&"diff".to_string()));
        assert!(git.sub.contains(&"push".to_string()));
    }

    #[test]
    fn merge_integrated_check() {
        let project = parse_config("[allow.ls]\n[allow.git]\ndeny_sub = [\"push\"]\n");
        let user = parse_config("[allow.git]\nsub = [\"diff\", \"push\", \"log\"]\n[allow.cargo]\n");
        let merged = merge_configs(Some(project), Some(user));
        // ls allowed (from project)
        assert!(check_commands(&[cmd("ls", &["-la"])], &merged).is_none());
        // cargo allowed (from user)
        assert!(check_commands(&[cmd("cargo", &["build"])], &merged).is_none());
        // git diff allowed (sub from user, not in deny_sub)
        assert!(check_commands(&[cmd("git", &["diff"])], &merged).is_none());
        // git push denied (in sub but blocked by deny_sub from project)
        let denied = check_commands(&[cmd("git", &["push"])], &merged).unwrap();
        assert!(denied[0].contains("denied sub"));
        // rm not in either config → denied
        assert!(check_commands(&[cmd("rm", &["-rf"])], &merged).is_some());
    }

    // --- deny_sub check tests ---

    #[test]
    fn deny_sub_blocks_allowed_sub() {
        let c = parse_config(
            "[allow.git]\nsub = [\"diff\", \"push\", \"commit\"]\ndeny_sub = [\"push\"]\n",
        );
        assert!(check_commands(&[cmd("git", &["diff"])], &c).is_none());
        let denied = check_commands(&[cmd("git", &["push", "origin"])], &c).unwrap();
        assert!(denied[0].contains("denied sub"));
    }

    #[test]
    fn deny_sub_empty_allows_all() {
        let c = parse_config("[allow.git]\nsub = [\"diff\", \"push\"]\ndeny_sub = []\n");
        assert!(check_commands(&[cmd("git", &["diff"])], &c).is_none());
        assert!(check_commands(&[cmd("git", &["push"])], &c).is_none());
    }

    #[test]
    fn deny_sub_case_insensitive() {
        let c = parse_config("[allow.git]\nsub = [\"Push\"]\ndeny_sub = [\"push\"]\n");
        let denied = check_commands(&[cmd("git", &["PUSH"])], &c).unwrap();
        assert!(denied[0].contains("denied sub"));
    }
}
