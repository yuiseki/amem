# ADR 011: CLI Structure and Convenience Commands

## Status
Accepted

## Context
`amem` is used in short, frequent loops (searching memory, checking today's context, browsing entries).
The CLI should optimize daily assistant workflows while keeping command behavior explicit and scriptable.

## Decision
Define a command structure with stable aliases and convenience commands for daily use.

### 1. Root command
- Binary: `amem`
- Version: `amem --version`
- Default behavior (no subcommand): execute `amem today`

### 2. Bootstrap command
- `amem init`
- Purpose:
  - Initialize memory scaffold in the active memory root (default: `~/.amem`).
  - Create default owner/agent namespace structure if missing.
- Behavior:
  - Idempotent and non-destructive (must not overwrite existing files by default).
  - Prints resolved memory root path.
- Common options:
  - `--json`

### 3. Retrieval command
- `amem search <query>`
- Alias: `amem remember <query>`
- Purpose:
  - Run hybrid retrieval (lexical + semantic) over memory indexes.
- Common options:
  - `-k, --top-k <n>`
  - `--lexical-only`
  - `--semantic-only`
  - `--json`

### 4. Browsing command
- `amem list`
- Alias: `amem ls`
- Purpose:
  - List memory entries/files in `.amem` with optional filters.
- Common options:
  - `--path <glob>`
  - `--kind <owner|activity|tasks|inbox>`
  - `--date <yyyy-mm-dd>`
  - `--limit <n>`
  - `--json`

### 5. Daily digest command
- `amem today`
- Purpose:
  - Provide a practical daily snapshot for assistant execution.
  - Include at minimum:
    - owner essentials (`owner/profile.md`, `owner/preferences.md`)
    - open tasks
    - today's activity log
    - most relevant memory candidates for near-term actions
- Common options:
  - `--date <yyyy-mm-dd>` (default: local today)
  - `--json`

### 6. Existing core commands (from ADR 004)
- `amem index`
- `amem watch`
- `amem capture --kind <type> --text <text>`
- `amem context --task <text>`

### 7. Append-first capture command
- `amem keep <text>`
- Purpose:
  - Fast append-only memory capture for daily notes and assistant observations.
  - Default target is today's activity log.
- Common options:
  - `--kind <activity|inbox|task-note>`
  - `--date <yyyy-mm-dd>` (for backfilling activity logs)
  - `--source <manual|assistant|tool>`
  - `--json`
- Notes:
  - `keep` is the ergonomic command for append-first usage.
  - `capture` remains available for explicit/structured write scenarios.

### 8. Path discovery command
- `amem which`
- Purpose:
  - Print the absolute path to the active memory root directory (`~/.amem` by default).
  - Resolve overrides from `--memory-dir` / `AMEM_DIR` and return the final resolved absolute path.
- Output:
  - plain text absolute path (default)
  - JSON output with `--json`

### 9. Stability policy
- `init` must be idempotent and non-destructive by default.
- `search` and `remember` are equivalent and must stay backward compatible.
- `list` and `ls` are equivalent and must stay backward compatible.
- `keep` must remain append-only by default to avoid accidental overwrite flows.
- `which` output must be stable for scripting (single absolute path line in text mode).
- Machine-readable output must remain stable under `--json`.

### 10. Codex bridge command
- `amem codex`
- Purpose:
  - Start a Codex session with memory bootstrap context from the active memory root.
  - Use a seed flow (`codex exec --json --dangerously-bypass-approvals-and-sandbox` with memory snapshot), extract `thread_id`, then run `codex resume --dangerously-bypass-approvals-and-sandbox <thread_id>`.
- Common options:
  - `--resume-only` (skip seeding and resume the latest conversation)
  - `--prompt <text>` (append an initial user prompt at resume)
- Notes:
  - Default behavior uses YOLO mode (`--dangerously-bypass-approvals-and-sandbox`) to avoid repeated permission prompts.
  - This command is intended as an ergonomic bridge, not a replacement for Codex native session APIs.

### 11. Gemini bridge command
- `amem gemini`
- Purpose:
  - Start a Gemini CLI session with memory bootstrap context from the active memory root.
  - Use a seed flow (`gemini --approval-mode yolo --output-format json -p` with memory snapshot), extract `session_id`, then run `gemini --approval-mode yolo --resume <session_id>`.
- Common options:
  - `--resume-only` (skip seeding and resume latest directly)
  - `--prompt <text>` (pass initial interactive prompt at resume)
- Notes:
  - Default behavior uses YOLO mode (`--approval-mode yolo`) to avoid repeated permission prompts.
  - This command follows the same bridge pattern as `amem codex`.

### 12. Claude bridge command
- `amem claude`
- Purpose:
  - Start a Claude Code session with memory bootstrap context from the active memory root.
  - Use a seed flow (`claude --dangerously-skip-permissions --print --output-format json` with memory snapshot), extract `session_id`, then run `claude --dangerously-skip-permissions --resume <session_id>`.
- Common options:
  - `--resume-only` (skip seeding and run `claude --continue`)
  - `--prompt <text>` (append an initial user prompt when resuming)
- Notes:
  - Default behavior uses YOLO mode (`--dangerously-skip-permissions`) to avoid repeated permission prompts.
  - This command avoids `--continue` after seeding to prevent restoring the wrong conversation.

### 13. Copilot bridge command
- `amem copilot`
- Purpose:
  - Start a GitHub Copilot CLI session with memory bootstrap context from the active memory root.
  - Use a seed flow (`copilot -p ... --allow-all --share`), extract `session_id` from the generated `copilot-session-<id>.md`, then run `copilot --allow-all --resume <session_id>`.
- Common options:
  - `--resume-only` (skip seeding and run `copilot --continue`)
  - `--prompt <text>` (append an initial interactive prompt when resuming)
- Notes:
  - Default behavior uses YOLO mode (`--allow-all`) to avoid repeated permission prompts.
  - This command avoids blind `--continue` after seeding to prevent restoring the wrong conversation.

### 14. Domain-oriented get/set commands
- `amem get <domain> ...`
- `amem set <domain> ...`
- Purpose:
  - Provide a human-first command surface focused on intent and memory domains.
  - Keep existing low-level commands available for backward compatibility and scripting.
- Initial domains:
  - `owner`
  - `acts`
  - `tasks`
- Notes:
  - Canonical behavior and alias rules are detailed in ADR 012.
  - Existing commands (`keep`, `list`, `today`, `search`) remain supported.

## Consequences
- Faster day-to-day operation for both humans and assistant automation.
- Lower prompt/tool friction due to memorable command names (`remember`, `today`).
- Slightly larger command surface area requiring command-level tests and docs maintenance.
