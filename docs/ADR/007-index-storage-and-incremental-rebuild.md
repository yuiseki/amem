# ADR 007: Index Storage and Incremental Rebuild

## Status
Accepted

## Context
`amem` uses Markdown files in `.amem` as SoT, but search performance requires derived indexes.
We need a robust on-disk index format that supports:
- unigram full-text search
- semantic retrieval
- fast incremental updates
- safe recovery after interruption

## Decision
Store derived indexes under `.amem/.index/` with a versioned schema and incremental rebuild flow.

### 1. Index directory layout
- `.amem/.index/manifest.json`
- `.amem/.index/index.db` (SQLite main index: lexical, vector metadata, embedding cache)
- `.amem/.index/tmp/*` (temporary rebuild artifacts)

### 2. File identity and change detection
- Track each source file by:
  - normalized relative path
  - content hash (SHA-256)
  - mtime
- Reindex only changed files by default.
- `amem index --rebuild` forces full rebuild.

### 3. Chunking policy
- Chunk by heading/paragraph boundaries first.
- Fall back to fixed max token window when sections are too large.
- Keep chunk metadata: `path`, `section`, `line_start`, `line_end`, `updated_at`.

### 4. Safe rebuild semantics
- Build updated SQLite index in `.amem/.index/tmp/index.db`.
- Atomically swap active `index.db` only after full success.
- On failure, keep previous active indexes unchanged.

## Consequences
- Fast incremental indexing for day-to-day updates.
- Reliable recovery from crashes or partial failures.
- Clear version boundaries for future schema migration.
