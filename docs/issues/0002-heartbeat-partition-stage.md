# 0002: unless_heartbeat under a real network partition

- status: open
- kind: not-proven
- phase: 5
- opened: 2026-09-11

`unless_heartbeat` is the dead-man trigger: a target that stops hearing
from the controller fires its backstop and undoes the plan on its own
clock. It is proven on one machine's clocks only, never across a severed
link (ROADMAP Phase 3, "not proven"; Phase 5, "the `unless_heartbeat`
partition stage").

**Done when.** A tier-6 stage on the FreeBSD reaper guest arms a plan with
a heartbeat, severs the controller's link to the target (a `pf` rule, as
the T3 harness already manages), and shows the artifact firing and undoing
at the deadline; with the link restored, the engine reads the firing on
its next contact (R0402, `BackstopFired`). The stage passes, which is Phase
5's acceptance line.
