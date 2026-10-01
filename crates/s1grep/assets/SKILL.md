---
name: s1grep
description: Find where behaviour lives in a repository by describing what the code does, in English or Spanish. Use it before broad text searches when you know what the code should do but not what it is called. For exact names, strings or filenames, use grep instead.
---

# s1grep

s1grep ranks the functions of a repository by how well they answer a description, with a local judge model. Nothing
leaves the machine.

## Search

```sh
s1grep "where do we retry a failed payment" path/to/repo
s1grep "dónde se valida que el token no haya expirado" . -n 10
s1grep "export rows to csv" . --json
```

- The query says what the code does, not what it is called.
- The path defaults to the current folder; a narrower folder limits the search.
- Each result shows `file:start-end`, the function name, the judge's probability and its first lines. `--json` returns the
  same with the whole source of each function.
- Tests, migrations and dependency folders are skipped by the ignore rules (`~/.config/s1grep/ignore`, `.s1grepignore`).
- Python, JavaScript, TypeScript, Go, Java, PHP, Rust, Ruby and C# files are searched; the ranking model was trained
  on Python and is most reliable there.

## Setup

If `s1grep` reports missing models, ask the user to run `s1grep setup` (a one-time 2.9 GB download). The first search
loads the models into a background process (about 10 s); later searches take about a second. `s1grep status` shows
what is ready and how far each project is indexed.

## Reading the output

Read the returned functions before searching again; use their file and line references to open more context. A low
judge probability on every result means the behaviour is probably not in that folder. When the output says that only
part of the repository can be searched yet, the answer may be in the rest: `s1grep status` (or the MCP tool
`index_status`) shows when indexing finishes, and `s1grep index <path>` waits for it.
