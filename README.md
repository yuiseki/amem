# amem

Local memory CLI for AI assistant or AI agent workflows.

`amem` manages a Markdown-based memory store (default: `~/.amem`) and provides:

- daily snapshot rendering (`today`)
- append-only logging (`keep` / `capture`)
- memory listing and search (`list` / `search`)
- optional SQLite indexing (`index`)
- bridge commands for coding agents (`codex`, `gemini`, `claude`, `copilot`)

## Install

Build and install from source:

```bash
cd /home/yuiseki/Workspaces/repos/amem
cargo install --path .
```

Run without installing:

```bash
cargo run -q -- --help
```

## Usage

```bash
amem --help
```

Top-level commands:

- `init`
- `search` (alias: `remember`)
- `list` (alias: `ls`)
- `today`
- `keep`
- `which`
- `index`
- `watch`
- `capture`
- `context`
- `get`
- `set`
- `owner` (alias for `get owner`)
- `codex`
- `gemini`
- `claude`
- `copilot`

Global options:

- `--memory-dir <path>`: override memory root
- `--json`: JSON output mode

## Quick Start

```bash
amem init
amem keep "Implemented feature X" --kind activity --source manual
amem today
amem search feature --top-k 5
```

## Main Commands

### `amem init`

Create scaffold files/directories (idempotent, non-destructive).

### `amem which`

Print resolved memory root path.

### `amem keep <text>`

Append an entry.

- `--kind <activity|inbox|task-note>` (default: `activity`)
- `--date <yyyy-mm-dd>` (default: today)
- `--source <name>` (default: `manual`)

Examples:

```bash
amem keep "Investigated bug #123"
amem keep "Read later: article URL" --kind inbox
amem keep "Prepare weekly review" --kind task-note --source codex
```

### `amem capture --kind <kind> --text <text>`

Structured wrapper for `keep` (same write behavior/options).

### `amem list` / `amem ls`

List memory files.

- `--kind <owner|activity|tasks|inbox>`
- `--path <glob>`
- `--date <yyyy-mm-dd>` (string match filter)
- `--limit <n>`

### `amem today`

Render Today Snapshot (Markdown by default, JSON with `--json`).

- `--date <yyyy-mm-dd>`
- Markdown snapshot labels use explicit namespaces:
  - `Owner Profile`
  - `Owner Preferences` (hidden when empty)
  - `Agent Tasks`
  - `Agent Activities`

### `amem context --task <text>`

Build task-oriented context from today snapshot + related memory hits.

- `--date <yyyy-mm-dd>`

### `amem get ...`

Domain-oriented read commands:

- `amem get owner`
- `amem get owner <name|github|github_username|email|location|job|occupation|lang|native_language|birthday>`
- `amem get owner preference`
- `amem get acts [today|yesterday|week|yyyy-mm-dd]`
- `amem get tasks [today|yesterday|week|yyyy-mm-dd]`

`get acts/tasks` options:

- `--limit <n>`
- default behavior:
  - without period: latest 10 entries
  - with period (`today|yesterday|week|yyyy-mm-dd`): all matching entries

### `amem set ...`

Domain-oriented write commands:

- `amem set owner <key> <value>`
- `amem set owner preference <key:value>` (auto timestamp)
- `amem set acts <text>`
- `amem set tasks <text>` (returns short task id)
- `amem set tasks done <id|text>`

### `amem search <query>` / `amem remember <query>`

Search memory entries (top-k scored hits with snippet).

- `-k, --top-k <n>` (default: `8`)
- `--lexical-only`
- `--semantic-only`

Notes:

- If `.index/index.db` exists, search uses the index; otherwise it scans Markdown files directly.
- `--semantic-only` currently returns no hits (semantic retrieval is not implemented yet).

### `amem index`

Build/rebuild local SQLite index:

- output path: `<memory-root>/.index/index.db`
- `--rebuild`: delete existing DB before rebuilding

### `amem watch`

Reserved command. Current output:

- `watch mode is not implemented yet. use amem index periodically.`

## Coding Agent Bridge Commands

These commands bootstrap memory context into each agent, then resume an interactive session.

Common options:

- `--resume-only`: skip seed step and directly resume latest session
- `--prompt <text>`: append an initial prompt when resuming

### `amem codex`

- Seed: `codex exec --json --dangerously-bypass-approvals-and-sandbox ...`
- Resume: `codex resume --dangerously-bypass-approvals-and-sandbox ...`
- `--resume-only` uses `codex resume --last`

### `amem gemini`

- Seed: `gemini --approval-mode yolo --output-format json -p ...`
- Resume: `gemini --approval-mode yolo --resume <session_id>`
- `--resume-only` uses `gemini --resume latest`

### `amem claude`

- Seed: `claude --dangerously-skip-permissions --print --output-format json ...`
- Resume: `claude --dangerously-skip-permissions --resume <session_id>`
- `--resume-only` uses `claude --continue`

### `amem copilot`

- Seed: `copilot -p ... --allow-all --share`
- Resume: `copilot --allow-all --resume <session_id>`
- `--resume-only` uses `copilot --allow-all --continue`

Note:

- Bridge commands default to YOLO/auto-approval style flags to reduce repeated permission prompts.

## Memory Layout

Default root: `~/.amem`

Scaffold created by `amem init`:

- `owner/profile.md`
- `owner/personality.md`
- `owner/preferences.md`
- `owner/interests.md`
- `owner/diary/`
- `agent/tasks/open.md`
- `agent/tasks/done.md`
- `agent/inbox/captured.md`
- `agent/activity/YYYY/MM/YYYY-MM-DD.md` (created on first write)

Compatibility:

- Legacy paths (`tasks/*`, `inbox/*`, `activity/*`) are still read for backward compatibility.

Default `owner/profile.md` template fields:

- `name`
- `github_username`
- `location`
- `occupation`
- `native_language`

Index files:

- `.index/index.db`

## Environment Variables

- `AMEM_DIR`: override memory root (same priority as `--memory-dir`, lower than CLI flag)
- `AMEM_CODEX_BIN`: override `codex` executable
- `AMEM_GEMINI_BIN`: override `gemini` executable
- `AMEM_CLAUDE_BIN`: override `claude` executable
- `AMEM_COPILOT_BIN`: override `copilot` executable

## Development

```bash
cargo fmt
cargo test
cargo build
```
