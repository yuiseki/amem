# ADR 013: Owner/Agent Namespace Split

## Status
Accepted

## Context
`amem` stores both stable owner knowledge and operational agent logs in the same root.
As memory grows, these concerns should be separated explicitly:
- Owner-side memory (`profile`, preferences, life logs)
- Agent-side operational memory (activity, tasks, inbox)

This split enables durable owner life-log capture (for example `owner/diary/*`) while keeping assistant execution logs isolated.

## Decision
Adopt a namespace split under `.amem`:

- Owner namespace:
  - `.amem/owner/profile.md`
  - `.amem/owner/personality.md`
  - `.amem/owner/preferences.md`
  - `.amem/owner/interests.md`
  - `.amem/owner/diary/YYYY/MM/YYYY-MM-DD.md`
- Agent namespace:
  - `.amem/agent/activity/YYYY/MM/YYYY-MM-DD.md`
  - `.amem/agent/tasks/open.md`
  - `.amem/agent/tasks/done.md`
  - `.amem/agent/inbox/captured.md`

`amem init` creates the new namespace-oriented scaffold by default.
`amem set diary` writes owner life logs into `owner/diary/YYYY/MM/YYYY-MM-DD.md`.

## Compatibility
- Legacy paths are still read for compatibility:
  - `activity/*`
  - `tasks/*`
  - `inbox/*`
- New writes target `agent/*` paths.

## Consequences
- Clear conceptual boundary between owner memory and agent operational logs.
- Better extensibility for owner-first long-term memory (including diary/life logs).
- Slightly more complex path resolution due to backward compatibility handling.
