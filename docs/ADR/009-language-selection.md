# ADR 009: Language Selection

## Status
Accepted

## Context
`amem` must run as a single binary, handle local indexing/search efficiently, and remain stable for long-running personal assistant workflows.
Key requirements include:
- strong CLI ergonomics
- safe filesystem and concurrency handling
- good performance for indexing and retrieval
- smooth integration with UTF-8 unigram tokenization and Ollama HTTP APIs

## Decision
Implement `amem` in Rust.

### 1. Runtime and distribution model
- Build a single native binary with no required runtime installation.
- Prefer static linking where practical for easy deployment.

### 2. Capability fit
- Use Rust crates for:
  - CLI parsing
  - filesystem traversal and watch
  - HTTP client calls to Ollama
  - UTF-8 aware tokenization
  - local database access (SQLite)

### 3. Reliability and safety
- Rely on Rust's ownership model to reduce memory corruption and data races.
- Keep indexing and search operations explicit and recoverable by design.

## Consequences
- Good fit for single-binary distribution and predictable resource use.
- Strong baseline for performance and correctness.
- Higher implementation complexity than scripting languages, with longer initial development time.
