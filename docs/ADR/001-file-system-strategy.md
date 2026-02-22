# ADR 001: File System Strategy (`.amem` as Source of Truth)

## Status
Accepted

## Context
`amem` manages long-term and mid-term memory for an AI secretary.
We need memory that is human-readable, easy to edit, Git-friendly, and recoverable.

## Decision
Use Markdown files on the filesystem as the Source of Truth (SoT), rooted at `.amem`.

### 1. Root directory
- Default: `~/.amem/` (fallback: `./.amem/` only when home directory is unavailable)
- Overridable via `--memory-dir <path>` or `AMEM_DIR`

### 2. Base SoT layout
- `.amem/owner/profile.md` (name, github_username, location, occupation, native_language, core attributes)
- `.amem/owner/personality.md`
- `.amem/owner/preferences.md`
- `.amem/owner/interests.md`
- `.amem/activity/YYYY/MM/YYYY-MM-DD.md` (recent activity)
- `.amem/tasks/open.md`
- `.amem/tasks/done.md`
- `.amem/inbox/captured.md`

### 3. Derived data
- Indexes and caches are not SoT.
- Store derived artifacts under `.amem/.index/` only.
- Rebuild from SoT with `amem index --rebuild`.

### 4. Write conventions
- Append-first for event streams (`activity`, `inbox`).
- Structured updates for reference docs (`profile`, `preferences`).
- Persist as UTF-8 with LF line endings.

## Consequences
- High readability and portability.
- Works naturally with Git workflows.
- Index corruption is recoverable by rebuilding from SoT.
