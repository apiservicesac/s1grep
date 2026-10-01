# 0010. Development tools live in s1-lab, outside the shipped binary

- Status: Accepted (2026-10-01)
- Implementation: Implemented in stage 0.

## Context

eval, rerank-eval, bench, decide and units were hidden commands of the shipped binary, and eval wrote into the
user's real cache.

## Decision

They live in the s1-lab crate, which is never released. s1grep is a library plus its binary, so s1-lab reuses the
same code. The exam refuses to run unless XDG_CACHE_HOME points at a lab folder.

## Alternatives considered

- Keep them hidden in s1grep: larger binary, user-visible surface that is not supported.

## Consequences

- `./dev.sh cargo run --release -p s1-lab -- eval …` replaces `s1grep eval …`.
