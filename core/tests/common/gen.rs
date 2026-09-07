//! A seeded generator over whole sites and plans, for the tier-4 properties
//! (docs/ROADMAP.md Phase 1 task 10): every item kind, every op field,
//! every undo form, every primitive and reference, every gate shape, every
//! trigger. Self-contained so `render/tests` can include it by path; the
//! xorshift is the same one the laws have always used, so every run is
//! replayable from its seed.
#![allow(dead_code)]

use rue_core::body::*;
use rue_core::model::*;

/// xorshift32: deterministic across platforms, replayable from the seed.
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u32) as usize]
    }
    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        self.below(den) < num
    }
    pub fn maybe<T>(&mut self, g: impl FnOnce(&mut Rng) -> T) -> Option<T> {
        if self.chance(1, 2) {
            Some(g(self))
        } else {
            None
        }
    }
}

const OSES: &[&str] = &["freebsd", "linux", "macos", "windows", "appliance"];
const TRANSPORTS: &[&str] = &["ssh", "api", "console"];

pub fn gen_name(rng: &mut Rng) -> String {
    let n = 1 + rng.below(4);
    (0..n)
        .map(|_| *rng.pick(&['a', 'b', 'c', 'd', 'e', 'f']))
        .collect()
}

fn few<T>(rng: &mut Rng, n: u32, mut g: impl FnMut(&mut Rng) -> T) -> Vec<T> {
    let k = rng.below(n.min(4) + 1);
    (0..k).map(|_| g(rng)).collect()
}

fn subset(rng: &mut Rng, xs: &[&str]) -> Vec<String> {
    xs.iter()
        .filter(|_| rng.chance(1, 2))
        .map(|s| s.to_string())
        .collect()
}

fn duration(rng: &mut Rng) -> Duration {
    Duration::new(*rng.pick(&[0, 1, 20, 60, 600, 1800, 3600, 14_400, 86_400]))
}

pub fn gen_site(rng: &mut Rng) -> Site {
    let n = 1 + rng.below(3);
    let hosts: Vec<HostRecord> = (0..n)
        .map(|i| {
            let filesystem = rng.chance(3, 4);
            HostRecord {
                name: format!("h{i}"),
                os: rng.pick(OSES).to_string(),
                reach: subset(rng, TRANSPORTS),
                filesystem,
                stdin_preamble: if filesystem {
                    rng.chance(3, 4)
                } else {
                    rng.chance(1, 4)
                },
                artifact: rng.maybe(|r| {
                    *r.pick(&[
                        ArtifactLanguage::Sh,
                        ArtifactLanguage::Powershell,
                        ArtifactLanguage::Python,
                    ])
                }),
            }
        })
        .collect();
    let scheduler_present = hosts
        .iter()
        .filter(|_| rng.chance(2, 3))
        .map(|h| h.name.clone())
        .collect();
    Site {
        hosts,
        transports: subset(rng, TRANSPORTS),
        authenticators: few(rng, 3, |r| Authenticator {
            id: r
                .pick(&["oncall", "alice", "driver", "requester"])
                .to_string(),
            human: r.chance(2, 3),
        }),
        max_wait: rng.maybe(duration),
        scheduler_present,
        secrets_deliver_to: subset(rng, &["requester", "hook:escrow"]),
    }
}

fn gen_guard(rng: &mut Rng) -> Guard {
    Guard {
        name: gen_name(rng),
        value: *rng.pick(&[Tri::Yes, Tri::No, Tri::Unknown]),
        force_never: rng.chance(1, 4),
    }
}

fn shape(rng: &mut Rng) -> String {
    match rng.below(6) {
        0..=3 => format!("file:/{}", gen_name(rng)),
        4 => format!("file:/{{{}}}", gen_name(rng)),
        _ => format!("svc:{}", gen_name(rng)),
    }
}

fn gen_entry(rng: &mut Rng) -> FootprintEntry {
    let kind = *rng.pick(&[
        Kind::Owned,
        Kind::Region,
        Kind::Modified,
        Kind::Derived,
        Kind::AppendOnly,
        Kind::Held,
    ]);
    FootprintEntry {
        kind,
        shape: shape(rng),
        instance: rng.maybe(gen_name),
        anchor: if kind == Kind::Region || rng.chance(1, 8) {
            Some(gen_name(rng))
        } else {
            None
        },
    }
}

fn gen_ref(rng: &mut Rng) -> Ref {
    match rng.below(7) {
        0 => fact(&gen_name(rng)),
        1 | 2 => param(&gen_name(rng)),
        3 => host_field(rng.pick(&["name", "os", "address"])),
        4 => output(&gen_name(rng), &gen_name(rng), rng.chance(1, 2)),
        5 => controller(&gen_name(rng)),
        _ => secret(&gen_name(rng)),
    }
}

