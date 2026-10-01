# 0001. General purpose: nothing specific to a domain or a language

- Status: Accepted (2026-10-01)
- Implementation: Implemented (the glossary was removed in 0.2.4).

## Context

A Spanish→English glossary and a stopword list were added so that a word search could match Spanish queries against
English code while a project was still being indexed. They covered one business domain and one country, not code in
general, and every new domain would have needed new entries. probe hard-codes English term lists with the same
problem.

## Decision

s1grep contains no glossaries, term lists, stemmers or rules for a framework, domain or country. Searching across
languages is the multilingual embedding model's job; the judge reads code in any language it was trained on. A lexical
index, when added, matches identifiers and literal text only.

## Alternatives considered

- Keep and grow the glossary: unbounded maintenance, biased towards one user.
- Translate queries with a model: another model to ship and run, for a problem the embedder already solves.

## Consequences

- Quality in a new domain depends on the models only, and is measured by the exam.
- Until a project has vectors, it cannot be searched by meaning; ADR-0008 keeps that window short.
