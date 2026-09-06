//! T4: a temporary override in a reactive host (docs/ROADMAP.md section 8.4).
//! Temporary, embedded. One op over a group of actuator facts with a
//! restorative undo, a controller undo locus, and drift :clobber or :defer.

use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![host("site-ctl", "reactive-host", &["actuate"], false)],
        transports: strings(&["actuate"]),
        authenticators: vec![authenticator("site_operator", true)],
        max_wait: None,
        scheduler_present: vec![],
    }
}

fn shed_load_op(drift: Drift) -> Op {
    Op {
        undo_locus: UndoLocus::Controller,
        drift: Some(drift),
        undo_one_line: "restore the three actuators to their reported pre-shed state".into(),
        ..Op::new(
            "shed_load",
            vec![
                FootprintEntry::entry(Kind::Modified, "actuator:hvac-1:state"),
                FootprintEntry::entry(Kind::Modified, "actuator:hvac-2:state"),
                FootprintEntry::entry(Kind::Modified, "actuator:pump-1:state"),
            ],
        )
    }
}

pub fn shed_load() -> Plan {
    Plan {
        wane: Some(dur(7200)),
        ..Plan::new(
            "shed_load",
            "site-ctl",
            vec![s(shed_load_op(Drift::Clobber))],
        )
    }
}

pub fn shed_load_deferring() -> Plan {
    Plan {
        wane: Some(dur(7200)),
        ..Plan::new(
            "shed_load_deferring",
            "site-ctl",
            vec![s(shed_load_op(Drift::Defer))],
        )
    }
}

pub fn tenant() -> Tenant {
    Tenant {
        name: "t4".into(),
        site: site(),
        requester: "reactive_host".into(),
        cases: vec![
            case("site-ctl", shed_load()),
            case("site-ctl-defer", shed_load_deferring()),
        ],
    }
}
