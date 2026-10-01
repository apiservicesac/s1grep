# 0014. No file watcher

- Status: Accepted (2026-10-01)
- Implementation: decided in stage 5 (0.5.0); revisit if change detection ever costs a noticeable part of a search.

## Context

Stage 5 planned an optional file watcher (the notify crate) so that a search would not walk the project to find
changed files. Since stage 2 the walk runs in parallel and only files whose size or modification time changed are
read.

## Measurement

In the largest repository at hand (7,951 files, 24,740 functions), finding the changes costs about 110 ms per search:
26 ms to walk and 85 ms to compare every file with the catalog (`s1-lab profile`). A search takes 1.5–2 s, most of it
the judge.

## Decision

No watcher. At most it would save about 5 % of a search, and it brings the problems other tools ran into: inotify
watch limits on large trees, missed events, nothing on network file systems, editors that save by renaming, and a
reconcile path that has to exist anyway.

## Consequences

- Every search walks its project; the cost grows with the number of files, not with the size of the code.
- If a project large enough to make the walk costly appears, the first step is to skip the walk when the last one is a
  few seconds old, before a watcher.