fn gen_part(rng: &mut Rng) -> Part {
    if rng.chance(2, 3) {
        text(rng.pick(&["x ", "it's ", "$(x) ", "\"q\" ", "\\ ", "\u{1} "]))
    } else {
        interp(gen_ref(rng))
    }
}

fn gen_value(rng: &mut Rng) -> Value {
    match rng.below(3) {
        0 => lit(rng.pick(&["x", "it's", "", "line\n"])),
        1 => Value::Ref(gen_ref(rng)),
        _ => Value::Template(few(rng, 3, gen_part)),
    }
}

fn gen_fact_ref(rng: &mut Rng) -> FactRef {
    FactRef {
        shape: shape(rng),
        anchor: rng.maybe(gen_name),
    }
}

fn gen_prim(rng: &mut Rng) -> Prim {
    match rng.below(11) {
        0..=2 => Prim::Run(Run {
            cmd: few(rng, 4, gen_part),
            env: few(rng, 1, |r| EnvVar {
                name: "V".into(),
                value: gen_value(r),
            }),
            stdin: rng.maybe(gen_value),
        }),
        3 => write(gen_fact_ref(rng), gen_value(rng)),
        4 => remove(gen_fact_ref(rng)),
        5 => append(gen_fact_ref(rng), gen_value(rng)),
        6 => region_set(gen_fact_ref(rng), gen_value(rng)),
        7 => region_clear(gen_fact_ref(rng)),
        8 => stage(&gen_name(rng), gen_value(rng), 0o600),
        9 => hook(&gen_name(rng), vec![("k", gen_value(rng))]),
        _ => match rng.below(3) {
            0 => install(&gen_name(rng)),
            1 => release(&gen_name(rng)),
            _ => Prim::Call(Call {
                prim: gen_name(rng),
                run: few(rng, 3, gen_part),
                args: few(rng, 2, |r| ClassedArg {
                    name: gen_name(r),
                    class: *r.pick(&[ArgClass::TargetLocal, ArgClass::Controller]),
                    value: gen_value(r),
                }),
            }),
        },
    }
}

pub fn gen_body(rng: &mut Rng) -> Body {
    few(rng, 3, gen_prim)
}

fn gen_factor(rng: &mut Rng, depth: u32) -> Factor {
    match rng.below(if depth == 0 { 3 } else { 4 }) {
        0 => Factor::Auth {
            id: rng
                .pick(&["oncall", "alice", "driver", "requester", "ghost"])
                .to_string(),
            weight: 1 + rng.below(3),
        },
        1 => Factor::Humans {
            weight: 1 + rng.below(3),
        },
        2 => Factor::Wait {
            duration: duration(rng),
            weight: 1 + rng.below(3),
        },
        _ => Factor::Group {
            expr: Box::new(gen_gate(rng, depth - 1)),
            weight: 1 + rng.below(3),
        },
    }
}

pub fn gen_gate(rng: &mut Rng, depth: u32) -> GateExpr {
    if rng.chance(1, 2) {
        GateExpr::Single(gen_factor(rng, depth))
    } else {
        GateExpr::Thresh {
            n: rng.below(4),
            factors: few(rng, 3, |r| gen_factor(r, depth)),
        }
    }
}

fn gen_undo(rng: &mut Rng, entries: &[FootprintEntry]) -> Undo {
    let pre = || -> Vec<String> { entries.iter().map(|e| e.shape.clone()).collect() };
    match rng.below(6) {
        0..=2 => Undo::Restore,
        3 => Undo::NoUndo,
        4 => Undo::Computed {
            body: gen_body(rng),
            undo_pre: if rng.chance(1, 5) { vec![] } else { pre() },
        },
        _ => Undo::Compensate {
            body: gen_body(rng),
            undo_pre: if rng.chance(1, 5) { vec![] } else { pre() },
        },
    }
}

fn gen_refusal(rng: &mut Rng) -> Refusal {
    match rng.below(4) {
        0 | 1 => Refusal::Revert,
        2 => Refusal::Hold {
            via: rng.maybe(gen_name),
        },
        _ => Refusal::Knell {
            guard: rng.maybe(gen_guard),
            cost: if rng.chance(1, 2) {
                Cost::Probe(gen_name(rng))
            } else {
                Cost::NoCost(gen_name(rng))
            },
            ack: if rng.chance(1, 2) {
                Ack::Gate(gen_gate(rng, 1))
            } else {
                Ack::NoAck(gen_name(rng))
            },
        },
    }
}

