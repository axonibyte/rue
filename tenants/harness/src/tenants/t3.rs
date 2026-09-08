//! T3: commit-confirmed firewall change (docs/ROADMAP.md section 8.3).
//! Permanent. A region change with reach ssh(host), undo locus :target, a
//! backstop [unless_confirmed: 10m] installed and armed before the change, a
//! reachability probe, confirm(), commit() last; and a Windows variant.

use rue_core::body::*;
use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![
            host("fw-01", "freebsd", &["ssh"], true),
            host("fw-win-01", "windows", &["ssh"], true),
            // The same two firewalls with their artifacts in Python, and a
            // macOS pf host on the default sh: one plan, five artifacts.
            python_artifact(host("fw-02", "freebsd", &["ssh"], true)),
            python_artifact(host("fw-win-02", "windows", &["ssh"], true)),
            host("fw-mac-01", "macos", &["ssh"], true),
        ],
        transports: strings(&["ssh"]),
        authenticators: vec![authenticator("netops", true)],
        max_wait: None,
        scheduler_present: strings(&["fw-01", "fw-win-01", "fw-02", "fw-win-02", "fw-mac-01"]),
        secrets_deliver_to: vec![],
    }
}

pub fn pf_allow() -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        reach: strings(&["ssh"]),
        do_: vec![
            region_set(
                anchored_ref("file:/etc/pf.conf", "rue-mgmt"),
                Value::Template(vec![
                    text("pass in proto tcp to port "),
                    interp(param("port")),
                ]),
            ),
            run_lit("pfctl -f /etc/pf.conf"),
        ],
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
        do_: vec![run(vec![
            text("New-NetFirewallRule -Name rue-mgmt -Direction Inbound -Protocol TCP -LocalPort "),
            interp(param("port")),
            text(" -Action Allow"),
        ])],
        undo: computed(
            vec![run_lit("Remove-NetFirewallRule -Name rue-mgmt")],
            &["winfw:rule:rue-mgmt"],
        ),
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
        with_args(change, &["port: 8443"]),
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
            case(
                "fw-02",
                Plan {
                    owner: "fw-02".into(),
                    ..open_mgmt_port()
                },
            ),
            case(
                "fw-win-02",
                Plan {
                    owner: "fw-win-02".into(),
                    ..open_mgmt_port_windows()
                },
            ),
            case(
                "fw-mac-01",
                Plan {
                    owner: "fw-mac-01".into(),
                    ..open_mgmt_port()
                },
            ),
        ],
    }
}
