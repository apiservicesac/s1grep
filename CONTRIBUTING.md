# Contributing

## Before you start

Read [ARCHITECTURE.md](ARCHITECTURE.md) and the [decisions](docs/decisions/README.md). A change that goes against a
decision needs a new decision record first.

## Building

Everything runs in Docker through `./dev.sh` and `make`; nothing else is installed on the host.

```sh
make build          # target/release/s1grep
make windows        # cross-compiled s1grep.exe
make test           # unit tests
make lint           # clippy, warnings fail
make fmt            # format
make ci             # format check, clippy and tests, as CI runs them
```

Development tools are in `s1-lab` (`./dev.sh cargo run --release -p s1-lab -- --help`); they are never released.

## Code style

- Everything in the repository is in English: code, comments, docs.
- Types own behaviour: no free functions; one responsibility per type.
- Descriptive names: no single-letter variables, no leading underscores.
- Every tunable value lives in its crate's `settings.rs`, with a comment saying what it controls.
- Comments explain why, not what; keep them few.
- Use established libraries (indicatif, console, ignore, tree-sitter) rather than hand-written equivalents.

## Quality

A change passes the gates in [docs/quality.md](docs/quality.md) that apply to it. Changes to models, extraction,
retrieval or fusion include exam results in the pull request.

## Releasing

1. Describe the release in `CHANGELOG.md` under `## [X.Y.Z]`, with the exam and benchmark results.
2. `make bump-patch`, `make bump-minor`, `make bump-major` or `make bump VERSION=X.Y.Z` sets the version, commits,
   tags and pushes.
3. The Release workflow builds the Linux and Windows binaries and publishes them with the install and update scripts.
