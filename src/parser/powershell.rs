use tree_sitter::{Node, Parser};

use crate::types::ParsedCommand;

/// Extract all commands from a PowerShell command string.
pub fn extract_commands(input: &str) -> Vec<ParsedCommand> {
    let mut parser = Parser::new();
    let language = tree_sitter_powershell::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("failed to load powershell grammar");

    let Some(tree) = parser.parse(input, None) else {
        return vec![];
    };

    let source = input.as_bytes();
    let mut commands = Vec::new();
    collect_commands(tree.root_node(), source, &mut commands);
    commands
}

fn collect_commands(node: Node, source: &[u8], commands: &mut Vec<ParsedCommand>) {
    if node.kind() == "command" {
        extract_from_command(node, source, commands);
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_commands(child, source, commands);
        }
    }
}

fn extract_from_command(node: Node, source: &[u8], commands: &mut Vec<ParsedCommand>) {
    let mut name: Option<String> = None;
    let mut args: Vec<String> = Vec::new();

    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else {
            continue;
        };
        match child.kind() {
            "command_name" => {
                let text = child.utf8_text(source).unwrap_or("").trim().to_string();
                if !text.is_empty() {
                    name = Some(text);
                }
            }
            "command_elements" => {
                for j in 0..child.child_count() {
                    if let Some(arg_node) = child.child(j) {
                        if arg_node.kind() == "generic_token" {
                            let text = arg_node.utf8_text(source).unwrap_or("").trim().to_string();
                            if !text.is_empty() {
                                args.push(text);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(n) = name {
        commands.push(ParsedCommand { name: n, args });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cmds: &[ParsedCommand]) -> Vec<&str> {
        cmds.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn single_cmdlet() {
        let cmds = extract_commands("Get-Process");
        assert_eq!(names(&cmds), vec!["Get-Process"]);
    }

    #[test]
    fn pipeline() {
        let cmds = extract_commands("Get-Process | Select-Object Name");
        assert_eq!(names(&cmds), vec!["Get-Process", "Select-Object"]);
        assert_eq!(cmds[1].args, vec!["Name"]);
    }

    #[test]
    fn semicolon_chain() {
        let cmds = extract_commands("Get-Process; Get-Service");
        assert!(names(&cmds).contains(&"Get-Process"));
        assert!(names(&cmds).contains(&"Get-Service"));
    }
}