pub fn gen_op(rng: &mut Rng, site: &Site) -> Op {
    let footprint = few(rng, 3, gen_entry);
    let undo = gen_undo(rng, &footprint);
    let held = footprint.iter().any(|e| e.kind == Kind::Held);
    Op {
        id: gen_name(rng),
        pre: few(rng, 2, gen_guard),
        do_: gen_body(rng),
        undo,
        post: few(rng, 2, gen_guard),
        // Weighted toward :target so backstop coverage and the renderer are
        // exercised often enough for their properties to bite.
        undo_locus: *rng.pick(&[
            UndoLocus::Target,
            UndoLocus::Target,
            UndoLocus::Controller,
            UndoLocus::NoLocus,
        ]),
        refusal: gen_refusal(rng),
        drift: rng.maybe(|r| *r.pick(&[Drift::Clobber, Drift::Defer])),
        reach: subset(rng, TRANSPORTS),
        outputs: few(rng, 2, |r| Output {
            name: gen_name(r),
            secret: r.chance(1, 2),
        }),
        exclusivity: rng.maybe(gen_name),
        locus: match rng.below(5) {
            0 => Locus::Controller,
            1 | 2 => Locus::Target,
            3 => Locus::Host(HostRef::Static(
                site.hosts
                    .get(rng.below(site.hosts.len() as u32 + 1) as usize)
                    .map(|h| h.name.clone())
                    .unwrap_or_else(|| "ghost".into()),
            )),
            _ => Locus::Host(HostRef::Bound(gen_name(rng))),
        },
        suspend: if held || rng.chance(1, 6) {
            rng.maybe(gen_body)
        } else {
            None
        },
        reestablish: if held || rng.chance(1, 6) {
            rng.maybe(gen_body)
        } else {
            None
        },
        handoff_done: rng.maybe(gen_name),
        undo_idempotent: rng.chance(7, 8),
        footprint,
    }
}

pub fn gen_stepi(rng: &mut Rng, site: &Site) -> StepI {
    StepI {
        op: gen_op(rng, site),
        direction: *rng.pick(&[Direction::Forward, Direction::Inverse]),
        gate: rng.maybe(|r| gen_gate(r, 1)),
        window: rng.maybe(duration),
        on_lapse: *rng.pick(&[OnLapse::Revert, OnLapse::Hold]),
        force: few(rng, 2, |r| match r.below(3) {
            0 => ForceName::Guard(gen_name(r)),
            1 => ForceName::Drift,
            _ => ForceName::Unknown,
        }),
        alias: rng.maybe(gen_name),
        args: few(rng, 2, |r| format!("{}: {}", gen_name(r), gen_name(r))),
    }
}

/// An item of bounded size; knells only when `knells` is set.
pub fn gen_item_in(rng: &mut Rng, site: &Site, n: u32, knells: bool) -> Item {
    if n == 0 {
        return Item::Step(gen_stepi(rng, site));
    }
    match rng.below(14) {
        0..=4 => Item::Step(gen_stepi(rng, site)),
        5 if knells => Item::Knell(gen_stepi(rng, site)),
        5 => Item::Step(gen_stepi(rng, site)),
        6 => Item::Par {
            children: few(rng, n / 3, |r| gen_item_in(r, site, n / 3, knells)),
        },
        7 => Item::Confirm,
        8 => Item::Observe {
            probe: gen_name(rng),
            alias: gen_name(rng),
        },
        9 => Item::Assert {
            guard: gen_guard(rng),
            window: rng.maybe(duration),
            on_lapse: *rng.pick(&[OnLapse::Revert, OnLapse::Hold]),
        },
        10 => Item::Repeat {
            form: if rng.chance(1, 2) {
                RepeatForm::Count(1 + rng.below(3))
            } else {
                RepeatForm::Over {
                    list: gen_name(rng),
                    max: 1 + rng.below(16),
                    set_valued: rng.chance(3, 4),
                }
            },
            var: gen_name(rng),
            body: few(rng, n / 3, |r| gen_item_in(r, site, n / 3, knells)),
        },
        11 => Item::Preflight {
            guards: few(rng, 2, gen_guard),
        },
        12 => Item::Slot {
            name: gen_name(rng),
        },
        _ => Item::When {
            guard: gen_guard(rng),
            window: rng.maybe(duration),
            on_lapse: *rng.pick(&[OnLapse::Revert, OnLapse::Hold]),
            then_: few(rng, n / 3, |r| gen_item_in(r, site, n / 3, knells)),
            else_: few(rng, n / 3, |r| gen_item_in(r, site, n / 3, knells)),
        },
    }
}

fn gen_items(rng: &mut Rng, site: &Site, knells: bool) -> Vec<Item> {
    let n = 1 + rng.below(8);
    let mut v = few(rng, n, |r| gen_item_in(r, site, n, knells));
    if v.is_empty() {
        v.push(Item::Step(gen_stepi(rng, site)));
    }
    if rng.chance(1, 3) {
        v.push(Item::Commit);
    }
    v
}

