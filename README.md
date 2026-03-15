# cmd-guard

Claude Code [PreToolUse hook](https://docs.anthropic.com/en/docs/claude-code/hooks) for granular permission control over piped/chained shell commands.

## Problem

Claude Code treats piped commands (e.g. `ls | grep foo`) as a single pattern for permission matching. Even if `ls` and `grep` are individually allowed, the combined command triggers a new permission prompt every time.

## Solution

This tool parses shell commands into individual commands using [tree-sitter](https://tree-sitter.github.io/) and checks each one against a configurable allowlist — with subcommand-level control and regex deny patterns.

### Supported shells

| Shell | Parser | 2-stage parsing |
|-------|--------|-----------------|
| Bash | tree-sitter-bash | Top-level (always) |
| PowerShell | tree-sitter-powershell | Via `pwsh -c "..."` |
| Nushell | tree-sitter-nu | Via `nu -c "..."` |
| cmd.exe | tree-sitter-bash (reuse) | Via `cmd /c "..."` |

### 2-stage parsing

When the top-level bash command is `nu.exe -c "..."`, `pwsh.exe -c "..."`, or `cmd.exe /c "..."`, the inner string is re-parsed with the corresponding shell grammar (cmd.exe reuses the bash parser since pipe/chain syntax is compatible):

```
Input:  nu.exe -c "ls | where size > 1mb"

Stage 1 (bash):   nu.exe  →  allowlist check
Stage 2 (nushell): ls, where  →  allowlist check
```

## Setup

### 1. Build

```sh
cargo build --release
```

### 2. Configure allowlist

Create `~/.claude/cmd-guard.toml` (user-level) or `.claude/cmd-guard.toml` (project-level):

```toml
# All arguments allowed
[allow.ls]
[allow.grep]
[allow.echo]

# Subcommand-restricted with deny patterns
[allow.git]
sub = ["diff", "log", "status", "push"]
deny_pattern = ['push\s.*--force', 'push\s.*-f']

[allow.npm]
sub = ["install", "run", "test"]
deny_pattern = ['install\s.*--global', 'install\s.*-g']
```

See [`config/allowlist.example.toml`](config/allowlist.example.toml) for a more complete example.

### Allow rules

| Config | Meaning |
|--------|---------|
| `[allow.ls]` (empty section) | Allow command with any arguments |
| `sub = ["diff", "log"]` | Allow only listed subcommands |
| `deny_pattern = ['push\s.*--force']` | Deny args matching regex (takes priority over sub) |

- Commands not in `[allow.*]` trigger a permission prompt (`ask`)
- Matching is case-insensitive
- Path prefixes are stripped (`/usr/bin/env` → `env`)
- `deny_pattern` matches against the full argument string (args joined by spaces)

### 3. Register hook

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/cmd-guard.exe",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

## Behavior

| Scenario | Output |
|----------|--------|
| All commands in allowlist | `"allow"` — no prompt |
| Command not in allowlist | `"ask"` — normal permission prompt |
| Args match `deny_pattern` | `"ask"` — with pattern info in reason |
| Parse failure | `"ask"` — safe fallback |
| Non-Bash tool call | No output — ignored |

## Project structure

```
src/
├── main.rs              # Entry point: stdin → parse → decide → stdout
├── types.rs             # Hook I/O, ParsedCommand, config types
├── allowlist.rs         # TOML config loading, subcommand + regex matching
└── parser/
    ├── mod.rs           # Common interface
    ├── bash.rs          # Bash parser + 2-stage dispatch
    ├── powershell.rs    # PowerShell parser
    └── nushell.rs       # Nushell parser
```

## Testing

```sh
cargo test
```

Manual test:

```sh
echo '{"tool_name":"Bash","tool_input":{"command":"ls | grep foo"}}' | ./target/release/cmd-guard.exe
```
