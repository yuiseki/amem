# ADR 005: Owner Profile Memory Model

## Status
Accepted

## Context
An AI secretary is only useful if it consistently reflects owner identity, preferences, and interests.
These facts should not be mixed with short-lived activity logs.

## Decision
Normalize owner-specific memory under `owner/` and separate it from activity streams.

### 1. Required files
- `.amem/owner/profile.md` (name, github_username, location, occupation, native_language, timezone, constraints)
- `.amem/owner/personality.md`
- `.amem/owner/preferences.md`
- `.amem/owner/interests.md`
- `.amem/owner/diary/YYYY/MM/YYYY-MM-DD.md` (optional but recommended life log stream)

### 2. Memory metadata
Where possible, each durable fact should include:
- `source`
- `updated_at`
- `confidence` (`high|medium|low`)

### 3. Context priority order
1. `owner/profile.md`
2. `owner/preferences.md`
3. `agent/tasks/open.md`
4. recent `agent/activity/*`

### 4. Update policy
- Owner references can be overwritten when needed.
- Keep edits diff-friendly and include short rationale where practical.

## Consequences
- Better consistency for personalized assistance.
- Less retrieval noise by separating stable and temporal memory.
- Owner docs may contain sensitive data and require explicit access controls.
