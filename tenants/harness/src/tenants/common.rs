//! Shared shapes and builders for the tenant terms, kept as close to the
//! prototype's as Rust allows so the two can be read side by side.

use rue_core::body::Prim;
use rue_core::diagnostics::Code;
use rue_core::model::*;

/// One plan checked on one host.
#[derive(Debug, Clone)]
pub struct Case {
    /// The directory name under `expected/`.
    pub host: String,
    pub plan: Plan,
}

#[derive(Debug, Clone)]
pub struct Tenant {
    /// The directory name under `tenants/`.
    pub name: String,
    pub site: Site,
    pub requester: String,
    pub cases: Vec<Case>,
}

/// A plan the checker must refuse with exactly the named code.
#[derive(Debug, Clone)]
pub struct Negative {
    pub code: Code,
    pub slug: String,
    pub site: Site,
    pub requester: String,
    pub plan: Plan,
}

/// A host record; the executor honours the stdin preamble iff it has a
/// filesystem and a shell, which every ssh host does and no appliance does.
pub fn host(name: &str, os: &str, reach: &[&str], filesystem: bool) -> HostRecord {
    HostRecord {
        name: name.into(),
        os: os.into(),
        reach: reach.iter().map(|r| r.to_string()).collect(),
        filesystem,
        stdin_preamble: filesystem,
    }
}

/// A computed undo body with the facts it needs unchanged.
pub fn computed(body: Vec<Prim>, undo_pre: &[&str]) -> Undo {
    Undo::Computed {
        body,
        undo_pre: strings(undo_pre),
    }
}

/// A compensating undo body with the facts it needs unchanged.
pub fn compensate(body: Vec<Prim>, undo_pre: &[&str]) -> Undo {
    Undo::Compensate {
        body,
        undo_pre: strings(undo_pre),
    }
}

pub fn authenticator(id: &str, human: bool) -> Authenticator {
    Authenticator {
        id: id.into(),
        human,
    }
}

pub fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

pub fn s(o: Op) -> Item {
    Item::Step(StepI::new(o))
}

pub fn knell(o: Op) -> Item {
    Item::Knell(StepI::new(o))
}

pub fn dur(seconds: u64) -> Duration {
    Duration::new(seconds)
}

pub fn auth(id: &str) -> GateExpr {
    GateExpr::Single(Factor::Auth {
        id: id.into(),
        weight: 1,
    })
}

pub fn humans() -> GateExpr {
    GateExpr::Single(Factor::Humans { weight: 1 })
}

pub fn wait(seconds: u64) -> GateExpr {
    GateExpr::Single(Factor::Wait {
        duration: dur(seconds),
        weight: 1,
    })
}

pub fn plan_gate(expr: GateExpr, window: Option<u64>, allow_zero_human: bool) -> PlanGate {
    PlanGate {
        expr,
        window: window.map(dur),
        allow_zero_human,
    }
}

pub fn backstop(triggers: Vec<Trigger>, arm_before: u32) -> Backstop {
    Backstop {
        triggers,
        arm_before,
    }
}

pub fn case(host: &str, plan: Plan) -> Case {
    Case {
        host: host.into(),
        plan,
    }
}
