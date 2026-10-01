# 0005. Versioned schema with migrations

- Status: Accepted (2026-10-01)
- Implementation: Stage 1.

## Context

The index schema is created with CREATE TABLE IF NOT EXISTS and has no version. Any change to a table would meet
old files on users' machines without a way to tell them apart.

## Decision

Each database records its schema in PRAGMA user_version and is brought up to date by ordered migrations. A version
newer than the binary knows, or a failed migration, rebuilds that project's index instead of guessing.

## Alternatives considered

- Delete indexes on every release: forces a full re-index after each update.

## Consequences

- Every schema change ships with its migration and a test that opens the previous version.
