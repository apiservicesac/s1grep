# 0006. Languages are added as data: a grammar, a query and a registry entry

- Status: Accepted (2026-10-01)
- Implementation: done (0.2.6): eight languages beside Python; the judge is used for all of them.

## Context

The Python extractor is wired into the indexer, there is no language column, and the minimum-lines rule is Python
specific. ck and aider support a dozen languages each with one tree-sitter query file per language.

## Decision

A LanguageExtractor is a tree-sitter grammar plus a tags-style query (definitions, names, docs) and a size policy,
registered by file extension. Oversized definitions are split and tiny ones merged (cAST); unparseable files fall back
to line windows. Results always carry the original bytes and line ranges.

## Alternatives considered

- Hand-written extraction code per language: more code per language, more bugs.

## Consequences

- Adding a language needs a grammar crate, one .scm file and exam questions in that language.
- The judge reads candidates of every language. s1-code v3 was trained on Python only and helps on Java and PHP but
  hurts on JavaScript and Go ([0000](0000-engine-measurements.md#other-programming-languages)); the README shows
  these figures, and s1-code v4, trained on several languages, is the fix rather than switching the judge off.
