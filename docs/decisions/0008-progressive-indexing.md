# 0008. Progressive indexing with priorities and reported coverage

- Status: Accepted (2026-10-01)
- Implementation: outline pass in 0.2.4; bounded first wait, yielding to searches and resuming on start in 0.2.5.

## Context

Indexing a 16,000-function repository with whole sources takes 30 to 55 minutes on a CPU. Waiting for it
before the first answer is not acceptable.

## Decision

A large project first gets outline vectors (path, name, first lines; about 4 times cheaper) for the searched
folder, so it can be searched at once; the background process then computes whole-source vectors. Work is ordered:
the current search, recently changed files, the rest. Every answer reports how many functions were searched, how many
with whole sources, and what is pending.

## Alternatives considered

- Block the first search until indexing ends: minutes to an hour.
- A word search over unindexed code: needs glossaries for Spanish queries (rejected, ADR-0001).

## Consequences

- Measured on the exam: outlines only give top-1 135/201 against 168/201 with whole sources.
