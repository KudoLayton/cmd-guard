use tree_sitter::{Node, Parser};

use crate::types::ParsedCommand;

/// Extract all commands from a nushell command string.
pub fn extract_commands(input: &str) -> Vec<ParsedCommand> {
    let mut parser = Parser::new();
    let language = tree_sitter_nu::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("failed to load nushell grammar");

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
        "where_command" | "overlay_hide" | "overlay_use" | "overlay_new" | "hide_command"
        | "module_command" | "use_command" | "export_command" => {
            if let Some(keyword) = node.child(0) {
                let text = keyword.utf8_text(source).unwrap_or("").trim().to_string();
                if !text.is_empty() {
                    commands.push(ParsedCommand {
                        name: text,
                        args: vec![],
                    });
                }
            }
        }
        _ => {}
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
            "cmd_identifier" => {
                let text = child.utf8_text(source).unwrap_or("").trim().to_string();
                if !text.is_empty() {
                    name = Some(text);
                }
            }
            "val_string" | "val_number" | "val_variable" | "path" => {
                let text = child.utf8_text(source).unwrap_or("").trim().to_string();
                if !text.is_empty() && name.is_some() {
                    args.push(text);
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
    fn single_command() {
        let cmds = extract_commands("ls");
        assert_eq!(names(&cmds), vec!["ls"]);
    }

    #[test]
    fn pipeline() {
        let cmds = extract_commands("ls | where size > 1mb");
        assert_eq!(names(&cmds), vec!["ls", "where"]);
    }

    #[test]
    fn long_pipeline() {
        let cmds = extract_commands("ls | where size > 1mb | sort-by size");
        assert_eq!(names(&cmds), vec!["ls", "where", "sort-by"]);
    }

    #[test]
    fn semicolon_chain() {
        let cmds = extract_commands("echo hello; ls");
        assert_eq!(names(&cmds), vec!["echo", "ls"]);
    }

    #[test]
    fn command_with_args() {
        let cmds = extract_commands("open file.txt");
        assert_eq!(cmds[0].name, "open");
        assert_eq!(cmds[0].args, vec!["file.txt"]);
    }

    #[test]
    fn empty_input() {
        assert!(extract_commands("").is_empty());
    }

    #[test]
    fn err_redirect() {
        let cmds = extract_commands("cat file.txt err> errors.log");
        let n = names(&cmds);
        assert!(n.contains(&"cat"), "got: {:?}", n);
    }

    #[test]
    fn out_err_redirect() {
        let cmds = extract_commands("make build out+err> all.log");
        let n = names(&cmds);
        assert!(n.contains(&"make"), "got: {:?}", n);
    }

    #[test]
    fn pipe_with_err_redirect() {
        let cmds = extract_commands("ls | where size > 1mb err> /dev/null");
        let n = names(&cmds);
        assert!(n.contains(&"ls"), "got: {:?}", n);
        assert!(n.contains(&"where"), "got: {:?}", n);
    }
}
