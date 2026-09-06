# rue-proto

The Phase 0 prototype: a Haskell encoding of the core model (docs/ROADMAP.md
section 5) with a checker over it, the four acceptance tenants as terms, and
the tests that hold the roadmap's claims to their letter. It exists to find
out whether the model is coherent before Phase 1 rewrites it in Rust; its
goldens and its canonical JSON are what Phase 1 must reproduce.

## Layout

| Path | What |
|---|---|
| `src/Rue/Proto/Model.hs` | Facts, footprints, ops, items, plans, sites (sections 5.1 to 5.4) |
| `src/Rue/Proto/Algebra.hs` | `seq`, `par`, `reverse`, `reverse_from` and the reversal laws (5.5) |
| `src/Rue/Proto/Interference.hs` | The interference query as list comprehensions (5.7) |
| `src/Rue/Proto/Backstop.hs` | Coverage, arming order, the reach rule, trigger rules (5.6) |
| `src/Rue/Proto/Intent.hs` | Intent inference and the commit rules (5.4) |
| `src/Rue/Proto/Gates.hs` | Weighted-threshold gates: satisfiability, humans, requester, wait-alone (5.11) |
| `src/Rue/Proto/Check.hs` | `check :: Site -> Requester -> Plan -> Verdict` |
| `src/Rue/Proto/Verdict.hs`, `Prose.hs`, `Explain.hs` | The structured verdict (5.8), its prose (Appendix A), `explain` (Appendix B) |
| `src/Rue/Proto/States.hs` | The runtime state machine from its five rules (5.9); generates `docs/state-transitions.tsv` |
| `src/Rue/Proto/Ledger.hs` | The cross-plan ledger: reservation at Pending, exclusivity classes (5.12) |
| `src/Rue/Proto/Diagnostics.hs` | Every code of section 6.7 as a constructor; the only place a code is text |
| `src/Rue/Proto/Json/Canonical.hs` | The canonical encoder (docs/TESTING.md) |
| `src/Rue/Proto/Json/PlanIr.hs` | The plan IR: the checker's input as data, one `plan.json` golden per case (docs/TESTING.md) |
| `tenants/Rue/Proto/Tenants/` | T1 to T4 as terms, the negative cases, and the artifact list |
| `app/` | `rue-proto-check`, `rue-proto-goldens` (the only golden writer), `rue-proto-states` |
| `test/` | The suite; see docs/TESTING.md |

```sh
cabal build all && cabal test all --test-show-details=direct
cabal run -v0 rue-proto-check -- t1 db-01             # prose; --json, --explain, --ir
RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens       # rewrite expected files, deliberately
```

## What encoding the tenants taught

Phase 0's stated exit path is to fold what the prototype learned back into
section 5. These are the positions the prototype took where the roadmap was
silent, or where its text could not be followed as written. Each is a test.

1. **The requester is an input to `check`.** E0508 (a gate counting the
   requester) is only decidable offline if the checker knows who is asking.
   `check` takes the requester's authenticator id; `rue-proto-check` takes it
   from the tenant. Phase 2's CLI needs an `--as` or an operator identity.
2. **Facts are scoped by host.** The same shape on two hosts is two facts;
   T2's heir on node-c does not collide with the per-guest loop on node-b.
   A step whose host is bound at runtime is penumbral by host, and the
   verdict lists the binding under `unresolved_bindings`. Section 5.7's
   query should say so.
3. **Par siblings have no order.** Children of one `par` are judged only by
   umbra disjointness (E0303), never as an ordered conflict pair (E0301).
4. **A repeated anchor is E0305 alone.** It is also, formally, a conflict;
   the specific diagnosis wins and E0301 is not raised beside it.
5. **Holding is per knell segment.** `holds_at` lists the first `:hold`
   step of each segment between knells; section 5.5 says "at or before the
   first knell", and T2 holds after it. `held_indefinitely` lists every
   `:hold` step of a permanent plan and every deferred one, as section 8.2
   reads.
6. **Deferred is a host rule.** A step is deferred when its host is not the
   owner and no site transport reaches it, or when its host is bound at
   runtime. `handoff_done` is printed with it.
7. **`Pending` needs a bound.** A plan-entry gate with no `window:` on a
   site with no `max_wait` would reserve umbras until cancelled; the
   prototype refuses it with E0506.
8. **Reversibility counts steps before the knell, else mutating steps.**
   With a knell, `reversible_through` is the step before it (T2: "through
   step 3", the probes rung). Without one, it is the last mutating step, so
   `confirm()`, `commit()` and observations after it do not extend it (T3:
   "through step 1").
9. **The verdict carries `mode`.** Appendix A's single hold clause,
   "(human required)", contradicts section 8.2 for `mode: :auto`, where a
   hold waits for `resume`, `recant` or `commit` with no human in the loop.
   The prose reads "(until resume, recant or commit)" under auto. Appendix
   A needs the variant, and the schema gained the field.
10. **E0406 cannot arise.** "Artifact would be installed after a covered
    step" is unreachable in this model: installation precedes the first
    covered step by construction. Either the model is missing a way to
    place installation, or the code belongs to the engine.
11. **`Applied` and `Suspended` are temporary-only; renewal is meaningful
    while `Applying`.** A permanent plan goes from `Applying` to `Committed`
    and never rests; wane is anchored at approval, so `renew` while still
    applying is a real event.
12. **A `hold_via:` op's footprint is not yet in the interference query.**
    T2's resurrection gate is encoded as an ordinary `:hold` step with a
    footprint; the roadmap's `hold_via:` form has no separate treatment.
13. **E-codes were renumbered before Phase 0** so that section 6.7's table
    and the enumeration agree with no gaps; they freeze at Phase 1.

## What is not modeled

Thirty of the fifty-six codes: the surface's E01xx (parsing, names, kinds,
totality), the secret rules (E0206, E0209 to E0211, E0411), the engine-shaped
E0204, E0402, E0406, E0408 and E0409, and the binding rules E0601 to E0606.
No executor, no artifact, no engine, no parser. The `.rue` text under
`tenants/` is what Phase 2 must accept; nothing here reads it.
