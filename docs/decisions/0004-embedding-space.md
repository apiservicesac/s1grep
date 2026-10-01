# 0004. Every vector is keyed by its embedding space

- Status: Accepted (2026-10-01)
- Implementation: Stage 1.

## Context

Vectors are stored under a bare model name ("granite", "granite-outline"). Changing the model revision, the
dimension, the text that is embedded or the outline length would silently reuse incompatible vectors.

## Decision

A vector's key is its EmbeddingSpace: model id, revision, dimension, version of the embedded text format, and pass
(outline or whole). Any change creates a new space. A new model is indexed side by side and switched to once its
coverage is complete; old spaces are removed by garbage collection.

## Alternatives considered

- Wipe the cache on every change: re-embeds everything, including other projects.

## Consequences

- Existing "granite" vectors migrate to the current space once; outline vectors are recomputed.
