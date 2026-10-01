# 0011. Quality gates for every release

- Status: Accepted (2026-10-01)
- Implementation: Stage 0 (CI); exam and benchmarks on every release.

## Context

Retrieval and model changes are easy to judge by feel and hard to judge correctly; the 201-question exam has already
shown that intuitive changes can lose answers.

## Decision

A release passes the gates in docs/quality.md: the exam (no loss beyond noise), indexing and search benchmarks,
unit tests on Linux and Windows, clippy with no warnings, and model parity when a model changes. Results go into the
CHANGELOG entry.

## Alternatives considered

- Spot checks by hand: unrepeatable.

## Consequences

- Releases take longer; regressions are caught before users see them.
