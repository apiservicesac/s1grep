---
license: apache-2.0
base_model: convaiinnovations/laya
language:
- en
- es
library_name: laya
pipeline_tag: text-classification
tags:
- code-search
- reranker
- system-one
- decision-model
- python
---

# s1-code v3

**s1-code** is a small System One decision model specialised in **code relevance**: given a search written in English
or Spanish and one Python function, it returns a calibrated probability that the function is the code the search is
looking for. It is used as the judge of s1grep, a local code search tool: an embedding
model brings the candidates and s1-code reorders the first few.

It is fine-tuned from [convaiinnovations/laya](https://huggingface.co/convaiinnovations/laya) (multilingual checkpoint,
322M parameters, Apache 2.0) and runs on an ordinary CPU.

> **Resumen en español.** s1-code es un modelo System One pequeño (322M) que decide si una función de Python responde a
> una búsqueda escrita en inglés o en español. Corre en CPU. La v3 es la versión recomendada.

## How to use

```python
import laya

judge = laya.load("api-service-sac/s1-code-v3", device="cpu")
state = "billing/gateway/client.py\nGatewayClient.send_with_retry\n\n" + function_source[:1500]
answer = judge.system_one(state, {"match": {"type": "noul",
                                            "instructions": "This code answers the search: where do we retry sending invoices"}})
print(answer["answers"]["match"]["noul"])  # probability that the function answers the search
```

The model was trained with one exact input format, so use it as is:

- **State**: the file path, the function name, an empty line, and the function source (its first 1,500 characters):

  ```text
  billing/gateway/client.py
  GatewayClient.send_with_retry

  def send_with_retry(self, document, attempts=3):
      for attempt in range(attempts):
          response = self.post(document)
          ...
  ```

- **Question**: a noul (yes/no) question whose instructions are `This code answers the search: <your search>`.

The `onnx/` folder holds the exported graph that s1grep runs through ONNX Runtime (Rust).

## Results

New held-out test: 197 real questions written from commits of private repositories that were never used for training,
over functions that appear in no earlier exam. Each system ranks the same 25 candidates; the metric is how often the
right function comes first. McNemar tests compare systems question by question.

| System | Size | Top 1 (of 197) | English | Spanish |
|---|---|---|---|---|
| Qwen3-Reranker-4B (teacher) | 4B, GPU | 183 | 91 % | 95 % |
| Nimble (System One, choice over all candidates) | 9B, GPU | 177 | 90 % | 90 % |
| **s1-code v3 + Qwen3-Embedding (fused)** | 0.3B, CPU | **168** | **85 %** | **85 %** |
| s1-code v3 + granite-embedding (fused) | 0.3B, CPU | 165 | 83 % | 84 % |
| **s1-code v3 alone** | 0.3B, CPU | **161** | 82 % | 81 % |
| jina-reranker-v2-base-multilingual (non-commercial licence) | 0.28B, CPU | 158 | 78 % | 82 % |
| s1-code v2 alone | 0.3B, CPU | 157 | 80 % | 79 % |
| Qwen3-Embedding-0.6B alone | 0.6B | 152 | 76 % | 78 % |
| s1-code v1 alone | 0.3B, CPU | 144 | 68 % | 78 % |

- v3 alone beats v1 alone (p = 0.002) and Qwen3-Embedding alone; fused with Qwen3-Embedding it beats Qwen3-Embedding
  alone (p = 0.0015). Its gain over v2 is not statistically significant (p = 0.54 alone, p = 0.65 fused).
- The larger GPU models remain clearly better judges (teacher vs v3 fused: p = 0.013).
- Calibration: noul temperature 1.014, expected calibration error 2.5 %.

## Training

- 72,332 training groups (36,166 English and 36,166 Spanish questions): each question with its right function and 7
  negatives mined with Qwen3-Embedding inside the same repository; near-copies of the answer are never used as negatives.
- Sources: generated questions for functions of 306 public Python repositories with permissive or LGPL licences,
  commit messages from CommitPackFT (MIT, BSD, Apache and ISC repositories only), issues linked to the fixed function from
  SWE-bench, SWE-Gym and SWE-smith (MIT), and functions of privately owned repositories, filtered for secrets,
  personal data and licences. Spanish twins of every question were written with Qwen3-8B (Apache 2.0).
- 20,000 groups carry soft labels from Qwen3-Reranker-4B (Apache 2.0), mixed 50/50 with the real labels.
- Loss: per-candidate cross-entropy plus a listwise loss over each group. One epoch planned; validation on held-out
  repositories every 1,000 steps; the best step (13,000) was kept after early stopping at 17,000.

## Limitations

- Python only. Judge quality depends on the retriever bringing the right function among the candidates.
- It is a specialist: on general typed decisions unrelated to code (Laya's typed-decisions benchmark) it scores 27 %,
  below the base Laya checkpoint (35 %). Use base Laya for other tasks.
- Scoring 25 candidates on a CPU takes about 2.5 s; s1grep judges only the first 5 to 10.

## Licence and attribution

Apache 2.0. Fine-tuned from Laya by Convai Innovations (Apache 2.0). Teacher: Qwen3-Reranker-4B (Apache 2.0). The
private training data is not released.
