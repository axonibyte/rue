//! Tier 4 fuzz for the renderer: over seeded random sites and plans,
//! `render` never panics for any host of the site, and when it renders,
//! the text carries no secret label. Shares core's generator by path.

#[path = "../../core/tests/common/gen.rs"]
mod gen;

use gen::*;
use rue_core::body::{Prim, Ref, Value};
use rue_core::model::*;
use rue_render::{render, Bindings, Instance};

const STEPS: u32 = 500;

/// Every reference a plan mentions, in any body of any op.
fn refs_of(plan: &Plan) -> Vec<Ref> {
    fn values(p: &Prim) -> Vec<Value> {
        match p {
            Prim::Run(r) => {
                let mut v = vec![Value::Template(r.cmd.clone())];
                v.extend(r.env.iter().map(|e| e.value.clone()));
                v.extend(r.stdin.clone());
                v
            }
            Prim::Write(w) => vec![w.content.clone()],
            Prim::Append(a) => vec![a.line.clone()],
            Prim::RegionSet(r) => vec![r.content.clone()],
            Prim::Stage(s) => vec![s.content.clone()],
            Prim::Hook(h) => h.args.iter().map(|a| a.value.clone()).collect(),
            Prim::Call(c) => {
                let mut v = vec![Value::Template(c.run.clone())];
                v.extend(c.args.iter().map(|a| a.value.clone()));
                v
            }
            Prim::Remove(_) | Prim::RegionClear(_) | Prim::Install(_) | Prim::Release(_) => vec![],
        }
    }
    let mut out = Vec::new();
    for (_, it) in rue_core::algebra::numbered(&plan.body) {
        let Some(o) = rue_core::algebra::op_of(it) else {
            continue;
        };
        let mut bodies = vec![&o.do_];
        if let Undo::Computed { body, .. } | Undo::Compensate { body, .. } = &o.undo {
            bodies.push(body);
        }
        bodies.extend(o.suspend.iter());
        bodies.extend(o.reestablish.iter());
        for b in bodies {
            for p in b {
                for v in values(p) {
                    out.extend(v.refs().into_iter().cloned());
                }
            }
        }
    }
    out
}

/// Every secret label a plan mentions.
fn secret_labels(plan: &Plan) -> Vec<String> {
    refs_of(plan)
        .into_iter()
        .filter_map(|r| match r {
            Ref::Secret(n) => Some(format!("secret:{n}")),
            _ => None,
        })
        .collect()
}

#[test]
fn render_never_panics_and_never_bakes_a_secret() {
    let mut rendered = 0u32;
    let mut refused = 0u32;
    each_step(0x5EED_0004, STEPS, |rng, _| {
        let site = gen_site(rng);
        let plan = gen_plan(rng, &site);
        // Bind every parameter the plan names, as a request would.
        let mut b = Bindings::default();
        for r in refs_of(&plan) {
            if let Ref::Param(n) = r {
                b.params.insert(n.clone(), format!("v-{n}"));
            }
        }
        b.host_fields.insert("address".into(), "10.0.0.1".into());
        let inst = Instance {
            id: "fuzz".into(),
            rue_root: rng.maybe(|_| "/tmp/r".to_string()),
        };
        for h in &site.hosts {
            match render(&site, &plan, &h.name, &inst, &b) {
                Ok(a) => {
                    rendered += 1;
                    assert!(!a.text.is_empty() && a.text.ends_with('\n'));
                    for label in secret_labels(&plan) {
                        assert!(
                            !a.text.contains(&label),
                            "{}: {label} reached the artifact",
                            h.name
                        );
                    }
                }
                Err(_) => refused += 1,
            }
        }
        let _ = render(&site, &plan, "ghost", &inst, &b);
    });
    eprintln!("fuzz: rendered {rendered}, refused {refused}");
    // The property is not vacuous: a fair share of the generated plans
    // carry a :target backstop the renderer accepts.
    assert!(
        rendered >= refused / 20,
        "rendered {rendered} of {}",
        rendered + refused
    );
}
