# ADR 003: Embedding Provider Strategy (Ollama First)

## Status
Accepted

## Context
`amem` requires semantic search for Japanese text.
To support local execution and privacy-friendly operation, Ollama should be first-class.

## Decision
Use Ollama as the default embedding provider and combine semantic + lexical retrieval.

### 1. Provider priority
1. `ollama` (default)
2. `none` (disable embeddings, lexical-only)
3. Future pluggable providers (e.g., OpenAI)

### 2. Configuration
- `AMEM_EMBED_PROVIDER=ollama|none|...`
- `AMEM_OLLAMA_HOST` (e.g., `http://127.0.0.1:11434`)
- `AMEM_OLLAMA_MODEL` (embedding model name)

### 3. Failure behavior
- If Ollama is unavailable or model is missing, log a warning and continue in lexical-only mode.
- `amem index` allows partial success and can be rerun safely.

### 4. Caching
- Keep embedding cache in SQLite table `embedding_cache` inside `.amem/.index/index.db`.
- Cache key: `provider + model + content_hash`.

## Consequences
- Semantic search works in local-only environments.
- System remains usable without Ollama.
- Embedding model quality directly affects semantic ranking quality.
