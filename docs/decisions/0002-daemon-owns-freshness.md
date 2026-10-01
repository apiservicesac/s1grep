# 0002. The background process owns freshness and in-memory indexes

- Status: Accepted (2026-10-01)
- Implementation: done in stage 2 (0.2.5); a file watcher comes in stage 5.

## Context

Every search walks the whole project, compares each file with the index and then reads every unit and vector from
SQLite (about 70 MB decoded for 16,000 functions) to use 25 of them. ck and claude-context share this problem; tools
that stay fast at scale keep the index resident and process only changes.

## Decision

The background process keeps one session per recently used project: an open catalog, the vector matrix in memory
and the file states. A search uses them directly. Changes are found by a cheap stat walk (later a file watcher) and
applied as a change set; sources are read only for the final candidates.

## Alternatives considered

- Keep scanning per search and only optimise queries: still O(project) per search.
- Make the CLI search without the daemon: pays the 10 s model load on every search.

## Consequences

- Warm searches cost the query embedding, a matrix product and the judge.
- Memory grows with the projects kept open; sessions are evicted least-recently-used.
