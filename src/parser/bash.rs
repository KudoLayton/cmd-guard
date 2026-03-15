use tree_sitter::{Node, Parser};

use crate::types::ParsedCommand;

use super::{nushell, powershell};

const NUSHELL_COMMANDS: &[&str] = &["nu", "nu.exe"];
const POWERSHELL_COMMANDS: &[&str] = &["pwsh", "pwsh.exe", "powershell", "powershell.exe"];
const CMD_COMMANDS: &[&str] = &["cmd", "cmd.exe"];

/// Extract all commands from a bash command string.
/// Detects `nu -c` and `pwsh -c` patterns for 2-stage parsing.
pub fn extract_commands(input: &str) -> Vec<ParsedCommand> {
    let mut parser = Parser::new();
    let language = tree_sitter_bash::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("failed to load bash grammar");

    let Some(tree) = parser.parse(input, None) else {
        return vec![];
    };

    let source = input.as_bytes();
    let mut commands = Vec::new();
    collect_commands(tree.root_node(), source, &mut commands);
    commands
}

fn collect_commands(node: Node, source: &[u8], commands: &mut Vec<ParsedCommand>) {
    match node.kind() {
        "command" => {
            extract_from_command(node, source, commands);
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_commands(child, source, commands);
                }
            }
        }
    }
}

fn extract_from_command(node: Node, source: &[u8], commands: &mut Vec<ParsedCommand>) {
    let mut command_name: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    let mut nested_substitutions: Vec<Node> = Vec::new();

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else {
            continue;
        };
        match child.kind() {
            "command_name" => {
                if let Some(name_node) = child.child(0) {
                    let text = node_text(name_node, source);
                    command_name = Some(basename(&text));
                }
            }
            _ if command_name.is_some() && is_argument_node(child.kind()) => {
                args.push(unquote(&node_text(child, source)));
            }
            "command_substitution" => {
                nested_substitutions.push(child);
            }
            _ => {}
        }
    }

    if let Some(name) = command_name {
        try_second_stage(&name, &args, commands);
        commands.push(ParsedCommand {
            name,
            args: args.clone(),
        });
    }

    for sub_node in nested_substitutions {
        collect_commands(sub_node, source, commands);
    }
}

fn try_second_stage(
    command_name: &str,
    args: &[String],
    commands: &mut Vec<ParsedCommand>,
) {
    // cmd.exe uses /c (case-insensitive), others use -c
    if CMD_COMMANDS.contains(&command_name) {
        let c_index = args.iter().position(|a| a.eq_ignore_ascii_case("/c"));
        let Some(c_idx) = c_index else { return };
        let inner = if c_idx + 1 < args.len() {
            &args[c_idx + 1]
        } else {
            return;
        };
        // cmd.exe pipe/chain syntax is close enough to bash for tree-sitter-bash to parse
        commands.extend(extract_commands(inner));
        return;
    }

    let c_index = args.iter().position(|a| a == "-c");
    let Some(c_idx) = c_index else { return };
    let inner_command = if c_idx + 1 < args.len() {
        &args[c_idx + 1]
    } else {
        return;
    };

    if NUSHELL_COMMANDS.contains(&command_name) {
        commands.extend(nushell::extract_commands(inner_command));
    } else if POWERSHELL_COMMANDS.contains(&command_name) {
        commands.extend(powershell::extract_commands(inner_command));
    }
}

fn is_argument_node(kind: &str) -> bool {
    matches!(
        kind,
        "word" | "string" | "raw_string" | "concatenation" | "simple_expansion" | "expansion"
    )
}

