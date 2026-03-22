use std::path::PathBuf;

use crate::allowlist::merge_entries;
use crate::types::AllowlistConfig;

// Embedded preset TOML files
const EMBEDDED_PRESETS: &[(&str, &str)] = &[
    // Shell presets
    ("bash-readonly", include_str!("../presets/bash-readonly.toml")),
    ("nu-readonly", include_str!("../presets/nu-readonly.toml")),
    ("ps-readonly", include_str!("../presets/ps-readonly.toml")),
    ("cmd-readonly", include_str!("../presets/cmd-readonly.toml")),
    // Tool readonly
    ("git-readonly", include_str!("../presets/git-readonly.toml")),
    ("npm-readonly", include_str!("../presets/npm-readonly.toml")),
    ("pnpm-readonly", include_str!("../presets/pnpm-readonly.toml")),
    ("yarn-readonly", include_str!("../presets/yarn-readonly.toml")),
    ("cargo-readonly", include_str!("../presets/cargo-readonly.toml")),
    ("pip-readonly", include_str!("../presets/pip-readonly.toml")),
    ("go-readonly", include_str!("../presets/go-readonly.toml")),
    ("node-readonly", include_str!("../presets/node-readonly.toml")),
    ("rustup-readonly", include_str!("../presets/rustup-readonly.toml")),
    // Tool build
    ("cargo-build", include_str!("../presets/cargo-build.toml")),
    ("npm-build", include_str!("../presets/npm-build.toml")),
    ("pnpm-build", include_str!("../presets/pnpm-build.toml")),
    ("yarn-build", include_str!("../presets/yarn-build.toml")),
    ("git-fetch", include_str!("../presets/git-fetch.toml")),
    ("go-build", include_str!("../presets/go-build.toml")),
    // Tool test
    ("cargo-test", include_str!("../presets/cargo-test.toml")),
    ("npm-test", include_str!("../presets/npm-test.toml")),
    ("pnpm-test", include_str!("../presets/pnpm-test.toml")),
    ("yarn-test", include_str!("../presets/yarn-test.toml")),
    ("go-test", include_str!("../presets/go-test.toml")),
    // Container
    ("docker-readonly", include_str!("../presets/docker-readonly.toml")),
    ("podman-readonly", include_str!("../presets/podman-readonly.toml")),
    // OS package managers
    ("apt-readonly", include_str!("../presets/apt-readonly.toml")),
    ("dnf-readonly", include_str!("../presets/dnf-readonly.toml")),
    ("pacman-readonly", include_str!("../presets/pacman-readonly.toml")),
    ("brew-readonly", include_str!("../presets/brew-readonly.toml")),
    ("winget-readonly", include_str!("../presets/winget-readonly.toml")),
    ("choco-readonly", include_str!("../presets/choco-readonly.toml")),
    ("scoop-readonly", include_str!("../presets/scoop-readonly.toml")),
    // CLI tools
    ("kubectl-readonly", include_str!("../presets/kubectl-readonly.toml")),
    ("gh-readonly", include_str!("../presets/gh-readonly.toml")),
];

/// Commands included by coreutils in bash-readonly.
/// Used by the `no-coreutils` modifier to remove these entries.
const COREUTILS_COMMANDS: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "grep", "find", "diff", "sort", "uniq", "tr", "basename",
    "dirname", "realpath", "file", "stat", "du", "df", "less", "more", "seq",
];

/// Apply presets to a merged config.
/// Preset entries are the base layer; explicit config entries take precedence.
pub fn apply_presets(config: &mut AllowlistConfig) {
    let preset_names = config.presets.clone();
    let has_no_coreutils = preset_names.iter().any(|p| p == "no-coreutils");

    for name in &preset_names {
        if name == "no-coreutils" {
            continue;
        }

        let preset_config = load_preset(name);
        let Some(preset_config) = preset_config else {
            eprintln!("[cmd-guard] Unknown preset: {}", name);
            continue;
        };

        for (key, preset_entry) in preset_config.allow {
            if has_no_coreutils
                && name == "bash-readonly"
                && COREUTILS_COMMANDS.contains(&key.as_str())
            {
                continue;
            }

            config
                .allow
                .entry(key)
                .and_modify(|existing| {
                    *existing = merge_entries(&preset_entry, existing);
                })
                .or_insert(preset_entry);
        }
    }
}

/// Load a preset by name.
/// Priority: runtime (user/project presets dir) > embedded.
fn load_preset(name: &str) -> Option<AllowlistConfig> {
    // Try runtime paths first
    if let Some(config) = load_runtime_preset(name) {
        return Some(config);
    }

    // Fall back to embedded
    load_embedded_preset(name)
}

fn load_runtime_preset(name: &str) -> Option<AllowlistConfig> {
    let filename = format!("{}.toml", name);

    // User-level: ~/.claude/cmd-guard/presets/
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".claude").join("cmd-guard").join("presets").join(&filename);
        if let Some(config) = load_toml_file(&path) {
            return Some(config);
        }
    }

    // Project-level: .claude/cmd-guard/presets/
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join(".claude").join("cmd-guard").join("presets").join(&filename);
        if let Some(config) = load_toml_file(&path) {
            return Some(config);
        }
    }

    None
}

fn load_toml_file(path: &PathBuf) -> Option<AllowlistConfig> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

fn load_embedded_preset(name: &str) -> Option<AllowlistConfig> {
    EMBEDDED_PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, content)| toml::from_str(content).ok())
}

