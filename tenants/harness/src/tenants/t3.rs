//! T3: commit-confirmed firewall change (docs/ROADMAP.md section 8.3).
//! Permanent. A region change with reach ssh(host), undo locus :target, a
//! backstop [unless_confirmed: 10m] installed and armed before the change, a
//! reachability probe, confirm(), commit() last; and a Windows variant.

use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![
            host("fw-01", "freebsd", &["ssh"], true),
            host("fw-win-01", "windows", &["ssh"], true),
        ],
        transports: strings(&["ssh"]),
        authenticators: vec![authenticator("netops", true)],
        max_wait: None,
        scheduler_present: strings(&["fw-01", "fw-win-01"]),
    }
}

pub fn pf_allow() -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        reach: strings(&["ssh"]),
        undo_one_line: "strip the rue-mgmt anchor from /etc/pf.conf; pfctl reload".into(),
        ..Op::new(
            "pf_allow",
            vec![FootprintEntry::anchored("file:/etc/pf.conf", "rue-mgmt")],
        )
    }
}

fn winfw_allow() -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        reach: strings(&["ssh"]),
        undo_one_line: "remove the rue-mgmt firewall rule".into(),
        ..Op::new(
            "winfw_allow",
            vec![FootprintEntry::entry(Kind::Owned, "winfw:rule:rue-mgmt")],
        )
    }
}

fn confirmed(owner: &str, change: Op) -> Plan {
    Plan {
        backstop: Some(backstop(vec![Trigger::UnlessConfirmed(dur(600))], 1)),
        ..Plan::new(
            "open_mgmt_port",
            owner,
            vec![
                Item::Step(StepI {
                    args: strings(&["port: 8443"]),
                    ..StepI::new(change)
                }),
                Item::Observe {
                    probe: "verify_reach".into(),
                    alias: "reach".into(),
                },
                Item::Confirm,
                Item::Commit,
            ],
        )
    }
}

pub fn open_mgmt_port() -> Plan {
    confirmed("fw-01", pf_allow())
}

pub fn open_mgmt_port_windows() -> Plan {
    confirmed("fw-win-01", winfw_allow())
}

/// T3's body with the change step swapped, for the negatives derived from it.
pub fn body_with(change: Op) -> Vec<Item> {
    vec![
        Item::Step(StepI::new(change)),
        Item::Observe {
            probe: "verify_reach".into(),
            alias: "reach".into(),
        },
        Item::Confirm,
        Item::Commit,
    ]
}

pub fn tenant() -> Tenant {
    Tenant {
        name: "t3".into(),
        site: site(),
        requester: "netops_requester".into(),
        cases: vec![
            case("fw-01", open_mgmt_port()),
            case("fw-win-01", open_mgmt_port_windows()),
        ],
    }
}
