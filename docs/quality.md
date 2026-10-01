# Quality gates

What a change must pass before it ships ([ADR-0011](decisions/0011-quality-gates.md)). Results of the exam and the
benchmarks go into the release's CHANGELOG entry.

| Gate | Threshold | When |
|---|---|---|
| Format | `make fmt-check` passes | every push (CI) |
| Clippy | `make lint` with no warnings | every push (CI) |
| Unit tests | `make test` on Linux; `cargo test` on Windows | every push (CI) |
| Model parity | `make test-parity`: tokens identical, answers within 6e-4, embeddings cosine ≥ 0.999 | a model or engine change |
| Exam | top-1 ≥ 165 and top-5 ≥ 187 of 201; English/Spanish gap no wider; McNemar against the previous release when a model changes | a change to models, extraction, retrieval or fusion |
| Indexing benchmark | functions per second on a fixed sample, no loss over 10 % | every release |
| Search benchmark | first search in a 16,000-function project under 20 s; a warm search spends under 0.5 s outside the models (the judge takes 1.5–2 s on an 8-core CPU) | every release from 0.2.5 |
| Robustness | two searches at once, the daemon killed while indexing, a file deleted during a scan | daemon or index changes |

## The exam

201 real searches (103 in English, 98 in Spanish) over 13 Python repositories, each with the function that answers
it. It is a regression gate: settings are never tuned on it. Tuning uses the separate development split.

The exam's questions and repositories are private and are not in this repository. With them in place:

```sh
XDG_CACHE_HOME=/tmp/s1-lab-cache ./dev.sh cargo run --release -p s1-lab -- eval \
  --exam <exam folder> --repos <repositories folder> --models models
```

`--outline` scores the outline vectors alone, the way a project is searched before its whole sources are indexed.

Reference results (granite 278M, s1-code v3, the judge reads 5):

| Setting | top-1 | top-5 |
|---|---|---|
| Whole sources | 168 | 189 |
| Outlines only (granite 97M-r2, the shipped outline model) | 133 | 166 |
| Function names as queries (`--queries identifier`, 197) | 151 | 183 |
| Quoted text from the answer as queries (`--queries literal`, 84) | 63 | 77 |

## Benchmarks

`s1-lab profile <project>` times the parts of a search that do not use the models on an existing index (walk, scan,
loading the vectors, ranking). `s1-lab bench` times the judge at several thread counts and lengths. The indexing and search benchmarks are run on the
same machine before and after a change; record the CPU, the thread count and the project in the CHANGELOG.