/// Extract all embedded presets to ~/.claude/cmd-guard/presets/.
/// Skips files that already exist unless `force` is true.
pub fn init_presets(force: bool) {
    let Some(home) = dirs::home_dir() else {
        eprintln!("[cmd-guard] Could not determine home directory");
        return;
    };

    let presets_dir = home.join(".claude").join("cmd-guard").join("presets");
    if let Err(e) = std::fs::create_dir_all(&presets_dir) {
        eprintln!("[cmd-guard] Failed to create directory: {}", e);
        return;
    }

    let mut created = 0;
    let mut skipped = 0;

    for (name, content) in EMBEDDED_PRESETS {
        let path = presets_dir.join(format!("{}.toml", name));
        if path.exists() && !force {
            skipped += 1;
            continue;
        }
        match std::fs::write(&path, content) {
            Ok(()) => {
                println!("  Created: {}.toml", name);
                created += 1;
            }
            Err(e) => {
                eprintln!("  Failed:  {}.toml ({})", name, e);
            }
        }
    }

    println!(
        "\nExtracted {} presets to {}",
        created,
        presets_dir.display()
    );
    if skipped > 0 {
        println!("Skipped {} existing files (use --force to overwrite)", skipped);
    }
}

/// Print help message with available presets.
pub fn print_help() {
    println!("cmd-guard - Shell command permission control for Claude Code");
    println!();
    println!("USAGE:");
    println!("  cmd-guard              Run as PreToolUse hook (reads JSON from stdin)");
    println!("  cmd-guard init         Extract embedded presets to ~/.claude/cmd-guard/presets/");
    println!("  cmd-guard init --force Overwrite existing preset files");
    println!("  cmd-guard -h, --help   Show this help message");
    println!();
    println!("AVAILABLE PRESETS:");
    println!();
    println!("  Shell:");
    println!("    bash-readonly    Bash builtins + coreutils (use no-coreutils to exclude)");
    println!("    nu-readonly      Nushell builtins + nu launcher");
    println!("    ps-readonly      PowerShell read-only cmdlets + pwsh launcher");
    println!("    cmd-readonly     cmd.exe read-only builtins + cmd launcher");
    println!("    no-coreutils     Modifier: exclude coreutils from bash-readonly");
    println!();
    println!("  Tool (readonly):");
    println!("    git-readonly     cargo-readonly    pip-readonly     node-readonly");
    println!("    npm-readonly     go-readonly       rustup-readonly");
    println!("    pnpm-readonly    yarn-readonly");
    println!();
    println!("  Tool (build):");
    println!("    cargo-build      npm-build         pnpm-build       yarn-build");
    println!("    git-fetch        go-build");
    println!();
    println!("  Tool (test):");
    println!("    cargo-test       npm-test          pnpm-test        yarn-test");
    println!("    go-test");
    println!();
    println!("  Container:");
    println!("    docker-readonly  podman-readonly");
    println!();
    println!("  OS Package Manager:");
    println!("    apt-readonly     dnf-readonly      pacman-readonly  brew-readonly");
    println!("    winget-readonly  choco-readonly    scoop-readonly");
    println!();
    println!("  CLI Tool:");
    println!("    kubectl-readonly gh-readonly");
}

/// Return the list of all available preset names.
#[allow(dead_code)]
pub fn available_preset_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = EMBEDDED_PRESETS.iter().map(|(n, _)| *n).collect();
    names.push("no-coreutils");
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_presets_parse() {
        for (name, content) in EMBEDDED_PRESETS {
            let result: Result<AllowlistConfig, _> = toml::from_str(content);
            assert!(result.is_ok(), "Failed to parse preset '{}': {:?}", name, result.err());
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(load_embedded_preset("nonexistent").is_none());
    }

    #[test]
    fn bash_readonly_contains_expected_commands() {
        let config = load_embedded_preset("bash-readonly").unwrap();
        assert!(config.allow.contains_key("echo"));
        assert!(config.allow.contains_key("ls"));
        assert!(config.allow.contains_key("cat"));
        assert!(config.allow.contains_key("pwd"));
    }

    #[test]
    fn no_coreutils_removes_coreutils() {
        let toml_str = r#"presets = ["bash-readonly", "no-coreutils"]"#;
        let mut config: AllowlistConfig = toml::from_str(toml_str).unwrap();
        apply_presets(&mut config);

        // Bash builtins should remain
        assert!(config.allow.contains_key("echo"));
        assert!(config.allow.contains_key("pwd"));
        // Coreutils should be removed
        assert!(!config.allow.contains_key("ls"));
        assert!(!config.allow.contains_key("cat"));
        assert!(!config.allow.contains_key("head"));
    }

    #[test]
    fn multiple_presets_merge_subs() {
        let toml_str = r#"presets = ["git-readonly", "git-fetch"]"#;
        let mut config: AllowlistConfig = toml::from_str(toml_str).unwrap();
        apply_presets(&mut config);

        let git = config.allow.get("git").unwrap();
        // From git-readonly
        assert!(git.sub.contains(&"diff".to_string()));
        assert!(git.sub.contains(&"log".to_string()));
        // From git-fetch
        assert!(git.sub.contains(&"fetch".to_string()));
        assert!(git.sub.contains(&"pull".to_string()));
    }

    #[test]
    fn preset_with_user_override() {
        let toml_str = r#"
presets = ["git-readonly"]

[allow.git]
deny_sub = ["config"]
"#;
        let mut config: AllowlistConfig = toml::from_str(toml_str).unwrap();
        apply_presets(&mut config);

        let git = config.allow.get("git").unwrap();
        // Sub from preset
        assert!(git.sub.contains(&"diff".to_string()));
        // deny_sub from user
        assert!(git.deny_sub.contains(&"config".to_string()));
    }

    #[test]
    fn available_presets_includes_no_coreutils() {
        let names = available_preset_names();
        assert!(names.contains(&"no-coreutils"));
        assert!(names.contains(&"bash-readonly"));
        assert!(names.contains(&"gh-readonly"));
    }
}