/// A plan of modest depth over the site, with every plan-level field
/// exercised; the owner is a site host, or (rarely) one the site lacks.
pub fn gen_plan(rng: &mut Rng, site: &Site) -> Plan {
    let owner = if rng.chance(1, 12) {
        "ghost".to_string()
    } else {
        rng.pick(&site.hosts).name.clone()
    };
    let body = gen_items(rng, site, true);
    let steps = body.len() as u32;
    Plan {
        id: gen_name(rng),
        owner,
        gate: rng.maybe(|r| PlanGate {
            expr: gen_gate(r, 2),
            window: r.maybe(duration),
            allow_zero_human: r.chance(1, 4),
        }),
        wane: rng.maybe(duration),
        renew_within: rng.maybe(duration),
        backstop: rng.maybe(|r| Backstop {
            triggers: {
                let mut t = few(r, 3, |r| match r.below(3) {
                    0 => Trigger::After(duration(r)),
                    1 => Trigger::UnlessConfirmed(duration(r)),
                    _ => Trigger::UnlessHeartbeat {
                        deadline: duration(r),
                        interval: r.maybe(duration),
                    },
                });
                if t.is_empty() {
                    t.push(Trigger::After(duration(r)));
                }
                t
            },
            arm_before: 1 + r.below(steps + 2),
        }),
        fires_by_construction: rng.chance(1, 6),
        strictness: *rng.pick(&[Strictness::Warn, Strictness::Strict]),
        mode: *rng.pick(&[Mode::Manual, Mode::Auto]),
        exclusivity: rng.maybe(gen_name),
        require_journal: rng
            .maybe(|r| *r.pick(&[JournalRequirement::Chained, JournalRequirement::Signed])),
        body,
    }
}

/// The requester: a declared authenticator, or one the site lacks.
pub fn gen_requester(rng: &mut Rng, site: &Site) -> String {
    if site.authenticators.is_empty() || rng.chance(1, 6) {
        "requester".into()
    } else {
        rng.pick(&site.authenticators).id.clone()
    }
}

// ---------------------------------------------------------------------------
// The laws' generators (section 5.5): the same shapes, knell-free by
// construction, over a fixed small site.

pub fn law_site() -> Site {
    Site {
        hosts: vec![HostRecord {
            name: "db-01".into(),
            os: "freebsd".into(),
            reach: vec!["ssh".into()],
            filesystem: true,
            stdin_preamble: true,
            artifact: None,
        }],
        transports: vec!["ssh".into()],
        authenticators: vec![],
        max_wait: None,
        scheduler_present: vec![],
        secrets_deliver_to: vec![],
    }
}

pub fn gen_step(rng: &mut Rng) -> StepI {
    gen_stepi(rng, &law_site())
}

/// A knell-free item of bounded size.
pub fn gen_item(rng: &mut Rng, n: u32) -> Item {
    gen_item_in(rng, &law_site(), n, false)
}

/// A knell-free plan body of modest depth.
pub fn gen_knell_free(rng: &mut Rng) -> Vec<Item> {
    let n = rng.below(9);
    few(rng, n, |r| gen_item_in(r, &law_site(), n, false))
}

pub fn gen_with_knell(rng: &mut Rng) -> Vec<Item> {
    let mut v = gen_knell_free(rng);
    v.push(Item::Knell(gen_step(rng)));
    v.extend(gen_knell_free(rng));
    v
}

/// The seed and step count a property runs with: `RUE_FUZZ_SEED` and
/// `RUE_FUZZ_STEPS`, else the defaults given.
pub fn fuzz_params(default_seed: u32, default_steps: u32) -> (u32, u32) {
    let get = |k: &str, d: u32| {
        std::env::var(k)
            .ok()
            .map(|v| {
                v.parse()
                    .unwrap_or_else(|_| panic!("{k}={v}: not an integer"))
            })
            .unwrap_or(d)
    };
    (
        get("RUE_FUZZ_SEED", default_seed),
        get("RUE_FUZZ_STEPS", default_steps),
    )
}

/// Run `f` for every step, naming the seed and step in the panic message
/// when one fails, so a run is replayable from the report.
pub fn each_step(default_seed: u32, default_steps: u32, mut f: impl FnMut(&mut Rng, u32)) {
    let (seed, steps) = fuzz_params(default_seed, default_steps);
    let mut rng = Rng(seed);
    for step in 0..steps {
        let state = rng.0;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut local = Rng(state);
            f(&mut local, step);
            local.0
        }));
        match r {
            Ok(next) => rng.0 = next,
            Err(e) => {
                eprintln!("fuzz: seed {seed} step {step} (rng state {state:#x}) failed; RUE_FUZZ_SEED={state} RUE_FUZZ_STEPS=1 replays it");
                std::panic::resume_unwind(e);
            }
        }
    }
}
