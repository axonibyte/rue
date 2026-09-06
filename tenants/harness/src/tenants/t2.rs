//! T2: cluster succession (docs/ROADMAP.md section 8.2). Permanent, ending in
//! commit(). Two variants: mode :auto (ack :none) and mode :manual (one
//! human), the manual path alone reaching the destructive rollback knell.

use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![
            host("node-b", "freebsd", &["ssh"], true),
            host("node-a", "freebsd", &["ssh"], true),
            host("node-c", "freebsd", &["console"], true),
        ],
        transports: strings(&["ssh"]),
        authenticators: vec![
            authenticator("operator", true),
            authenticator("second_operator", true),
            authenticator("fence_driver", false),
        ],
        max_wait: Some(dur(1800)),
        scheduler_present: strings(&["node-b"]),
    }
}

fn fence(ack: Ack) -> Op {
    Op {
        undo: Undo::NoUndo,
        undo_locus: UndoLocus::NoLocus,
        refusal: Refusal::Knell {
            guard: Some(Guard::new("fence_verified_off", Tri::Yes)),
            cost: Cost::Probe("fence_verdict".into()),
            ack,
        },
        locus: Locus::Controller,
        undo_one_line: String::new(),
        ..Op::new("fence_corpse", vec![])
    }
}

fn resurrection_gate() -> Op {
    Op {
        refusal: Refusal::Hold {
            via: Some("slave_mode".into()),
        },
        undo_locus: UndoLocus::Controller,
        locus: Locus::Controller,
        undo_one_line: "release the slave-mode gate on node-a".into(),
        ..Op::new(
            "resurrection_gate",
            vec![FootprintEntry::entry(
                Kind::Modified,
                "platform:node-a:mode",
            )],
        )
    }
}

fn start_guest() -> Op {
    Op {
        refusal: Refusal::Hold { via: None },
        undo_locus: UndoLocus::Controller,
        undo_one_line: "stop guest {g} on node-b".into(),
        ..Op::new(
            "start_guest",
            vec![FootprintEntry::entry(Kind::Modified, "guest:{g}:state")],
        )
    }
}

fn zfs_rollback() -> Op {
    Op {
        undo: Undo::NoUndo,
        undo_locus: UndoLocus::NoLocus,
        refusal: Refusal::Knell {
            guard: Some(Guard::new("datasets_ahead", Tri::Yes)),
            cost: Cost::Probe("destroyed_snapshots".into()),
            ack: Ack::Gate(humans()),
        },
        undo_one_line: String::new(),
        ..Op::new("rollback_ahead_datasets", vec![])
    }
}

fn succession_log() -> Op {
    Op {
        undo: Undo::Compensate(strings(&[
            "file:/var/db/succession.log",
            "record:placement",
        ])),
        undo_locus: UndoLocus::Controller,
        locus: Locus::Controller,
        undo_one_line: "append a reversal record (undone by record, not erasure)".into(),
        ..Op::new(
            "record_succession",
            vec![
                FootprintEntry::entry(Kind::AppendOnly, "file:/var/db/succession.log"),
                FootprintEntry::entry(Kind::AppendOnly, "record:placement"),
            ],
        )
    }
}

fn heir_on_other_node() -> Op {
    Op {
        refusal: Refusal::Hold { via: None },
        undo_locus: UndoLocus::Controller,
        locus: Locus::Host(HostRef::Static("node-c".into())),
        handoff_done: Some("heir_running_on_c".into()),
        undo_one_line: "stop the heir on node-c".into(),
        ..Op::new(
            "start_heir",
            vec![FootprintEntry::entry(Kind::Modified, "guest:heir:state")],
        )
    }
}

fn ladder(ack: Ack, manual_only: Vec<Item>) -> Plan {
    let mut body = vec![
        Item::Preflight {
            guards: vec![Guard::new("written_bytes_since_split", Tri::Yes)],
        },
        Item::Assert {
            guard: Guard::new("peer_dead", Tri::Yes),
            window: None,
            on_lapse: OnLapse::Revert,
        },
        Item::Assert {
            guard: Guard {
                name: "probes_agree".into(),
                value: Tri::Yes,
                force_never: true,
            },
            window: None,
            on_lapse: OnLapse::Revert,
        },
        knell(fence(ack)),
    ];
    body.extend(manual_only);
    body.extend(vec![
        s(resurrection_gate()),
        Item::Repeat {
            form: RepeatForm::Over {
                list: "guests".into(),
                max: 16,
                set_valued: true,
            },
            var: "g".into(),
            body: vec![s(start_guest())],
        },
        s(succession_log()),
        s(heir_on_other_node()),
        Item::Commit,
    ]);
    Plan {
        exclusivity: Some("corpse:node-a".into()),
        ..Plan::new("promote", "node-b", body)
    }
}

pub fn promote_auto() -> Plan {
    Plan {
        mode: Mode::Auto,
        id: "promote_auto".into(),
        ..ladder(
            Ack::NoAck("the driver's verified-off is the automation's own evidence".into()),
            vec![],
        )
    }
}

pub fn promote_manual() -> Plan {
    ladder(
        Ack::Gate(humans()),
        vec![Item::When {
            guard: Guard::new("datasets_ahead", Tri::Yes),
            window: None,
            on_lapse: OnLapse::Revert,
            then_: vec![knell(zfs_rollback())],
            else_: vec![],
        }],
    )
}

pub fn tenant() -> Tenant {
    Tenant {
        name: "t2".into(),
        site: site(),
        requester: "operator".into(),
        cases: vec![
            case("node-b-auto", promote_auto()),
            case("node-b-manual", promote_manual()),
        ],
    }
}
