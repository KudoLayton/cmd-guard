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

### 1. Install

```sh
cargo install --git https://github.com/KudoLayton/cmd-guard
```

This installs `cmd-guard` to `~/.cargo/bin/`. Alternatively, clone and build locally:

```sh
cargo build --release
```

### 2. Configure allowlist

Create `~/.claude/cmd-guard/config.toml` (user-level) and/or `.claude/cmd-guard/config.toml` (project-level).

> **Legacy paths** (`~/.claude/cmd-guard.toml`, `.claude/cmd-guard.toml`) are still supported as fallback. A deprecation notice will be printed to stderr when detected.

#### Using presets

Presets let you bulk-allow common read-only commands without listing them individually:

```toml
presets = [
    "bash-readonly",    # bash builtins + coreutils
    "nu-readonly",      # nushell builtins + nu launcher
    "git-readonly",     # git diff, log, status, etc.
    "cargo-readonly",   # cargo metadata, tree, etc.
    "cargo-build",      # cargo build, check, clippy, doc
    "cargo-test",       # cargo test, bench
]

# Additional rules on top of presets
[allow.git]
deny_pattern = ['push\s.*--force']
```

Run `cmd-guard --help` to see all available presets.

#### Customizing presets

To customize built-in presets, extract them to your local config directory:

```sh
cmd-guard init
```

This creates `~/.claude/cmd-guard/presets/` with all 35 preset TOML files. Edit any file to customize — local files take priority over embedded presets.

Use `cmd-guard init --force` to overwrite existing files.

#### Manual rules

```toml
# All arguments allowed
[allow.ls]
[allow.grep]

# Subcommand-restricted with deny patterns
[allow.git]
sub = ["diff", "log", "status", "push"]
deny_sub = ["push"]
deny_pattern = ['push\s.*--force', 'push\s.*-f']

# Multi-word subcommands
[allow.gh]
sub = ["pr list", "pr view", "pr status", "issue list", "issue view"]
```

See [`config/allowlist.example.toml`](config/allowlist.example.toml) for a more complete example.

### Allow rules

| Config | Meaning |
|--------|---------|
| `[allow.ls]` (empty section) | Allow command with any arguments |
| `sub = ["diff", "log"]` | Allow only listed subcommands |
| `sub = ["pr list", "pr view"]` | Multi-word subcommand matching |
| `deny_sub = ["push"]` | Deny specific subcommands even if listed in `sub` |
| `deny_pattern = ['push\s.*--force']` | Deny args matching regex |

**Check priority**: `deny_pattern` > `deny_sub` > `sub`

- Commands not in `[allow.*]` trigger a permission prompt (`ask`)
- Matching is case-insensitive
- Path prefixes are stripped (`/usr/bin/env` → `env`)
- `deny_pattern` matches against the full argument string (args joined by spaces)
- Multi-word `sub`/`deny_sub` entries match against the first N args in order

### Available presets

| Category | Presets |
|----------|--------|
| Shell | `bash-readonly`, `nu-readonly`, `ps-readonly`, `cmd-readonly`, `no-coreutils` |
| Tool (readonly) | `git-readonly`, `npm-readonly`, `pnpm-readonly`, `yarn-readonly`, `cargo-readonly`, `pip-readonly`, `go-readonly`, `node-readonly`, `rustup-readonly` |
| Tool (build) | `cargo-build`, `npm-build`, `pnpm-build`, `yarn-build`, `git-fetch`, `go-build` |
| Tool (test) | `cargo-test`, `npm-test`, `pnpm-test`, `yarn-test`, `go-test` |
| Container | `docker-readonly`, `podman-readonly` |
| OS Package Manager | `apt-readonly`, `dnf-readonly`, `pacman-readonly`, `brew-readonly`, `winget-readonly`, `choco-readonly`, `scoop-readonly` |
| CLI Tool | `kubectl-readonly`, `gh-readonly` |

### Config merging

When both user-level and project-level configs exist, they are merged with field-level union:

- **Disjoint commands**: both sides preserved
- **Overlapping commands**: `sub`, `deny_sub`, `deny_pattern`, and `presets` are each combined (union, deduplicated)

This allows a user-level config to define a broad allowlist, while project-level configs can add restrictions via `deny_sub` or add extra commands.

```
User-level:  git { sub: ["diff", "log", "push"] }
Project:     git { deny_sub: ["push"] }
Merged:      git { sub: ["diff", "log", "push"], deny_sub: ["push"] }
→ git diff ✅  git push ❌
```

### Preset loading priority

1. Runtime (user): `~/.claude/cmd-guard/presets/<name>.toml`
2. Runtime (project): `.claude/cmd-guard/presets/<name>.toml`
3. Embedded: built-in presets compiled into the binary

Runtime files override embedded presets with the same name.

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
            "command": "cmd-guard",
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
| Subcommand in `deny_sub` | `"ask"` — with denied sub info in reason |
| Args match `deny_pattern` | `"ask"` — with pattern info in reason |
| Parse failure | `"ask"` — safe fallback |
| Non-Bash tool call | No output — ignored |

## CLI

```
cmd-guard              Run as PreToolUse hook (reads JSON from stdin)
cmd-guard init         Extract embedded presets to ~/.claude/cmd-guard/presets/
cmd-guard init --force Overwrite existing preset files
cmd-guard -h, --help   Show this help message
```

## Project structure

```
src/
├── main.rs              # Entry point: stdin → parse → decide → stdout
├── types.rs             # Hook I/O, ParsedCommand, config types
├── allowlist.rs         # TOML config loading, subcommand + regex matching
├── preset.rs            # Preset embedding, runtime loading, init command
└── parser/
    ├── mod.rs           # Common interface
    ├── bash.rs          # Bash parser + 2-stage dispatch
    ├── powershell.rs    # PowerShell parser
    └── nushell.rs       # Nushell parser
presets/                 # Preset TOML files (embedded at compile time)
```

## Testing

```sh
cargo test
```

Manual test:

```sh
echo '{"tool_name":"Bash","tool_input":{"command":"ls | grep foo"}}' | ./target/release/cmd-guard.exe
```
