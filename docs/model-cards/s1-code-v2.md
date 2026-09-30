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

# s1-code v2

Earlier version of [s1-code](https://huggingface.co/api-service-sac/s1-code-v3). **For use, prefer v3.** A System One
decision model (322M, CPU) that returns the probability that a Python function answers a search in English or Spanish.
Fine-tuned from [convaiinnovations/laya](https://huggingface.co/convaiinnovations/laya) (Apache 2.0).

Same input format as v3: the state is the file path, the function name, an empty line and the first 1,500 characters
of the source; the question is the noul `This code answers the search: <your search>`.

## Results (new held-out test, 197 questions, 25 candidates from Qwen3-Embedding)

| System | Top 1 | English | Spanish |
|---|---|---|---|
| s1-code v2 alone | 157 | 80 % | 79 % |
| s1-code v2 + Qwen3-Embedding (fused) | 165 | 83 % | 84 % |
| Qwen3-Embedding alone | 152 | 76 % | 78 % |

## Training

About 81,000 questions with exact English/Spanish parity from 306 public repositories, commit messages (CommitPackFT),
issues (SWE-bench, SWE-Gym, SWE-smith), CoSQA and CoSQA+ searches, and filtered private functions; one positive and three
granite-mined negatives per question; checkpoint taken after the first of two epochs (the second overfitted).

**Known issue:** the CoSQA+ part of its data had false negatives (generated code that also answers the query). They were
removed for v3.

## Licence and attribution

Apache 2.0. Fine-tuned from Laya by Convai Innovations. Training data includes CoSQA+ (CC-BY-4.0, Gong et al.) and CoSQA
(MIT). Private training data is not released.
