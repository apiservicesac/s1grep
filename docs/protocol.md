# Background process protocol

The CLI and the MCP server talk to the background process over a loopback TCP connection, one request per
connection, in JSON lines. Version 2 is described here (`ServerSettings::PROTOCOL_VERSION`).

## Finding the process

The process writes `server.json` in the cache folder (mode 0600 on Unix): its port, a random token, its process id,
its s1grep version, its protocol version and its idle limit. It holds `server.lock` for as long as it lives, so a
`server.json` whose lock is free was left by a process that is gone. A client that finds another s1grep or protocol
version stops that process and starts its own.

## Request

The first line a client sends:

```json
{"token": "…", "protocol": 2, "request": {"kind": "search", "query": "…", "target": "/abs/path", "top": 5, "judge_top": 5, "filters": {"excludes": []}}}
```

| `kind` | Fields | Answer |
|---|---|---|
| `ping` | | `pong`, at once even while the process searches or indexes |
| `shutdown` | | `done`; the process stops after the request in hand |
| `search` | a `SearchRequest` | progress lines, then `searched` |
| `index` | `target` | progress lines while the project is read, then `indexing` with its progress; the indexing goes on in the background |
| `progress` | `target` | `indexing` with the project's progress |

## Replies

Zero or more `progress` lines, then exactly one of the others:

```json
{"kind": "progress", "event": {"event": "scanning", "done": 500, "files": 7951}}
{"kind": "searched", "response": {…}}
{"kind": "indexing", "progress": {"root": "…", "functions": 24740, "indexed": 992, "searchable": 6107, "indexing": {"done": 0, "total": 23748, "seconds_left": null, "searchable_seconds_left": 420.0}, "held_elsewhere": false}}
{"kind": "failed", "error": {"kind": "not_found", "message": "/abs/path does not exist"}}
```

Error kinds: `unauthorized` (wrong token), `protocol_mismatch`, `invalid_request`, `not_found` (the folder does not
exist), `models_missing` (`s1grep setup` downloads them), `internal`.

## Threads and order

One thread accepts connections and answers `ping` and `shutdown` itself; a worker owns the models and handles the rest
in arrival order. Between requests the worker runs short background indexing steps; a search makes them wait for a
moment so that the search after it does not queue behind one. A client that goes away is noticed between the costly
steps of its search (after the scan, during the first outline pass, before the judge), and that search is dropped
without a reply.

## MCP

`s1grep mcp` serves the Model Context Protocol on stdin/stdout with two tools:

- `search_code`: `query`, optional `path`, `top` and `offset`. The text lists the results, then any coverage notes
  (how much of the repository can be searched yet, whether indexing continues). When the call carries
  `_meta.progressToken`, progress notifications are sent while the repository is read and made searchable.
- `index_status`: optional `path`; how many functions can be searched and are fully indexed, and the time left.
