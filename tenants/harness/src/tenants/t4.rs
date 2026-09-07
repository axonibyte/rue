//! T4: a temporary override in a reactive host (docs/ROADMAP.md section 8.4).
//! Temporary, embedded. One op over a group of actuator facts with a
//! restorative undo, a controller undo locus, and drift :clobber or :defer.

use rue_core::body::*;
use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![host("site-ctl", "reactive-host", &["actuate"], false)],
        transports: strings(&["actuate"]),
        authenticators: vec![authenticator("site_operator", true)],
        max_wait: None,
        scheduler_present: vec![],
        secrets_deliver_to: vec![],
    }
}

fn shed_load_op(drift: Drift) -> Op {
    Op {
        undo_locus: UndoLocus::Controller,
        drift: Some(drift),
        do_: vec![hook(
            "host_actuate",
            vec![(
                "set",
                lit(r#"%{"hvac-1": :off, "hvac-2": :off, "pump-1": :low}"#),
            )],
        )],
        ..Op::new(
            "shed_load",
            vec![
                FootprintEntry::entry(Kind::Modified, "actuator:state:hvac-1"),
                FootprintEntry::entry(Kind::Modified, "actuator:state:hvac-2"),
                FootprintEntry::entry(Kind::Modified, "actuator:state:pump-1"),
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
            vec![with_args(
                shed_load_op(Drift::Clobber),
                &["drift: :clobber"],
            )],
        )
    }
}

pub fn shed_load_deferring() -> Plan {
    Plan {
        wane: Some(dur(7200)),
        ..Plan::new(
            "shed_load_deferring",
            "site-ctl",
            vec![with_args(shed_load_op(Drift::Defer), &["drift: :defer"])],
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
