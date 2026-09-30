# s1grep

Find code by asking what it does, in English or Spanish. s1grep runs locally: your code never leaves the machine.

```text
$ s1grep search "where do we retry a failed payment" ~/work/shop
 1. ~/work/shop/billing/gateway/client.py:41-58  GatewayClient.send_with_retry   [judge  93% · similarity 0.71]
      def send_with_retry(self, document, attempts=3):
          for attempt in range(attempts):
          ...
```

Status: v0.1, early. Python repositories only; Linux (x86-64, glibc 2.38 or newer: Ubuntu 24.04, Debian 13) and
Windows (x86-64).

## Install

1. Download the archive for your system from the [releases](https://github.com/apiservicesac/s1grep/releases) and unpack
   it. On Windows keep `onnxruntime.dll` next to `s1grep.exe`.
2. Download the models once (about 2.4 GB, checked with SHA-256):

   ```sh
   s1grep models download
   s1grep models status
   ```
3. Search:

   ```sh
   s1grep search "where do we retry a failed payment" path/to/repo
   ```

## How it works

1. **Index.** tree-sitter splits every Python file into functions and methods. An embedding model turns each one into a
   vector, stored in a SQLite index under your cache folder, never inside the repository. Only changed files are re-read.
2. **Retrieve.** The search is embedded with the same model and the 25 nearest functions become candidates.
3. **Judge.** [s1-code](https://huggingface.co/api-service-sac/s1-code-v3), a small System One decision model
   (322M parameters, fine-tuned from Laya), reads the first candidates and answers one typed question per candidate:
   *does this code answer the search?* with a calibrated probability.
4. **Fuse.** The judge's order and the retriever's order are combined with weighted rank fusion; the weights were
   chosen on a development exam, never on the test.

Everything runs on the CPU through ONNX Runtime.

The retriever is granite-embedding-278m-multilingual, and the judge reads the first 5 candidates by default. Reading
10 (`--judge-top 10`) takes twice as long for about 4 more right answers in 100 on the development exam.

On a held-out test of 201 real searches (103 in English, 98 in Spanish), s1grep puts the right function first 168
times against 145 for the embeddings alone, with a median search of 1.5 s on an 8-core CPU
(see [docs/decisions.md](docs/decisions.md)).

## Usage

```sh
s1grep index ~/work/shop                                    # optional: search indexes on the fly
s1grep search "validate the token expiry" ~/work/shop
s1grep search "dónde se valida que el token no haya expirado" ~/work/shop -n 10
s1grep search "export rows to csv" . --json                 # machine-readable, for agents
s1grep search "export rows to csv" . --no-judge             # embeddings only
s1grep units ~/work/shop                                    # the functions the index stores, as JSON lines
```

Options: `--judge-top N`, `--include-tests` (tests/ and migrations/ are skipped by
default), `--threads N`, `--models <folder>`.

`s1grep eval --exam <folder> --repos <folder>` runs an exam (one `<repository>.json` per repository with `text`,
`language`, `path`, `function` and optional `also_accept`) through the full pipeline and reports top-1 and top-5 per
language plus the median search time.

## Models

`s1grep models download` puts the model bundles in `~/.cache/s1grep/models` (`%LOCALAPPDATA%\s1grep\models` on
Windows); `--models` or `$S1GREP_MODELS` point elsewhere.

| Bundle | Hugging Face repository |
|---|---|
| `s1-code-v3-onnx` | [api-service-sac/s1-code-v3](https://huggingface.co/api-service-sac/s1-code-v3), folder `onnx/` |
| `granite-278m-onnx` | [api-service-sac/granite-embedding-278m-multilingual-onnx](https://huggingface.co/api-service-sac/granite-embedding-278m-multilingual-onnx) |

Both can also be rebuilt from the original checkpoints with `tools/model-export`.

## Layout

| Path | Contents |
|---|---|
| `crates/s1-engine` | Typed questions, Laya sequence encoding, ONNX Runtime sessions, calibrated answers, embedders |
| `crates/s1-index` | Python function extraction, repository walking, the SQLite index, vector ranking and rank fusion |
| `crates/s1grep` | Command line: `search`, `index`, `models`, `eval`, `rerank-eval`, `units`, `bench`, `decide` |
| `tools/model-export` | Development only: exports models to ONNX and records parity fixtures from Python |
| `docs/decisions.md` | Measured decisions (export, precision, latency budget) |
| `docs/model-cards` | The Hugging Face cards of s1-code v1, v2 and v3 |

## Development

Everything runs in Docker through `./dev.sh`; nothing is installed on the host.

```sh
./dev.sh export uv sync                                        # Python environment of the export tool
./dev.sh export python -m model_export export                  # models/laya-multilingual (FP32 ONNX bundle)
./dev.sh export python -m model_export fixtures                # parity fixtures from the Python runtimes
./dev.sh cargo test --release                                  # unit and parity tests
./dev.sh cargo build --release -p s1grep                       # target/release/s1grep
./dev.sh windows build --release -p s1grep                     # s1grep.exe
```

The parity tests check that the Rust engine reproduces the Python runtimes: token for token for Laya sequences,
within 6e-4 for answers, and cosine 0.999 or more for embeddings.

## License

Apache 2.0. Laya is Apache 2.0 by Convai Innovations; granite-embedding is Apache 2.0 by IBM.
