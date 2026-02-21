# Workspaces Agent Rules

## Scope

These rules apply only to this repository (`/home/yuiseki/Workspaces`) and its subdirectories.

## Startup Memory Restore (amem)

At the beginning of each new Codex session in this repository, do the following before deep work:

1. Resolve `amem` command in this order:
   - `amem` from `PATH`
   - `/home/yuiseki/Workspaces/repos/amem/target/debug/amem`
2. Run `amem init` (idempotent).
3. Run `amem today --json`.
4. Resolve memory root with `amem which` and always read `owner/profile.md`.
5. Read both the today snapshot and owner profile, then use them as working context for the rest of the session.
6. If `amem` is unavailable, report that briefly and continue.

## Ongoing Logging

When meaningful work is completed in this repository, append a short activity entry using:

- `amem keep "<what was done>" --kind activity --source codex`
