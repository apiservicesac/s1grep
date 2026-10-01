# 0013. A smaller model embeds outlines

- Status: Accepted (2026-10-01)
- Implementation: done (0.3.0).

## Context

A large repository is searchable only once its functions have vectors. The outline pass (path, name and first lines)
was embedded by the main retriever at about 20 functions per second in the background, so a 25,000-function
repository took more than 20 minutes to become searchable, and searches in the meantime missed what was not indexed.
Faster ways to run the main retriever lose answers on the exam (0000: a smaller model for everything, INT8).

## Decision

Outlines are embedded by granite-embedding-97m-multilingual-r2 and whole sources by granite-embedding-278m-multilingual.
Each kind of vector lives in its own embedding space. A search embeds the query with both models, takes the nearest
25 among whole-source vectors and among the outline vectors of functions without a whole-source vector, and merges
the two lists by reciprocal rank (`SearchSettings::MERGE_SMOOTHING`). With one list empty its order is kept, so a
fully indexed project is searched exactly as with the main retriever alone.

## Measurements

| Outline vectors by | Top-1 | Top-5 | Outline speed |
|---|---|---|---|
| granite 278M | 135 | 167 | 1× |
| granite 97M-r2 | 133 | 166 | 3.3× (benchmark), about 3× in the background |

With whole sources the exam is unchanged: 168 / 189. A 25,000-function repository is fully searchable in about 7
minutes on an 8-core CPU instead of more than 20.

## Alternatives considered

- The smaller model for whole sources too: top-1 154.
- INT8 weights for the main model: top-1 147–163 for 1.3× (also slower or no faster for the judge on these CPUs).
- Shorter outlines: barely faster, top-1 129.

## Consequences

- One more model to download (about 400 MB) and to keep in memory in the background process.
- During indexing, outline-only candidates and whole-source candidates are merged by rank, not by similarity, since
  the two models' similarities are not comparable.
