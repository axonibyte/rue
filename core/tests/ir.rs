//! The plan IR reader: the spelling docs/TESTING.md gives is what this crate
//! reads and writes back unchanged; unknown fields and other versions are
//! refused.

use rue_core::ir::{parse, IrError, IR_VERSION};
use rue_core::json::canonical::encode;
use rue_core::model::*;

// A small plan in canonical form: one step with a knell ack gate, a wait
// factor, a bound-host locus, a heartbeat backstop, and every item kind.
const DOC: &str = r#"{
  "ir_version": 5,
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
          "do": [
            {
              "run": {
                "cmd": [
                  {
                    "lit": "fence "
                  },
                  {
                    "ref": {
                      "param": "node"
                    }
                  }
                ],
                "env": [
                  {
                    "name": "TOKEN",
                    "value": {
                      "ref": {
                        "output": {
                          "name": "tok",
                          "secret": true,
                          "step": "k"
                        }
                      }
                    }
                  }
                ],
                "stdin": {
                  "ref": {
                    "secret": "fence_key"
                  }
                }
              }
            },
            {
              "write": {
                "content": {
                  "template": [
                    {
                      "lit": "fenced by "
                    },
                    {
                      "ref": {
                        "host": "name"
                      }
                    }
                  ]
                },
                "fact": {
                  "anchor": null,
                  "shape": "file:/x"
                }
              }
            },
            {
              "hook": {
                "args": [
                  {
                    "name": "node",
                    "value": {
                      "ref": {
                        "controller": "g"
                      }
                    }
                  }
                ],
                "name": "fence"
              }
            },
            {
              "call": {
                "args": [
                  {
                    "class": "target_local",
                    "name": "n",
                    "value": {
                      "lit": "sshd"
                    }
                  }
                ],
                "prim": "svc",
                "run": [
                  {
                    "lit": "service restart sshd"
                  }
                ]
              }
            },
            {
              "stage": {
                "content": {
                  "ref": {
                    "fact": "snapshot"
                  }
                },
                "mode": 493,
                "name": "restore.sh"
              }
            },
            {
              "region_set": {
                "content": {
                  "lit": "x"
                },
                "fact": {
                  "anchor": "a",
                  "shape": "file:/x"
                }
              }
            },
            {
              "region_clear": {
                "fact": {
                  "anchor": "a",
                  "shape": "file:/x"
                }
              }
            },
            {
              "append": {
                "fact": {
                  "anchor": null,
                  "shape": "file:/log"
                },
                "line": {
                  "lit": "done"
                }
              }
            },
            {
              "remove": {
                "fact": {
                  "anchor": null,
                  "shape": "file:/x"
                }
              }
            },
            {
              "install": {
                "name": "backstop"
              }
            },
            {
              "release": {
                "name": "backstop"
              }
            }
          ],
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
          "reestablish": [
            {
              "run": {
                "cmd": [
                  {
                    "lit": "tunnel resume"
                  }
                ],
                "env": [],
                "stdin": null
              }
            }
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
          "suspend": [],
          "undo": "none",
          "undo_idempotent": true,
          "undo_locus": "none"
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
      },
      {
        "alias": null,
        "args": [],
        "direction": "forward",
        "force": [],
        "gate": null,
        "item": "step",
        "on_lapse": "revert",
        "op": {
          "do": [
            {
              "write": {
                "content": {
                  "ref": {
                    "param": "posture"
                  }
                },
                "fact": {
                  "anchor": null,
                  "shape": "file:/y"
                }
              }
            }
          ],
          "drift": null,
          "exclusivity": null,
          "footprint": [
            {
              "anchor": null,
              "instance": null,
              "kind": "owned",
              "shape": "file:/y"
            }
          ],
          "handoff_done": null,
          "id": "posture",
          "locus": "target",
          "outputs": [],
          "post": [],
          "pre": [],
          "reach": [],
          "reestablish": null,
          "refusal": "revert",
          "suspend": null,
          "undo": {
            "computed": {
              "body": [
                {
                  "run": {
                    "cmd": [
                      {
                        "lit": "restore-posture"
                      }
                    ],
                    "env": [],
                    "stdin": null
                  }
                }
              ],
              "undo_pre": [
                "file:/y"
              ]
            }
          },
          "undo_idempotent": true,
          "undo_locus": "target"
        },
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
    "probes": [
      {
        "body": [
          {
            "run": {
              "cmd": [
                {
                  "lit": "sshd -T"
                }
              ],
              "env": [],
              "stdin": null
            }
          }
        ],
        "equivalence": "bytes",
        "locus": "target",
        "name": "posture",
        "produces": [
          "posture"
        ],
        "static": false
      },
      {
        "body": [
          {
            "run": {
              "cmd": [
                {
                  "lit": "jls -j "
                },
                {
                  "ref": {
                    "controller": "g"
                  }
                },
                {
                  "lit": " jid"
                }
              ],
              "env": [],
              "stdin": null
            }
          }
        ],
        "equivalence": "bytes",
        "locus": "target",
        "name": "guest_state",
        "produces": [],
        "reads": "guest:state:{g}",
        "static": false
      }
    ],
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
        "artifact": "python",
        "filesystem": true,
        "name": "db-01",
        "os": "freebsd",
        "reach": [
          "ssh"
        ],
        "stdin_preamble": true
      }
    ],
    "max_wait_s": null,
    "scheduler_present": [
      "db-01"
    ],
    "secrets_deliver_to": [
      "requester"
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
    assert_eq!(ir.plan.probes.len(), 2);
    assert_eq!(ir.plan.probes[0].produces, vec!["posture".to_string()]);
    assert_eq!(
        ir.plan.probes[0].reads, None,
        "absent when a probe reads nothing"
    );
    assert_eq!(ir.plan.probes[1].reads.as_deref(), Some("guest:state:{g}"));
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
    match &ir.plan.body[7] {
        Item::Step(s) => {
            assert_eq!(s.op.do_.len(), 1);
            assert_eq!(
                s.op.undo,
                Undo::Computed {
                    body: vec![rue_core::body::run_lit("restore-posture")],
                    undo_pre: vec!["file:/y".into()],
                }
            );
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
        &format!("\"ir_version\": {IR_VERSION},"),
        &format!("\"ir_version\": {},\n  \"future\": true,", IR_VERSION + 1),
        1,
    );
    match parse(doc.as_bytes()) {
        Err(IrError::Version { found }) if found == IR_VERSION + 1 => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn not_json_is_refused() {
    assert!(matches!(parse(b"not json"), Err(IrError::Json(_))));
}
