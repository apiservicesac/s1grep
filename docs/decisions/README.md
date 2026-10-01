# Decisions

Architecture decision records: why s1grep is built the way it is. A decision changes only through a new record that
supersedes it. [0000](0000-engine-measurements.md) collects the engine measurements behind the model settings.

| Record | Decision | Implementation |
|---|---|---|
| [0001](0001-general-purpose.md) | General purpose: nothing specific to a domain or a language | Implemented (the glossary was removed in 0.2.4). |
| [0002](0002-daemon-owns-freshness.md) | The background process owns freshness and in-memory indexes | done in stage 2 (0.2.5); a file watcher comes in stage 5. |
| [0003](0003-sqlite-and-flat-vectors.md) | SQLite as the source of truth, a flat vector matrix in memory | done in stage 2 (0.2.5). |
| [0004](0004-embedding-space.md) | Every vector is keyed by its embedding space | done in stage 1 (0.2.5). |
| [0005](0005-schema-migrations.md) | Versioned schema with migrations | done in stage 1 (0.2.5). |
| [0006](0006-languages-as-data.md) | Languages are added as data: a grammar, a query and a registry entry | done (0.2.6): eight languages beside Python. |
| [0007](0007-hybrid-retrieval.md) | Hybrid retrieval: vectors and BM25, fused, then the judge | Stage 4. |
| [0008](0008-progressive-indexing.md) | Progressive indexing with priorities and reported coverage | outline pass in 0.2.4; bounded first wait, yielding to searches and resuming on start in 0.2.5. |
| [0009](0009-ipc-protocol.md) | A versioned IPC protocol with typed errors and cancellation | Stage 5 (atomic server info and lock order in stage 1). |
| [0010](0010-lab-outside-the-product.md) | Development tools live in s1-lab, outside the shipped binary | Implemented in stage 0. |
| [0011](0011-quality-gates.md) | Quality gates for every release | Stage 0 (CI); exam and benchmarks on every release. |
| [0012](0012-storage-layout.md) | Storage: SQLite, one index per project, in its own folder | done in stage 1 (0.2.5). |
| [0013](0013-outline-model.md) | A smaller model embeds outlines | done (0.3.0). |
