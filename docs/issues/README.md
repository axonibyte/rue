# Issues

rue's issue tracker: one file per issue in this directory, versioned and
reviewed like the code. Bitbucket Cloud retired its native issue tracker
in August 2026, and the owner chose a tracker in the repository
(2026-09-11). The roadmap stays the plan of record; an issue is a unit of
work or a decision, and links to the roadmap rather than restating it.

## An issue

`NNNN-slug.md`, numbered in order and never reused:

```text
# 0016: One line saying what is wrong or wanted

- status: open
- kind: defect
- phase: 5
- opened: 2026-09-11

What is wrong or wanted, why it matters, and what done looks like.
```

- **status**: `open`, `in-progress` or `closed`.
- **kind**: `defect` (rue does something wrong), `feature` (something rue
  should do), `question` (a decision someone has to make), `not-proven` (a
  claim no test holds yet).
- **phase**: the roadmap phase it belongs to (`5`, `3W`, ...), or `-`.
- **closed**: a closed issue adds `- closed: YYYY-MM-DD`, the day it was
  closed, and ends with a paragraph saying how -- fixed, decided, or not
  done and why. An issue that is not closed names no date. The commit that
  closes it says `Closes #NNNN` in its message, which is how to find it
  (`git log --grep='Closes #NNNN'`); a commit cannot name its own hash in a
  file it changes.

A title may not contain `|`, which would break the index.

## Working an issue

Open one by adding its file and its index row in one commit. Mark it
`in-progress` when work starts. Close it in the commit that finishes it:
the status, the `closed` date, the closing paragraph and the index row,
with `Closes #NNNN` in the commit message.

`tools/lint-issues.sh`, the gate's `issues` phase, holds these rules: every
file well formed, numbers unique and matching their names, a closed issue
naming the day it closed, and the index below listing exactly the issues, each
with the title, kind and status the issue itself gives.

## Index

| # | Title | Kind | Status |
|---|---|---|---|
| 0001 | Upgrade vectors: an older release's texts and store check and migrate, or say what changed | feature | closed |
| 0002 | unless_heartbeat under a real network partition | not-proven | closed |
| 0003 | Drill mode: scheduled apply-and-recant on a canary, with an attestation | feature | closed |
| 0004 | The complete simulation | feature | closed |
| 0005 | tree-sitter-rue: a grammar for editors | feature | closed |
| 0006 | A language server: diagnostics and hover | feature | closed |
| 0007 | rue explain --html | feature | closed |
| 0008 | The name sweep, and a public README | feature | closed |
| 0009 | Multi-controller: refuse, or a lock protocol over a shared fact? | question | closed |
| 0010 | Release v0.2.1 with the SDK fixes? | question | closed |
| 0011 | A step interrupted by the engine's death is undone even if it never took | not-proven | closed |
| 0012 | The Rust conformance hook ends at a line that is not JSON | defect | closed |
| 0013 | Python SDK: the reply-shape check is an assert | defect | closed |
| 0014 | Phase 3W: task 14 on a real Windows guest | feature | open |
| 0015 | macOS as a controller | not-proven | open |
| 0016 | A public README | feature | closed |
