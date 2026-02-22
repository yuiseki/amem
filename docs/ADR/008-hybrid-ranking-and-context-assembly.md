# ADR 008: Hybrid Ranking and Context Assembly

## Status
Accepted

## Context
`amem` must return useful context for owner-centric assistant tasks.
Lexical search is strong for exact terms, while embeddings are strong for paraphrases.
A deterministic fusion and context assembly policy is required.

## Decision
Use hybrid retrieval (lexical + semantic), fuse with RRF, and assemble context with explicit source priorities.

### 1. Candidate generation
- Lexical candidates:
  - phase 1: UTF-8 unigram candidate scoring (overlap + IDF-like weights)
  - phase 2: morphology-aware BM25 (e.g., Lindera tokens)
- Semantic candidates:
  - cosine similarity on embeddings
- Collect top-K from both pipelines before fusion.

### 2. Fusion and scoring
- Use Reciprocal Rank Fusion (RRF) as default:
  - `rrf_score = Σ 1 / (k + rank_i)`
- Apply boosts after fusion:
  - recency boost for recent activity files
  - owner-priority boost for `owner/*`
  - task-priority boost for `agent/tasks/open.md`
- Keep final score and component breakdown for debugging.

### 3. Context assembly policy
- Build final context in this order:
  1. owner essentials (`owner/profile.md`, `owner/preferences.md`)
  2. open tasks (`agent/tasks/open.md`)
  3. top-ranked recent activity chunks
  4. supporting historical chunks
- Deduplicate near-identical chunks by path + overlap hash.
- Enforce token budget with stable truncation from lowest-priority tail.

### 4. Explainability
- Each returned chunk includes:
  - source path
  - line range
  - lexical score
  - semantic score
  - final fused score

## Consequences
- Better recall across both exact-match and paraphrased queries.
- Stable, explainable context construction for assistant workflows.
- Weight tuning (RRF constant and boosts) becomes a key quality lever.
- Phase-1 lexical quality is recall-oriented; phase-2 improves precision with BM25.
