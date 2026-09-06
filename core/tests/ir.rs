//! The plan IR reader: the spelling docs/TESTING.md gives is what this crate
//! reads and writes back unchanged; unknown fields and other versions are
//! refused.

use rue_core::ir::{parse, IrError, IR_VERSION};
use rue_core::json::canonical::encode;
use rue_core::model::*;

// A small plan in canonical form: one step with a knell ack gate, a wait
// factor, a bound-host locus, a heartbeat backstop, and every item kind.
const DOC: &str = r#"{
  "ir_version": 1,
  "plan": {
    "backstop": {
      "arm_before": 1,
      "triggers": [
        {
          "after": 3600
        },
        {
          "unless_heartbeat": {
            "deadline_s": 60,
            "interval_s": null
          }
        }
      ]
    },
    "body": [
      {
        "guards": [
          {
            "force_never": true,
            "name": "ready",
            "value": "unknown"
          }
        ],
        "item": "preflight"
      },
      {
        "alias": "r",
        "item": "observe",
        "probe": "p"
      },
      {
        "guard": {
          "force_never": false,
          "name": "g",
          "value": "yes"
        },
        "item": "assert",
        "on_lapse": "hold",
        "window_s": 30
      },
      {
        "alias": "k",
        "args": [
          "x: 1"
        ],
        "direction": "inverse",
        "force": [
          {
            "guard": "g"
          },
          "drift",
          "unknown"
        ],
        "gate": {
          "single": {
            "wait": {
              "duration_s": 60,
              "weight": 2
            }
          }
        },
        "item": "knell",
        "on_lapse": "revert",
        "op": {
          "drift": "defer",
          "exclusivity": "cls",
          "footprint": [
            {
              "anchor": "a",
              "instance": "i",
              "kind": "region",
              "shape": "file:/x"
            }
          ],
          "handoff_done": "done",
          "has_suspend": true,
          "id": "fence",
          "locus": {
            "host": {
              "bound": "pick"
            }
          },
          "outputs": [
            {
              "name": "tok",
              "secret": true
            }
          ],
          "post": [],
          "pre": [],
          "reach": [
            "ssh"
          ],
          "refusal": {
            "knell": {
              "ack": {
                "gate": {
                  "thresh": {
                    "factors": [
                      {
                        "auth": {
                          "id": "oncall",
                          "weight": 1
                        }
                      },
                      {
                        "humans": {
                          "weight": 1
                        }
                      },
                      {
                        "group": {
                          "expr": {
                            "single": {
                              "humans": {
                                "weight": 1
                              }
                            }
                          },
                          "weight": 1
                        }
                      }
                    ],
                    "n": 2
                  }
                }
              },
              "cost": {
                "none": "why"
              },
              "guard": null
            }
          },
          "undo": "none",
          "undo_closed": false,
          "undo_idempotent": true,
          "undo_locus": "none",
          "undo_one_line": ""
        },
        "window_s": null
      },
      {
        "children": [
          {
            "item": "slot",
            "name": "s"
          }
        ],
        "item": "par"
      },
      {
        "body": [
          {
            "item": "confirm"
          }
        ],
        "form": {
          "over": {
            "list": "guests",
            "max": 4,
            "set_valued": true
          }
        },
        "item": "repeat",
        "var": "g"
      },
      {
        "else": [],
        "guard": {
          "force_never": false,
          "name": "h",
          "value": "no"
        },
        "item": "when",
        "on_lapse": "revert",
        "then": [
          {
            "item": "commit"
          }
        ],
        "window_s": null
      }
    ],
    "exclusivity": null,
    "fires_by_construction": false,
    "gate": {
      "allow_zero_human": false,
      "expr": {
        "single": {
          "auth": {
            "id": "oncall",
            "weight": 1
          }
        }
      },
      "window_s": 1800
    },
    "id": "p",
    "mode": "auto",
    "owner": "db-01",
    "renew_within_s": null,
    "require_journal": "chained",
    "strictness": "warn",
    "wane_s": 3600
  },
  "requester": "req",
  "site": {
    "authenticators": [
      {
        "human": true,
        "id": "oncall"
      }
    ],
    "hosts": [
      {
        "filesystem": true,
        "name": "db-01",
        "os": "freebsd",
        "reach": [
          "ssh"
        ]
      }
    ],
    "max_wait_s": null,
    "scheduler_present": [
      "db-01"
    ],
    "transports": [
      "ssh"
    ]
  }
}
"#;

#[test]
fn the_documented_spelling_parses_and_writes_back_identically() {
    let ir = parse(DOC.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ir.ir_version, IR_VERSION);
    assert_eq!(ir.requester, "req");
    assert_eq!(ir.plan.mode, Mode::Auto);
    assert_eq!(ir.plan.require_journal, Some(JournalRequirement::Chained));
    match &ir.plan.body[3] {
        Item::Knell(s) => {
            assert_eq!(s.direction, Direction::Inverse);
            assert_eq!(
                s.force,
                vec![
                    ForceName::Guard("g".into()),
                    ForceName::Drift,
                    ForceName::Unknown
                ]
            );
            assert_eq!(s.op.locus, Locus::Host(HostRef::Bound("pick".into())));
            assert_eq!(s.op.undo, Undo::NoUndo);
            assert_eq!(s.op.undo_locus, UndoLocus::NoLocus);
            match &s.op.refusal {
                Refusal::Knell {
                    guard: None,
                    cost: Cost::NoCost(r),
                    ack: Ack::Gate(GateExpr::Thresh { n: 2, .. }),
                } => assert_eq!(r, "why"),
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    let back = encode(&serde_json::to_value(&ir).unwrap()).unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), DOC);
}

#[test]
fn an_unknown_field_is_refused() {
    let doc = DOC.replacen(
        "\"owner\": \"db-01\",",
        "\"owner\": \"db-01\",\n    \"colour\": 1,",
        1,
    );
    match parse(doc.as_bytes()) {
        Err(IrError::Json(e)) => assert!(e.to_string().contains("unknown field"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn another_version_is_refused_before_the_shape_is_read() {
    let doc = DOC.replacen(
        "\"ir_version\": 1,",
        "\"ir_version\": 2,\n  \"future\": true,",
        1,
    );
    match parse(doc.as_bytes()) {
        Err(IrError::Version { found: 2 }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn not_json_is_refused() {
    assert!(matches!(parse(b"not json"), Err(IrError::Json(_))));
}
