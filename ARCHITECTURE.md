# Architecture

This document describes how s1grep is built today and the architecture it is moving to. Decisions and their reasons
live in [`docs/decisions/`](docs/decisions/README.md); quality gates in [`docs/quality.md`](docs/quality.md).

## What s1grep is

A local, general-purpose code search: you describe what code does, in English or Spanish, and s1grep returns the
functions that do it. An embedding model brings candidates; s1-code, a small decision model, reads the best of them and
reorders them. Everything runs on the user's CPU; nothing leaves the machine.

Languages: Python, JavaScript, TypeScript (and TSX), Go, Java, PHP, Rust, Ruby and C#. Python keeps the extractor its
training data and exams were built with; the others are read through one tree-sitter query each
(`crates/s1-index/src/queries/*.scm`, registered in `LanguageSettings`). s1-code v3 was trained on Python only:
measured on other languages it helps on Java and PHP, is neutral on Ruby and hurts on JavaScript and Go
([0000](docs/decisions/0000-engine-measurements.md#other-programming-languages)); a judge trained on several languages
(s1-code v4) is planned.

Principles that every change keeps:

- **General purpose.** No glossaries, term lists or rules for a framework, domain or country. Understanding languages
  is the models' job; everything else is generic ([ADR-0001](docs/decisions/0001-general-purpose.md)).
- **Local and private.** No accounts, no telemetry. Models are downloaded once, pinned by revision and SHA-256.
- **Answer now, improve later.** No search waits minutes; what is missing is indexed in the background and every
  answer says how much it covered.
- **Measured, not guessed.** A change to models, retrieval or fusion ships only after the exam and the benchmarks
  ([ADR-0011](docs/decisions/0011-quality-gates.md)).

## Crates

| Crate | Responsibility | Shipped |
|---|---|---|
| `s1-engine` | Runs ONNX models: the sentence embedder and the decision model (Laya format), with exact parity to the Python reference | yes |
| `s1-index` | Turns source files into code units (tree-sitter), walks repositories, stores units and vectors in SQLite, ranks and fuses | yes |
| `s1grep` | The product, as a library and the `s1grep` binary: search service, background process, MCP server, model installer, terminal output | yes |
| `s1-lab` | Development tools: the exam, judge-only reranking, benchmarks, raw decisions, unit dumps | no |

Each crate keeps its tunable values in its own `settings.rs`.

## A search today

```text
s1grep "query" PATH
  └─ SearchBackend ── ensure a background process (spawn `serve --background` if none) ──┐
                                                                                        │ TCP 127.0.0.1, token,
  SearchServer (one request at a time) ◄─────────────────────────────────────────────────┘ JSON lines, progress stream
  └─ SearchService::search
       ├─ Project::locate      root = highest ancestor with an index, else nearest .git, else PATH; scope = subfolder
       ├─ IndexLock            per-project file lock; if another process holds it, search what is indexed
       ├─ Indexer::scan        parallel walk (ignore files), size/mtime gate, blake3, tree-sitter → units
       ├─ few units missing    embed their whole source now
       ├─ many units missing   embed outlines (path, name, first lines) of the scope now, schedule the rest
       ├─ ProjectSession       the project's open catalog and its vectors in memory
       └─ Searcher::search     embed query → nearest 25 in the VectorIndex → read their sources → judge reads
                               the top 5 → rank fusion
```

The background process keeps up to four recently searched projects open (`ProjectSession`): the catalog connection
and a `VectorIndex`, all vectors of the project in one matrix, reloaded only when this process or another one changed
them (`PRAGMA data_version`). A search ranks that matrix, then reads the source of its 25 candidates only.

Between requests the server calls `SearchService::index_step`, which embeds short batches of the oldest indexing job:
first the outlines of the whole project, then whole sources. After each request it yields for a moment, so the search
that follows a ping never queues behind a batch. On start it resumes the jobs of projects searched in the last day.
The process exits after 30 idle minutes once no job is left.

## Data on disk

```text
~/.cache/s1grep/                     (%LOCALAPPDATA%\s1grep on Windows)
├── models/                          s1-code-v3-onnx, granite-278m-onnx
├── vectors.sqlite                   shared by every project: (embedding space, content key) → f32 vector
├── projects/<path fingerprint>/
│   ├── project.json                 the project's real path, creation and last use
│   ├── catalog.sqlite               files (size, mtime, hash) and units (path, name, lines, source)
│   ├── lock, lock.pid               held while a process indexes the project, and by whom
├── server.json, server.lock         the running background process: port, token, pid, version
└── server.log
~/.config/s1grep/ignore              global ignore rules, written with the defaults on first use
```

A unit's content key is blake3 of its name and source, so the same function in several projects is embedded once.
An embedding space key names the model, its revision, the dimension and the kind of text embedded, e.g.
`granite-embedding-278m-multilingual@b795cbc00b23/768d/whole-v1`. Both databases record their schema version in
`PRAGMA user_version` and migrate forward (`SchemaMigrations`); `s1grep gc` removes what nothing uses.

## Where it is going

The audit of 2026-10-01 found that each search does work proportional to the whole project (walk, reading every
vector from SQLite) instead of to what changed, that the background process has robustness gaps, and that adding a
language, a lexical index or a new embedding model would touch most modules. The target architecture:

```text
interfaces   CLI · MCP
daemon       versioned protocol · project sessions (indexes in memory) · indexing scheduler (priority queue)
s1-search    SearchPipeline: Retriever (dense, lexical, hybrid) → Judge → FusionPolicy
s1-index     ProjectCatalog (SQLite, migrations) · VectorCache (by EmbeddingSpace) · VectorIndex · LexicalIndex
s1-source    SourceWalker (parallel) · ChangeDetector → ChangeSet · ExtractorRegistry (tree-sitter per language)
s1-engine    TextEmbedder · DecisionModel
```

| Stage | Content | Release |
|---|---|---|
| 0 | Documentation, `s1-lab`, settings and naming hygiene, clippy and Windows tests in CI | 0.2.5 |
| 1 | Schema versions and migrations, `EmbeddingSpace`, one folder per project, `gc`, daemon robustness | 0.2.5 |
| 2 | Resident project sessions, in-memory vector index, change detection without full rescans | 0.2.5 |
| 3 | Faster embedding: smaller model, INT8, token cap, all gated by the exam | 0.3.0 |
| 4 | More languages through tree-sitter queries; BM25 with tantivy and rank fusion | 0.4.0 |
| 5 | Daemon concurrency, cancellation, typed errors, richer MCP, optional file watcher | 0.5.0 |
| 6 | s1-code v4 trained on several languages | model |

Every stage leaves the tool working and ends with its own gate (see [`docs/quality.md`](docs/quality.md)).
