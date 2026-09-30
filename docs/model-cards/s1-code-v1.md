---
license: apache-2.0
base_model: convaiinnovations/laya
language:
- en
- es
library_name: laya
tags:
- code-search
- reranker
- system-one
- python
---

# s1-code v1

First version of [s1-code](https://huggingface.co/api-service-sac/s1-code-v3). **For use, prefer v3.** A System One
decision model (322M, CPU) that returns the probability that a Python function answers a search.
Fine-tuned from [convaiinnovations/laya](https://huggingface.co/convaiinnovations/laya) (Apache 2.0).

Same input format as v3: the state is the file path, the function name, an empty line and the first 1,500 characters
of the source; the question is the noul `This code answers the search: <your search>`.

## Results (new held-out test, 197 questions, 25 candidates from Qwen3-Embedding)

| System | Top 1 | English | Spanish |
|---|---|---|---|
| s1-code v1 alone | 144 | 68 % | 78 % |
| s1-code v1 + Qwen3-Embedding (fused) | 167 | 79 % | 91 % |
| Qwen3-Embedding alone | 152 | 76 % | 78 % |

Trained on 30,068 generated questions (59 % Spanish, 41 % English) for functions of 306 public Python repositories with
permissive licences, with granite-mined negatives. Stronger in Spanish than in English, which v2 and v3 corrected.

## Licence

Apache 2.0. Fine-tuned from Laya by Convai Innovations.
