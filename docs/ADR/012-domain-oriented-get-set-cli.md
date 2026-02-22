# ADR 012: Domain-Oriented `get/set` CLI

## Status
Accepted

## Context
The existing CLI has strong low-level primitives (`keep`, `list`, `today`, `search`) but can feel implementation-centric.
Daily usage is often phrased as domain operations: "get owner profile", "set owner preference", "get acts", "set tasks done".

## Decision
Introduce a domain-oriented command surface with `get`/`set` as first-class entry points.

### 1. Command model
- Read path: `amem get <domain> ...`
- Write path: `amem set <domain> ...`
- Preserve existing commands for backward compatibility (`today`, `keep`, `list`, `search`, etc).

### 2. Phase 1 domains
- `owner`
  - `amem get owner`
  - `amem get owner <key|alias>`
  - `amem set owner <key|alias> <value>`
  - `amem get owner preference`
  - `amem set owner preference <key:value>` (auto timestamp)
- `acts`
  - `amem get acts [today|yesterday|week|yyyy-mm-dd]`
  - `amem set acts <text>`
- `tasks`
  - `amem get tasks [today|yesterday|week|yyyy-mm-dd]`
  - `amem set tasks <text>`
  - `amem set tasks done <id|text>`
- Path mapping:
  - `acts` -> `agent/activity/*`
  - `tasks` -> `agent/tasks/*`

### 3. Owner key aliases (Phase 1)
- `github` -> `github_username`
- `job` -> `occupation`
- `lang` -> `native_language`
- Keep compatibility read path for legacy `github_handle`.

### 4. Task id strategy
- Generate deterministic short IDs from SHA-256 (`7` hex chars).
- Avoid MD5-specific dependency and keep stronger default hashing.

### 5. Time semantics
- `today`, `yesterday`, `week` are evaluated in local system timezone.
- `week` means the trailing 7-day window including today.
- For `get acts/tasks`, default result size is:
  - latest 10 entries when period is omitted
  - all matching entries when period is specified

## Consequences
- Improves usability for interactive assistant workflows.
- Reduces cognitive load by exposing domain verbs before storage details.
- Slightly expands CLI surface area; command docs/tests must stay synchronized.
