# ADR 002: UTF-8 Unigram Full-text Search Baseline

## Status
Accepted

## Context
`amem` needs a language-agnostic lexical baseline that works in a local single-binary setup.
The immediate requirement is unigram-based full-text search over UTF-8 text.
Morphological analysis can be added later, but is not required for v1.

## Decision
Make UTF-8 unigram tokenization mandatory for lexical indexing/search in v1.

### 1. Tokenization
- Use the same UTF-8 unigram tokenizer for indexing and querying.
- Define unigram as one token per Unicode scalar value (not byte-level tokenization).
- Do not use n-gram indexes in v1.

### 2. Ranking
- Phase 1 (v1):
  - Use unigram overlap + IDF-like weighting for lexical candidate scoring.
  - Treat lexical as high-recall candidate generation, not final ranking.
- Phase 2:
  - Introduce morphology-aware tokenization (Lindera) and BM25 scoring.
- Apply field weights and recency boost in both phases:
  - `owner/*` > `tasks/*` > `activity/*` > `inbox/*`

### 3. Future extension
- Keep an extension point for optional morphology-based tokenizers (e.g., Lindera).
- Phase-2 scope includes morphology, BM25, synonym expansion, and keyword extraction.

## Consequences
- Deterministic lexical behavior across all UTF-8 text.
- Search behavior remains explainable.
- Character-level unigram can increase noise for long texts, so phase-1 uses lexical as candidate generation.
- BM25 quality is expected to improve after phase-2 morphology rollout.
