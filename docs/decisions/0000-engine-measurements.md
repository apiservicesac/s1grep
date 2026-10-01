# 0000. Engine measurements

Measured on an 8-core desktop CPU (AVX2, no VNNI) with Laya multilingual (mmBERT, 322M parameters).

## Laya is exported with the dynamo exporter

The upstream `scripts/export_onnx.py` traces with TorchScript, which freezes the decision head's reshape at the
sample length (16 tokens); every longer input fails in ONNX Runtime. `model_export export` uses
`torch.onnx.export(dynamo=True)` with dynamic batch, sequence and marker axes. The FP32 graph matches PyTorch on
every fixture to four decimals, including 1024-token inputs.

## FP32, not INT8

- Dynamic INT8 (weights and activations) collapses every answer to ~0.5: mmBERT activations have large outliers,
  and the quantizer also rewrites the rotary-position MatMul (positions x inverse frequencies).
- Weight-only INT8 (`MatMulNBits`) keeps answers within 0.04-0.08 but runs no faster than FP32 on AVX2.
- Weight-only INT4 drifts by up to 0.87.

An INT8 weight-only bundle (MatMulNBits, 8 bits, block 32, accuracy level 4) drifted at most 0.017 on the parity
fixtures with no flipped decision. On a laptop CPU with AVX-VNNI (Windows) it was still slower than FP32: 224-233 ms versus
168-170 ms per 128-token fragment. INT8 is dropped.

## Latency budget

On that laptop: 168 ms per 128-token fragment with 6 threads, 148 ms per fragment in batches of 8 with
10 threads, 359 ms at 256 tokens. On the 8-core desktop: about 90 ms per 128-token fragment with 6-8 threads, 220 ms at 256 tokens and 560 ms at 512. Batching does not
reduce the per-fragment cost on CPU. A query can afford roughly 25 model decisions, so candidates are short units
chosen by a cheap prefilter; the model never scans a repository.

## The judge reads 5 candidates, one at a time, at 384 tokens

Measured with `s1-lab rerank-eval` on the 100-question development exam (granite candidates, 8 threads, the 8-core
desktop). Top-1 after fusion; granite alone gets 66.

| Tokens read | Candidates | Judge time per search | Top-1 |
|---|---|---|---|
| 384 | 10 | 3.8 s | 78 |
| 384 | 5 | 1.9 s | 74 |
| 256 | 5 | 1.1 s | 70 |
| 192 | 5 | 0.75 s | 66 |
| 128 | 5 | 0.48 s | 55 |

- s1-code v3 was trained on 384-token inputs; cutting them costs more accuracy than it saves time.
- 16 threads are no faster than 8.
- Judging candidates one per run instead of in one padded batch gives identical answers 25 % faster: 1.45 s for 5
  candidates and 2.9 s for 10.
- The default is 5 candidates; `--judge-top 10` trades twice the time for about 4 more right answers in 100.

## The judge stops early at 0.9

The judge reads candidates in the retriever's order and stops once one scores at least 0.9. Measured with
`s1-lab eval` on the 201-question exam (granite candidates, up to 5 judged):

| Setting | Top-1 | Top-5 | Candidates judged per search |
|---|---|---|---|
| Read all 5 | 168 | 189 | 5.00 |
| Stop at 0.95 | 168 | 189 | 4.06 |
| Stop at 0.9 | 168 | 189 | 3.33 |
| Stop at 0.8 | 166 | 189 | 2.82 |
| Read 3 | 165 | 189 | 3.00 |

Stopping at 0.9 removes a third of the judge's work with the same answers; reading only 3 does about as much work
and loses 3. Running two judge sessions at once was also tried and is slower on this CPU (1,060 ms per candidate
instead of 390 ms): the judge is limited by memory bandwidth, not by cores, which is also why more than 4 threads do
not help.

## Faster embedding: what was tried for 0.3.0

Measured on the 201-question exam and with `s1-lab embed-bench` (the same 400 functions, an 8-core AVX2 CPU without
VNNI). Speed is relative to granite-embedding-278m-multilingual in FP32.

| Retriever | Speed, whole / outline | Top-1 | Top-5 |
|---|---|---|---|
| granite 278M, FP32 (shipped) | 1× / 1× | 168 | 189 |
| granite-embedding-97m-multilingual-r2 | 2.3× / 3.3× | 154 | 178 |
| granite 278M, dynamic INT8 per channel | 1.25× | 147 | 175 |
| granite 278M, dynamic INT8, MatMul only, reduced range | 1.3× | 163 | 188 |

None keeps the exam, so the retriever stays as it is. Published benchmark scores did not carry over to these code
searches. More threads (16 instead of 8) add 5–10 %, larger batches are slower, and a lower token cap saves little:
the median function has about 160 tokens, so a 384-token cap still does 90–94 % of the work (`s1-lab lengths`).

## Outlines keep four lines

| Outline | Speed | Top-1 | Top-5 |
|---|---|---|---|
| Path, name and the first 4 lines (320 characters) | 1× | 135 | 167 |
| Path, name and the first 2 lines (160 characters) | 1.05× | 129 | 149 |

The path and the name are most of an outline's tokens, so shorter outlines are barely faster and clearly worse.

## v0.1 on the held-out test

`s1-lab eval` with the shipped settings (granite, the judge reads 5 candidates) on the held-out test of 201 real
searches, whose settings were chosen on the development exam only:

| | English (103) | Spanish (98) | All (201) |
|---|---|---|---|
| granite alone, top-1 | 71 | 74 | 145 (72 %) |
| s1grep, top-1 | 85 | 83 | 168 (84 %) |
| s1grep, top-5 | 96 | 93 | 189 (94 %) |

Median search time 1.46 s on the 8-core desktop, model loading excluded.

## Other programming languages

s1-code v3 was trained on Python only. On 100 CodeSearchNet queries per language (the first docstring sentence;
25 granite candidates from the same repository; the judge reads 10), top-1 changed as follows:

| Language | granite alone | with the judge |
|---|---|---|
| Python | 91 | 95 |
| Java | 58 | 68 |
| PHP | 79 | 82 |
| Ruby | 53 | 54 |
| JavaScript | 67 | 59 |
| Go | 47 | 41 |

The judge helps on Java and PHP, is neutral on Ruby and hurts on JavaScript and Go, so those languages should be
searched without it until a model is trained on them. With 100 queries each difference carries about ±9 points.

## Toolchain

The prebuilt ONNX Runtime linked by `ort` needs glibc 2.38 or newer, so builds run on Debian trixie. Portable
release binaries are addressed with distribution (milestone 5).
