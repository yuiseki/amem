# ADR 004: Single-binary Architecture

## Status
Accepted

## Context
A core requirement is that indexing and search run as a single-binary local tool.
Operational complexity should stay low.

## Decision
Ship `amem` as a single CLI binary that handles indexing, retrieval, and context assembly in one process.

### 1. Execution model
- Binary: `amem`
- Core commands:
  - `amem index`
  - `amem search <query>`
  - `amem watch`
  - `amem context --task <text>`
  - `amem capture --kind <type> --text <text>`

### 2. Internal data boundaries
- SoT: `.amem/**/*.md`
- Derived index data: `.amem/.index/*`
- No mandatory external DB server or daemon.

### 3. Retrieval fusion
- Combine lexical and semantic results:
  - phase 1 lexical: UTF-8 unigram candidate scoring
  - phase 2 lexical: morphology-aware BM25
  - semantic: embedding cosine
- Default fusion method: Reciprocal Rank Fusion (RRF).

### 4. Optional dependency handling
- Treat Ollama as an optional external endpoint.
- Keep all core commands working when Ollama is unavailable.

## Consequences
- Easy installation and operation.
- Debugging surface stays small.
- In-process performance tuning becomes important at larger scale.
