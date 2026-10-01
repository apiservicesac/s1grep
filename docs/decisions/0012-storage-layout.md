# 0012. Storage: SQLite, one index per project, in its own folder

- Status: Accepted (2026-10-01)
- Implementation: Stage 1.

## Context

Each project's index is a loose `<hash>.sqlite` and `<hash>.lock` in one folder, so it is hard to tell which file
belongs to which project, and indexes of deleted folders stay forever. The question of replacing SQLite came up.

## Decision

Keep SQLite: no server, crash-safe with WAL, the same on every platform, inspectable, with migrations. The bottleneck
is the embedding model, not the database. Keep one index per project, so that indexing one project never blocks
searches in another and a damaged index affects one project. Move each project into its own folder
(`project.json` with the real path, schema version and last use; `catalog.sqlite`; vector matrices; later the
lexical index; `lock`). Keep the vector cache shared across projects. `s1grep gc` removes indexes of folders that no
longer exist and orphaned vectors; `status` shows real paths.

## Alternatives considered

- etcd: a distributed store for clusters that needs a server.
- redb or LMDB: no SQL or migrations, for an unnoticeable gain.
- LanceDB or DuckDB: heavier than needed.
- One global database: indexing one project would block searches in all others.

## Consequences

- A one-time migration moves existing indexes into the new layout.
