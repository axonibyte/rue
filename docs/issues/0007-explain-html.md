# 0007: rue explain --html

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

`rue explain` prints a plan's numbered steps with their undo lines, loci
and policies. `--html` renders the same as one self-contained page (ROADMAP
Phase 5). Small.

**Closed.** Done, 2026-09-12. `rue explain --html` renders the same listing as one self-contained page: a table of the numbered steps with their loci, refusal modes, drift policies, undo lines, undo loci and notes, the verdict's prose above it whether the plan stands or not, and a style block. Nothing is fetched -- no script, no stylesheet, no font, no image -- because a page an operator keeps beside an incident or mails to an approver has to say the same thing years later on a machine with no network, and what cannot be fetched cannot change what the page said after it was read. A plan's text is a tenant's to write, so every value is escaped: an op named `<script>` renders as characters. Tests: the page is self-contained and names every op the text listing does (core/tests/render.rs), markup in a plan is escaped, the seeded fuzz renders a page for every generated plan and requires every row it opens to be closed, and the CLI test renders a real tenant through the verb end to end. Row `explain-html-renders-a-plans-markup`.
