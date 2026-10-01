# 0007. Hybrid retrieval: vectors and BM25, fused, then the judge

- Status: Accepted (2026-10-01)
- Implementation: done (0.4.0).

## Context

Embeddings find meaning and cross the language gap; they are weaker on exact identifiers, error strings and paths.
ck, osgrep and grepai fuse a lexical and a vector list with reciprocal rank fusion (k = 60).

## Decision

A tantivy index per project (`projects/<key>/lexical/`) holds each function's name, path and code. A tokenizer for
code splits `camelCase`, `snake_case`, acronyms and digits, keeps the whole identifier too, and folds accents; it has
no stemming and no stopwords, so it knows no human language. The index is updated with the files each scan changed and
rebuilt when it does not match the catalog.

The word list joins the vector lists in the reciprocal-rank merge only where it helps, decided by the shape of the
query alone (`QueryShape`):

- a query written like code (`snake_case`, `camelCase` starting in lower case, `module.function`, `f()`,
  `Type::method`, a path with an extension, quotes or backticks) searches every word;
- any other query is prose and only matches functions that contain it word for word, such as a pasted error message.

## Measurements

The 201-question exam, plus two query sets built from its answers by `s1-lab eval --queries`: the answer's function
name, and the longest quoted text of three or more words in it. Top-1 / top-5.

| Queries | Vectors only | Every word, every query (weight 1.0 / 0.5 / 0.25) | Shipped rule |
|---|---|---|---|
| The questions (201) | 168 / 189 | 145 / 173, 150 / 175, 155 / 179 | 168 / 189 |
| Function names (197) | 149 / 180 | | 151 / 183 |
| Quoted text (84) | 52 / 66 | | 63 / 77 |

Using every word for every query cost up to 23 answers, almost all Spanish questions: they share few words with code
written in English. The first shape rule also took brand names (`GitHub`), dates (`dd/mm/aaaa`) and abbreviations
(`a.m.`) for code and lost 2 answers; the shipped rule changes none of the 201.

## Alternatives considered

- Lexical search only (Cody's choice): loses Spanish-to-English and paraphrased queries.
- SQLite FTS5: workable, but less control over tokenization and scoring.
- A fixed lexical weight for every query: measured above.

## Consequences

- Searches for names and for pasted text find exact matches the embeddings miss; prose searches are unchanged.
- One more index per project (a few MB); it rebuilds itself in seconds when it is missing or out of step.
