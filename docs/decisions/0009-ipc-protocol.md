# 0009. A versioned IPC protocol with typed errors and cancellation

- Status: Accepted (2026-10-01)
- Implementation: done (0.5.0); described in docs/protocol.md.

## Context

The CLI, the MCP server and the daemon exchange JSON lines over loopback TCP with a token. Errors are free text,
requests cannot be cancelled, and a client can wait forever on a busy daemon.

## Decision

Keep loopback TCP with a token (identical on Windows and Unix). Add a protocol version handshake, request ids,
typed errors (models missing, busy, invalid project, internal), cancellation when the client disconnects, and read
timeouts. The daemon takes its lock before loading models and writes its info file atomically.

## Alternatives considered

- Unix domain sockets: cleaner on Unix, but a second transport to maintain for Windows.

## Consequences

- Clients can tell the user exactly what went wrong and stop waiting when they should.
