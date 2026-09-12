# 0002: unless_heartbeat under a real network partition

- status: closed
- kind: not-proven
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

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

**Closed.** Proven by `tenants/e2e/tests/partition.rs`, 2026-09-11, on both reaper guests. A plan with `[after: 1h, unless_heartbeat: 60s]` is applied; the controller's path to the target is then cut by a firewall rule naming the target address and port 22 alone, with the daemon left up and beating. The stage asserts the heartbeat file stops advancing across two intervals, the target's own cron fires the artifact on the stale beat and undoes the step with no engine involved, and once the link is restored the engine reads the firing and journals `backstop_fired` (R0402). A drop guard restores the link however the stage leaves, since a severed guest would fail every stage after it. The cut is not a vnet: it is a filter on the loopback path both ends share, so what is proven is that the engine cannot reach the host and the target acts alone, not a second network stack. Where the severing rule lives differs by family and cannot be symmetric -- pf evaluates only the anchors its ruleset names, so provisioning declares an empty one; nftables evaluates a table because it exists, so the stage makes its own. The first draft put a chain inside `/etc/nftables.conf` and the Linux guest lost it mid-run to T3's own reload of that file, which is the fact under test: the mechanism of a stage may not live inside the fact another stage mutates.