fn node_text(node: Node, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or("").to_string()
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn basename(s: &str) -> String {
    s.rsplit(['/', '\\']).next().unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cmds: &[ParsedCommand]) -> Vec<&str> {
        cmds.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn single_command() {
        let cmds = extract_commands("ls");
        assert_eq!(names(&cmds), vec!["ls"]);
    }

    #[test]
    fn pipe() {
        let cmds = extract_commands("ls | grep foo");
        assert_eq!(names(&cmds), vec!["ls", "grep"]);
    }

    #[test]
    fn chain_and() {
        let cmds = extract_commands("ls && echo done");
        assert_eq!(names(&cmds), vec!["ls", "echo"]);
    }

    #[test]
    fn chain_or() {
        let cmds = extract_commands("ls || echo fail");
        assert_eq!(names(&cmds), vec!["ls", "echo"]);
    }

    #[test]
    fn semicolon() {
        let cmds = extract_commands("ls; echo done");
        assert_eq!(names(&cmds), vec!["ls", "echo"]);
    }

    #[test]
    fn pipe_in_quotes_ignored() {
        let cmds = extract_commands(r#"echo "hello | world""#);
        assert_eq!(names(&cmds), vec!["echo"]);
    }

    #[test]
    fn command_substitution() {
        let cmds = extract_commands("echo $(ls | grep foo)");
        assert_eq!(names(&cmds), vec!["echo", "ls", "grep"]);
    }

    #[test]
    fn path_basename() {
        let cmds = extract_commands("/usr/bin/env node");
        assert_eq!(names(&cmds), vec!["env"]);
        assert_eq!(cmds[0].args, vec!["node"]);
    }

    #[test]
    fn variable_assignment_prefix() {
        let cmds = extract_commands("FOO=bar echo hello");
        assert_eq!(names(&cmds), vec!["echo"]);
    }

    #[test]
    fn complex_pipeline() {
        let cmds = extract_commands("cat file.txt | sort | uniq -c | head -10");
        assert_eq!(names(&cmds), vec!["cat", "sort", "uniq", "head"]);
    }

    #[test]
    fn redirection() {
        let cmds = extract_commands("echo foo > output.txt");
        assert_eq!(names(&cmds), vec!["echo"]);
    }

    #[test]
    fn subshell() {
        let cmds = extract_commands("(ls | grep foo) && echo done");
        assert_eq!(names(&cmds), vec!["ls", "grep", "echo"]);
    }

    #[test]
    fn empty_input() {
        assert!(extract_commands("").is_empty());
    }

    #[test]
    fn pwsh_second_stage() {
        let cmds = extract_commands(r#"pwsh.exe -c "Get-Process | Select-Object Name""#);
        let n = names(&cmds);
        assert!(n.contains(&"pwsh.exe"));
        assert!(n.contains(&"Get-Process"));
        assert!(n.contains(&"Select-Object"));
    }

    #[test]
    fn pwsh_no_c_flag_no_inner_parse() {
        let cmds = extract_commands(r#"pwsh.exe -File script.ps1"#);
        assert_eq!(names(&cmds), vec!["pwsh.exe"]);
    }

    #[test]
    fn backtick_substitution() {
        let cmds = extract_commands("echo `date`");
        assert_eq!(names(&cmds), vec!["echo", "date"]);
    }

    #[test]
    fn nested_pipe_and_substitution() {
        let cmds = extract_commands("echo $(cat file | sort) | head");
        assert_eq!(names(&cmds), vec!["echo", "cat", "sort", "head"]);
    }

    #[test]
    fn mixed_chain_and_pipe() {
        let cmds = extract_commands("ls | grep foo && echo done || echo fail");
        assert_eq!(names(&cmds), vec!["ls", "grep", "echo", "echo"]);
    }

    #[test]
    fn nu_second_stage() {
        let cmds = extract_commands(r#"nu.exe -c "ls | where size > 1mb | sort-by size""#);
        let n = names(&cmds);
        assert!(n.contains(&"nu.exe"));
        assert!(n.contains(&"ls"));
        assert!(n.contains(&"where"));
        assert!(n.contains(&"sort-by"));
    }

    #[test]
    fn git_subcommand_in_args() {
        let cmds = extract_commands("git diff --stat");
        assert_eq!(cmds[0].name, "git");
        assert_eq!(cmds[0].args, vec!["diff", "--stat"]);
    }

    #[test]
    fn npm_subcommand() {
        let cmds = extract_commands("npm install express");
        assert_eq!(cmds[0].name, "npm");
        assert_eq!(cmds[0].args, vec!["install", "express"]);
    }

    #[test]
    fn cmd_second_stage() {
        let cmds = extract_commands(r#"cmd.exe /c "dir | findstr foo""#);
        let n = names(&cmds);
        assert!(n.contains(&"cmd.exe"));
        assert!(n.contains(&"dir"));
        assert!(n.contains(&"findstr"));
    }

    #[test]
    fn cmd_second_stage_chain() {
        let cmds = extract_commands(r#"cmd /C "echo hello && dir""#);
        let n = names(&cmds);
        assert!(n.contains(&"cmd"));
        assert!(n.contains(&"echo"));
        assert!(n.contains(&"dir"));
    }

    #[test]
    fn cmd_no_c_flag_no_inner_parse() {
        let cmds = extract_commands(r#"cmd.exe /k "echo hello""#);
        assert_eq!(names(&cmds), vec!["cmd.exe"]);
    }

    // --- stderr/redirection edge cases ---

    #[test]
    fn stderr_redirect() {
        let cmds = extract_commands("gcc main.c 2>/dev/null");
        assert_eq!(names(&cmds), vec!["gcc"]);
    }

    #[test]
    fn stderr_to_stdout_pipe() {
        let cmds = extract_commands("gcc main.c 2>&1 | grep error");
        assert_eq!(names(&cmds), vec!["gcc", "grep"]);
    }

    #[test]
    fn stdout_and_stderr_redirect() {
        let cmds = extract_commands("make build >out.log 2>err.log");
        assert_eq!(names(&cmds), vec!["make"]);
    }

    #[test]
    fn stderr_append_redirect() {
        let cmds = extract_commands("cargo test 2>>errors.log");
        assert_eq!(names(&cmds), vec!["cargo"]);
    }

    #[test]
    fn combined_redirect_and_pipe() {
        let cmds = extract_commands("cmd1 2>/dev/null | cmd2 | cmd3 >out.txt 2>&1");
        assert_eq!(names(&cmds), vec!["cmd1", "cmd2", "cmd3"]);
    }
}
