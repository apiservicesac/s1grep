# 0003. SQLite as the source of truth, a flat vector matrix in memory

- Status: Accepted (2026-10-01)
- Implementation: Stage 2.

## Context

Up to a few hundred thousand functions, brute-force cosine over a contiguous matrix takes milliseconds. HNSW, vector
databases and sqlite-vec add dependencies and approximation without a measurable gain at this size.

## Decision

Vectors stay in SQLite. Each project session loads them once into a contiguous row-major f32 matrix with id maps,
updated in place as embeddings arrive. A flat file per project and embedding space may persist the matrix for fast
start-up; it is always rebuildable from SQLite.

## Alternatives considered

- usearch/HNSW now: worth it only above about a million functions.
- An external vector database: a server to run, against the local-first principle.

## Consequences

- Search time grows linearly with project size; int8 or binary prefiltering with float rescoring is the next step if
  it ever matters.
