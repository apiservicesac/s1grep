# 0007. Hybrid retrieval: vectors and BM25, fused, then the judge

- Status: Accepted (2026-10-01)
- Implementation: Stage 4.

## Context

Embeddings find meaning and cross the language gap; they are weaker on exact identifiers, error strings and paths.
ck, osgrep and grepai fuse a lexical and a vector list with reciprocal rank fusion (k = 60).

## Decision

A tantivy index per project with fields for symbol, path, signature and docstring, and body; a tokenizer that splits
camelCase and snake_case, with no stemming and no stopwords. It is updated incrementally from change sets. The two
lists are fused with RRF, and the judge reads the head of the fused list.

## Alternatives considered

- Lexical search only (Cody's choice): loses Spanish-to-English and paraphrased queries.
- SQLite FTS5: workable, but less control over tokenization and scoring.

## Consequences

- Fusion weights are tuned on the development exam, never on the test split.
