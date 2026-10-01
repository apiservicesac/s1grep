# Changelog

All notable changes to `s1grep` are documented here. Versions follow [Semantic Versioning](https://semver.org).

## [0.4.0] — 2026-10-01

### Added
- A word index per project (tantivy, BM25) with a tokenizer for code: it splits `camelCase`, `snake_case`, acronyms
  and digits, folds accents, and knows no human language. Searches written like code (`parse_config`,
  `sendInvoice()`, `client.retry`) use every word; other searches use it only for exact phrases, so pasting an error
  message finds the function that contains it. On the exam: questions unchanged (168/189), function names 151/183
  instead of 149/180, quoted text 63/77 instead of 52/66. Results found only by words are marked `by words`.
- `s1-lab eval --queries identifier|literal` builds those query sets from the exam's answers, and `--details` writes
  one line per question.

### Fixed
- An ignore file holding an unpublished development version of the defaults is now brought up to date like the
  published ones.

## [0.3.0] — 2026-10-01

### Changed
- Outlines, the quick first pass over a large repository, are embedded by granite-embedding-97m-multilingual-r2, about
  three times faster than the main retriever with nearly the same answers (top-1 133 instead of 135 on outlines
  alone); whole sources keep the main retriever, so a fully indexed project is searched exactly as before (168/189).
  A 25,000-function repository becomes fully searchable in about 7 minutes instead of more than 20. `s1grep setup`
  downloads the new model (about 400 MB more).
- The judge stops reading candidates once one scores at least 0.9: a third fewer judge runs per search (3.3 instead
  of 5) with the same exam results.

### Added
- When less than half of a project can be searched yet, the search says so plainly, with the time until all of it
  can be searched.
- The background process writes a line to its log when it stops.

### Fixed
- A search from a folder that holds many projects (a home or workspace folder) no longer takes over the projects
  inside it: within a git repository the project is never a folder above the repository, and outside one it is the
  nearest indexed folder. Before, one search from such a folder made every later search re-read all of it.
- Outline vectors use the first four lines of each function again, as measured: since 0.2.4 they used two, which
  found the right function first 129 times out of 201 instead of 135 (top-5: 149 instead of 167). Outline vectors are
  recomputed once in the background.

## [0.2.6] — 2026-10-01

### Added
- JavaScript, TypeScript (with TSX), Go, Java, PHP, Rust, Ruby and C#, each read through a tree-sitter query
  (`docs/languages.md`). Python keeps its extractor, so its results do not change. Each file records the extractor
  version that read it, and a file is read again only when its language's extractor changes.
- Default ignore rules for the test, build and generated files of those ecosystems (`test/`, `__tests__/`,
  `*.test.*`, `*.spec.*`, `*_test.go`, `target/`, `obj/`, `*.min.js`, `*.bundle.js`). An ignore file that still holds
  the defaults of an earlier version is updated; an edited one is left alone.

## [0.2.5] — 2026-10-01

### Added
- `s1grep gc` removes the indexes of folders that no longer exist and the vectors no index uses (`--dry-run` shows
  what it would remove).
- Indexes carry a schema version and migrate themselves; one written by a newer s1grep is never rewritten by an older
  one.
- Every vector is stored under its embedding space (model, revision, dimension and kind of text), so a model or text
  change never mixes old and new vectors.
- `ARCHITECTURE.md`, architecture decision records in `docs/decisions/`, `docs/quality.md` and `CONTRIBUTING.md`.
- CI runs clippy (warnings fail) and the unit tests on Windows.
- The README shows how the judge, trained on Python, does on other languages.

### Changed
- The background process keeps recently searched projects open with their vectors in memory and reads the source of
  the final candidates only; files are walked in parallel (5,900 files: 160 ms to 23 ms). A warm search now spends
  0.2–0.9 s outside the models, most of a search being the judge.
- A search no longer waits behind background indexing: the process yields for a moment after each request, and its
  batches are shorter. In a 16,000-function repository while indexing, searches went from 4.2 s to 2.0–2.7 s.
- A starting background process resumes indexing the projects searched in the last 24 hours that are not complete.
- Each project's index lives in its own folder, `projects/<key>/` with `project.json` (the real path and last use),
  `catalog.sqlite` and its lock. Indexes and vectors of earlier versions move there on first use, without re-indexing.
- The first search in a large project waits at most about 10 s for outline vectors and answers with what is ready,
  saying how many functions are not searchable yet; later searches answer at once while the background process
  continues. In a 16,000-function repository the first search went from 514 s to 18 s, the next ones take 4 s.
- `--exclude` hides results of one search instead of removing those files from the shared index; `s1grep index` no
  longer takes it.
- The development commands (`eval`, `rerank-eval`, `bench`, `decide`, `units`) moved out of the shipped binary into
  the `s1-lab` crate, which is never released. The exam refuses to run against the real cache.
- Internal clean-up with no change in behaviour: every tunable value in its crate's settings, one SQL scope filter,
  dead code removed, `decide` built on `decide_batch`.

### Fixed
- Two searches starting at once no longer load the models twice or fail with "already running": the background
  process takes its lock before loading the models, and a search waits for a process that is still starting.
- `server.json` is written atomically, and a stale one is removed when a new background process starts.
- A background indexing step that keeps failing pauses between tries and is dropped after three, instead of spinning.
- `s1grep status` no longer takes the indexing lock nor overwrites the process id of the process holding it.
- A file that disappears or becomes unreadable during a scan is skipped with a notice instead of failing the search.
- Functions shared with another folder are indexed when a subfolder is searched, and pending work outside the searched
  folder is handed to the background process.
- The number of judged candidates is reported as the number actually judged.
- `--help` and `--version` work without the ONNX Runtime library; Windows paths no longer show the `\\?\` prefix.

## [0.2.4] — 2026-10-01

### Added
- Large projects are indexed in two passes. First every function gets a vector of its outline (path, name and
  signature), about a tenth of the work, so the whole project can be searched after a short wait, in any language the
  retriever knows. Then the background process computes the vectors of the whole source between searches, and results
  improve as it goes. Results found through an outline are marked `by outline`.

### Changed
- Reading a project writes to the index in batches of 500 files and loads every file's state in one query: reading
  about 6,000 files after the first time takes a few seconds instead of half a minute.

### Fixed
- `s1grep stop`, `status` and searches no longer hang while the background process is busy: it is pinged with a timeout,
  and `stop` ends it by force when it does not answer.
- The time left for background indexing is measured from its first batch, once enough functions are done, instead of
  guessing hours from the first seconds.

## [0.2.3] — 2026-10-01

### Changed
- Animations are drawn with indicatif: a spinner while the models load, bars while files are read, functions are
  indexed (with speed and time left) and models are downloaded (bytes, speed and time left).
- The background process starts in its own session, detached from the terminal: closing the terminal that started it
  no longer stops it.
- Every tunable value lives in one settings module per crate (`s1grep`, `s1-index`, `s1-engine`); colours use the
  console crate.

## [0.2.2] — 2026-10-01

### Added
- An animated line while the models load, with the seconds elapsed; plain text when the output is not a terminal.

### Changed
- `s1grep setup` downloads models pinned to one Hugging Face commit, with the size and SHA-256 of every file built into
  s1grep: no API calls, and every install gets the same bytes.
- Downloads wait and retry when Hugging Face is busy (429, honouring Retry-After) or fails for a moment (5xx, network),
  up to six attempts, saying how long they wait.

## [0.2.1] — 2026-10-01

### Fixed
- A search no longer waits up to three minutes when the models are missing or the background process cannot start: it
  stops at once and says why (for missing models, to run `s1grep setup`).
- `install.sh` reads the glibc version correctly (it rejected 2.39 as older than 2.38) and no longer prints
  `work: unbound variable` when it finishes.

## [0.2.0] — 2026-10-01

### Added
- The first search starts a background process that keeps the models in memory: later searches take about a second,
  with no second terminal. It stops on its own after 30 minutes without searches; `s1grep stop` stops it now.
- Progress while indexing: a live bar with the functions done, the speed and the time left, also for searches answered
  by the background process.
- `s1grep status`: the background process, the models and how far each project is indexed.
- Ignore files decide what is read: `~/.config/s1grep/ignore` for every project (created with the defaults on first
  use) and a `.s1grepignore` in any folder; `--exclude` adds a pattern for one search.

- Installers: `install.sh` and `update.sh` for Linux, `install.ps1` and `update.ps1` for Windows. They check every download against the
  release's `SHA256SUMS` and prove the new binary runs before replacing anything.

### Changed
- One index per project: searching a subfolder reuses the index of its project and is indexed first.
- Vectors are stored by the content of each function and shared between projects, so a framework copied into several
  projects is embedded once.
- Only files whose size or date changed are read again.
- Only one process indexes a project at a time; another search uses what is already indexed and says so.

### Removed
- `--include-tests`: bring tests back with `!tests/` in an ignore file.

## [0.1.0] — 2026-10-01 (internal build, not published)

### Added
- Search Python repositories by what the code does, in English or Spanish: granite embeddings find 25 candidates and
  s1-code v3 judges the first 5. 168 of 201 real searches put the right function first, against 145 for the
  embeddings alone.
- `setup` downloads the models from Hugging Face and checks them with SHA-256; `doctor` checks the installation.
- `mcp` serves a `search_code` tool to coding agents; `skill --install` adds a Claude Code skill.
- Linux (x86-64, glibc 2.38 or newer) and Windows (x86-64) builds.
