# Changelog

All notable changes to `s1grep` are documented here. Versions follow [Semantic Versioning](https://semver.org).

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
