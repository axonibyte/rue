# 0016: A public README

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-12
- closed: 2026-09-12

Phase 5's exit criteria name a public README whose prior-art section is
ROADMAP 1.2's. The name sweep that was to come first is done
(`docs/prior-art.md`, and `docs/issues/0008`), and it found that `rue` is
taken on crates.io, PyPI and npm, and by two other programming languages on
GitHub.

**Why this is filed rather than written.** A README is the face of a
project, and its first line has to say what the thing is called. Writing one
now means either ignoring the collision or disambiguating against a name
that may not survive the owner's decision. ROADMAP 12's own order is sweep
first; the sweep says wait.

**Done when.** The owner has decided on the name (keep, keep-and-publish-as-
`rued`, or rename), and a README exists that says what rue is, what it
refuses to do, how to run the tenants, and carries 1.2's prior art with the
delta from each -- including the two languages the sweep found, so a reader
who arrives from one of them knows immediately which project this is.

**Closed.** Closed on a wrong premise, 2026-09-12: the README existed already -- 251 lines with the prior-art section 1.2 asks for -- and what it needed was Phase 5's status, not writing from nothing. The correction is mine: the note closing `docs/issues/0008` said the README was 'not written and deliberately left', which was not true of the repository it was written in.

The README now carries Phase 5 (the grammar, the language server, `explain --html`, drills, the partition stage and the controller stamp), an 'About the name' section that points at the sweep and says the name is undecided and that nothing here depends on the decision, the four new rows its 'what is NOT proven' table earned, and the two new crates and two new guards in 'what is here'. Going public remains the owner's decision; what this issue asked for exists.
