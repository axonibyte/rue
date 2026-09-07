//! T1: break-glass access (docs/ROADMAP.md section 8.1). Temporary.
//!
//! Four channel ops: a service-posture drop-in as owned with a derived verify;
//! a fenced block in a shared authorized-keys file as region; a
//! management-controller account enable as modified with a secret output and
//! a controller undo, on an API host with no instance directory; a console
//! tunnel as held with suspend/reestablish and a rotated secret. A plan-entry
//! gate through the approval hook; wane 4h with renewal; a :target backstop
//! [after: 4h, unless_heartbeat: 60s] covering the ssh-borne ops, armed after
//! them (reach empty, late arming); scheduler presence as a precondition.

use rue_core::body::*;
use rue_core::model::*;

use super::common::*;

pub fn site() -> Site {
    Site {
        hosts: vec![
            host("db-01", "freebsd", &["ssh"], true),
            host("bmc-01", "appliance", &["api"], false),
        ],
        transports: strings(&["ssh", "api"]),
        authenticators: vec![
            authenticator("oncall", true),
            authenticator("platform_a", true),
        ],
        max_wait: None,
        scheduler_present: strings(&["db-01"]),
        secrets_deliver_to: strings(&["requester", "hook:escrow"]),
    }
}

fn sshd_posture() -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        do_: vec![
            write(
                fact_ref("file:/etc/ssh/sshd_config.d/rue-breakglass.conf"),
                Value::Ref(param("posture")),
            ),
            run_lit("service sshd reload"),
        ],
        post: vec![Guard::new("sshd_posture_applied", Tri::Yes)],
        ..Op::new(
            "service_posture",
            vec![
                FootprintEntry::entry(
                    Kind::Owned,
                    "file:/etc/ssh/sshd_config.d/rue-breakglass.conf",
                ),
                FootprintEntry::entry(Kind::Derived, "probe:sshd_posture"),
            ],
        )
    }
}

fn authorized_keys_block() -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        do_: vec![region_set(
            anchored_ref("file:/root/.ssh/authorized_keys", "rue-breakglass"),
            Value::Ref(param("keys")),
        )],
        ..Op::new(
            "authorized_keys_block",
            vec![FootprintEntry::anchored(
                "file:/root/.ssh/authorized_keys",
                "rue-breakglass",
            )],
        )
    }
}

fn bmc_account() -> Op {
    Op {
        undo_locus: UndoLocus::Controller,
        locus: Locus::Host(HostRef::Static("bmc-01".into())),
        outputs: vec![Output {
            name: "bmc_password".into(),
            secret: true,
        }],
        do_: vec![hook("bmc_enable", vec![("account", lit("breakglass"))])],
        undo: computed(
            vec![hook("bmc_disable", vec![("account", lit("breakglass"))])],
            &["bmc:account:breakglass"],
        ),
        ..Op::new(
            "bmc_account_enable",
            vec![FootprintEntry::entry(
                Kind::Modified,
                "bmc:account:breakglass",
            )],
        )
    }
}

fn vnc_console() -> Op {
    Op {
        undo_locus: UndoLocus::Controller,
        locus: Locus::Controller,
        // F1: the text's `undo: run(...)` carries no undo_pre; the term names
        // the held fact, and the text gains the line (docs/TESTING.md).
        do_: vec![run(vec![
            text("vnc-tunnel up --to "),
            interp(host_field("address")),
        ])],
        undo: computed(vec![run_lit("vnc-tunnel down")], &["proc:vnc_tunnel"]),
        suspend: Some(vec![run_lit("vnc-tunnel suspend")]),
        reestablish: Some(vec![run_lit("vnc-tunnel resume --rotate")]),
        outputs: vec![Output {
            name: "console_secret".into(),
            secret: true,
        }],
        ..Op::new(
            "console_tunnel",
            vec![FootprintEntry::entry(Kind::Held, "proc:vnc_tunnel")],
        )
    }
}

pub fn breakglass() -> Plan {
    Plan {
        gate: Some(plan_gate(auth("oncall"), Some(1800), false)),
        wane: Some(dur(14_400)),
        renew_within: Some(dur(1800)),
        backstop: Some(backstop(
            vec![
                Trigger::After(dur(14_400)),
                Trigger::UnlessHeartbeat {
                    deadline: dur(60),
                    interval: Some(dur(20)),
                },
            ],
            3,
        )),
        exclusivity: Some("breakglass".into()),
        ..Plan::new(
            "breakglass",
            "db-01",
            vec![
                with_args(sshd_posture(), &["posture: PermitRootLogin yes"]),
                with_args(authorized_keys_block(), &["keys: requester_key"]),
                Item::Step(StepI {
                    alias: Some("bmc".into()),
                    ..StepI::new(bmc_account())
                }),
                s(vnc_console()),
            ],
        )
    }
}

pub fn tenant() -> Tenant {
    Tenant {
        name: "t1".into(),
        site: site(),
        requester: "ops_requester".into(),
        cases: vec![case("db-01", breakglass())],
    }
}
