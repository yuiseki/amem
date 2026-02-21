# ADR 010: Database Selection

## Status
Accepted

## Context
`amem` needs local persistent storage for derived index data with the following properties:
- no external server dependency
- transactional safety for incremental indexing
- fast point lookups for file/chunk metadata
- support for lexical and semantic retrieval pipelines
- compatibility with single-binary distribution
- high implementation confidence in Rust

## Decision
Adopt SQLite as the v1 primary database for all derived index stores under `.amem/.index/`.
Use `rusqlite` as the Rust access layer.
Use `sqlite-vec` as an optional acceleration path for vector similarity, with exact-scan fallback when unavailable.

### 1. Scope of SQLite usage
- Keep index metadata, lexical postings/statistics, embeddings metadata, and embedding cache in SQLite-backed files.
- Maintain SoT in Markdown files only (`.amem/**/*.md`), not in SQLite.

### 2. Storage layout alignment
- `.amem/.index/index.db` (main SQLite index containing lexical, vector metadata, and embedding cache tables)
- `.amem/.index/manifest.json` (schema version and build metadata)
- `.amem/.index/tmp/index.db` (rebuild staging)

### 3. Retrieval behavior
- Lexical retrieval:
  - UTF-8 unigram tokens (language-agnostic baseline)
  - phase 1: overlap/IDF-like candidate scoring
  - phase 2: BM25 scoring with morphology-aware tokens
- Semantic retrieval:
  - cosine similarity against stored vectors
  - phase 1 default: exact scan
  - optional acceleration: `sqlite-vec` (if available in runtime environment)

### 4. Operational guarantees
- Use transactional writes for incremental index updates.
- Use temporary rebuild databases and atomic swap for safe `--rebuild`.
- Keep operation possible without any external DB daemon.

### 5. Alternatives considered (v1)
- DuckDB (`fts` + `vss`):
  - strong for analytics, but FTS/VSS extension maturity and JP token behavior increase adoption risk for core v1.
- LanceDB:
  - strong vector capabilities, but SQLite has lower integration/operational risk for v1 core.
- Tantivy:
  - excellent lexical engine, but not a complete single-store answer for metadata + vector in current scope.

## Consequences
- Consistent local-first operation with low operational overhead.
- Good durability and recoverability for personal memory indexes.
- Clear portability path: works even when `sqlite-vec` is absent.
- Large-scale vector search may require future ANN optimization or alternative backend in later phases.
