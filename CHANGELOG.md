# Changelog

All notable changes to `s1grep` are documented here. Versions follow [Semantic Versioning](https://semver.org).

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
