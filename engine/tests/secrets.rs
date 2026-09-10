//! Secrets over the built-in acceptors (5.13): delivery at the producing
//! step's completion to the first acceptor that takes it, journaled by
//! label and never by value; a list every acceptor declines is exit 7; a
//! `hold()` gives its value up once to `rue reveal` and drops it at its
//! bound, when the instance ends, and at a daemon restart; and a
//! `hold(until: :wane)` on a permanent plan with no `max_wait` is R0104.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World};
use rue_core::model::{Item, Output as OutputDecl, Plan};
use rue_core::states::State;
use rue_engine::executor::{Output, Scripted};
use rue_engine::lifecycle::ApplyOptions;
use rue_engine::secrets::FakeAcceptor;

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

/// A step whose op declares one secret output, and the executor's answer.
fn secret_step(name: &str) -> Item {
    let mut op = world::op(name);
    op.outputs = vec![OutputDecl {
        name: "token".into(),
        secret: true,
    }];
    world::step(op)
}

fn produce(w: &World, value: &str) {
    let mut outputs = BTreeMap::new();
    outputs.insert("token".to_string(), value.to_string());
    w.ssh.with(|f| {
        f.script.push_back(Scripted::Ok(Output {
            stdout: String::new(),
            outputs,
        }))
    });
}

#[test]
fn a_secret_goes_to_the_first_acceptor_that_takes_it_and_never_to_the_store() {
    let mut w = World::new("sec-first");
    // requester() first with no client attached, then hold().
    let declines = FakeAcceptor::new("requester", false);
    let hold = FakeAcceptor::new("hold", true);
    w.engine.add_acceptor(Box::new(declines.clone()));
    w.engine.add_acceptor(Box::new(hold.clone()));
    produce(&w, "s3cr3t");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(out.exit, 0);
    let id = out.id.clone();
    // The first declined; the second took it, and both were offered it
    // in the order the site declares them.
    assert!(hold.holds(&id), "hold() has it");
    assert_eq!(declines.offers().len(), 1);
    assert_eq!(hold.offers().len(), 1);
    let events = w.events();
    assert!(
        events
            .iter()
            .any(|e| e.contains("SecretRevealed") && e.contains("hold")),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| e.contains("s3cr3t")),
        "the value is never journaled: {events:?}"
    );
    // Nor is it in the record the store holds.
    let rec = w.engine.status(&id).unwrap().unwrap();
    let json = serde_json::to_string(&rec).unwrap();
    assert!(!json.contains("s3cr3t"), "the store never holds a secret");
    // An acceptor that takes it first is the one that gets it, and the
    // second is never offered it.
    let mut w2 = World::new("sec-first-wins");
    let first = FakeAcceptor::new("requester", true);
    let second = FakeAcceptor::new("hold", true);
    w2.engine.add_acceptor(Box::new(first.clone()));
    w2.engine.add_acceptor(Box::new(second.clone()));
    produce(&w2, "other");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let out = w2
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert!(first.holds(&out.id));
    assert!(
        second.offers().is_empty(),
        "the delivery stopped at the first"
    );
}

#[test]
fn a_secret_no_acceptor_takes_is_applied_and_undelivered_at_exit_seven() {
    let mut w = World::new("sec-undelivered");
    // One acceptor, and no client attached: it declines.
    w.engine
        .add_acceptor(Box::new(FakeAcceptor::new("requester", false)));
    produce(&w, "s3cr3t");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(out.exit, 7, "{}", out.line);
    assert!(out.line.contains("secret undelivered"), "{}", out.line);
    assert!(
        w.events().iter().any(|e| e.contains("SecretUndelivered")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_held_secret_is_revealed_once_and_dropped_when_the_instance_ends() {
    let mut w = World::new("sec-reveal");
    let hold = FakeAcceptor::new("hold", true);
    w.engine.add_acceptor(Box::new(hold.clone()));
    produce(&w, "s3cr3t");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let id = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap()
        .id;
    assert_eq!(
        w.engine.reveal(&id).unwrap(),
        Some(("a.token".to_string(), "s3cr3t".to_string()))
    );
    // Once: the second fetch finds nothing.
    assert_eq!(w.engine.reveal(&id).unwrap(), None);
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("SecretRevealed") && e.contains("revealed")),
        "{:?}",
        w.events()
    );

    // A secret still held when the instance reverts is dropped and said.
    let mut w = World::new("sec-drop");
    let hold = FakeAcceptor::new("hold", true);
    w.engine.add_acceptor(Box::new(hold.clone()));
    produce(&w, "s3cr3t");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let id = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap()
        .id;
    assert!(hold.holds(&id));
    w.engine.recant(&id, &[]).unwrap();
    assert!(!hold.holds(&id), "the revert dropped it");
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("SecretDropped") && e.contains("a.token")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_hold_drops_at_its_bound_and_a_restart_holds_nothing() {
    let mut w = World::new("sec-bound");
    let hold = FakeAcceptor::new("hold", true);
    w.engine.add_acceptor(Box::new(hold.clone()));
    produce(&w, "s3cr3t");
    // A temporary plan: the bound is its wane, an hour away.
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let id = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap()
        .id;
    w.advance(3_599);
    w.engine.reap().unwrap();
    assert!(hold.holds(&id), "still inside the bound");
    w.advance(1);
    let report = w.engine.reap().unwrap();
    assert!(!hold.holds(&id), "dropped at the bound");
    assert!(
        report
            .actions
            .iter()
            .any(|a| a.contains("dropped at its bound")),
        "{:?}",
        report.actions
    );

    // A restart holds nothing: boot drops what is left.
    let mut w = World::new("sec-restart");
    let hold = FakeAcceptor::new("hold", true);
    w.engine.add_acceptor(Box::new(hold.clone()));
    produce(&w, "s3cr3t");
    let plan = world::temp_plan("p", vec![secret_step("a")]);
    let id = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap()
        .id;
    assert!(hold.holds(&id));
    let mut w = w.restart();
    w.engine.add_acceptor(Box::new(hold.clone()));
    let boot = w.engine.boot().unwrap();
    assert_eq!(boot.secrets_dropped, 1);
    assert!(!hold.holds(&id));
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("SecretDropped") && e.contains("daemon_restart")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_hold_until_wane_on_a_permanent_plan_with_no_max_wait_is_r0104() {
    let mut w = World::new("sec-r0104");
    w.engine
        .add_acceptor(Box::new(FakeAcceptor::new("hold", true)));
    produce(&w, "s3cr3t");
    let mut plan = Plan::new("p", world::OWNER, vec![secret_step("a"), Item::Commit]);
    plan.wane = None;
    let mut ir = world::ir(plan);
    // A site with no max_wait: the hold has no bound to resolve to.
    ir.site.max_wait = None;
    let out = w.engine.apply(ir, BTreeMap::new(), opts()).unwrap();
    assert_eq!(out.exit, 7, "{}", out.line);
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("SecretDropped") && e.contains("R0104")),
        "{:?}",
        w.events()
    );
}

/// A step whose `do` names `secret(:db_pw)` in an env var: the shape a
/// credential actually reaches a command in (5.13 forbids the command
/// line, so the preamble carries it).
fn step_wanting_a_secret(name: &str) -> Item {
    use rue_core::body::{secret, EnvVar, Part, Prim, Run, Value};
    let mut op = world::op(name);
    op.do_ = vec![Prim::Run(Run {
        cmd: vec![Part::Lit(format!("do {name}"))],
        env: vec![EnvVar {
            name: "PW".into(),
            value: Value::Ref(secret("db_pw")),
        }],
        stdin: None,
    })];
    world::step(op)
}

/// A source that answers one reference and refuses everything else, and
/// records what it was asked.
#[derive(Clone, Default)]
struct FakeSource(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl rue_engine::secrets::Source for FakeSource {
    fn name(&self) -> &str {
        "fake"
    }
    fn resolve(&mut self, reference: &str) -> Result<String, rue_engine::executor::ExecError> {
        self.0.lock().unwrap().push(reference.to_string());
        if reference == "db_pw" {
            Ok("s3cr3t".into())
        } else {
            Err(rue_engine::executor::ExecError::Failed(format!(
                "no secret named {reference}"
            )))
        }
    }
}

#[test]
fn a_secret_in_a_body_is_resolved_from_the_source_just_before_the_step_runs() {
    let mut w = World::new("sec-source");
    let source = FakeSource::default();
    w.engine.set_secret_source(Box::new(source.clone()));
    let plan = world::temp_plan("p", vec![step_wanting_a_secret("a")]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{out:?}");

    // Asked for exactly what the body named, once.
    assert_eq!(*source.0.lock().unwrap(), vec!["db_pw".to_string()]);

    // The value reached the executor on the primitive, marked secret, so
    // every downstream rule about where it may go applies to it.
    let body = w.ssh.with(|f| f.calls[0].body.clone());
    let rue_engine::executor::RPrim::Run { env, .. } = &body[0] else {
        panic!("the step's body is not a run: {body:?}")
    };
    assert_eq!(env[0].0, "PW");
    assert_eq!(env[0].1.text, "s3cr3t");
    assert!(env[0].1.secret, "the value must carry its secrecy");

    // And it is nowhere in the journal.
    let text = format!("{:?}", w.sink.events());
    assert!(!text.contains("s3cr3t"), "the journal holds the value");
}

#[test]
fn a_step_naming_a_secret_with_no_source_declared_is_refused_and_never_runs() {
    // The one outcome nobody wants is a step that runs with a blank where
    // a credential belongs, so a site with no `secrets from:` refuses.
    let mut w = World::new("sec-none");
    let plan = world::temp_plan("p", vec![step_wanting_a_secret("a")]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{out:?}");
    assert!(
        out.line.contains("db_pw") && out.line.contains("secrets from"),
        "{}",
        out.line
    );
    assert!(
        w.ssh.with(|f| f.calls.is_empty()),
        "the step ran without its secret"
    );
}

#[test]
fn a_source_that_refuses_a_reference_refuses_the_step() {
    let mut w = World::new("sec-refuse");
    w.engine.set_secret_source(Box::new(FakeSource::default()));
    let mut item = step_wanting_a_secret("a");
    // Name a reference the source does not hold.
    if let Item::Step(s) = &mut item {
        use rue_core::body::{secret, EnvVar, Part, Prim, Run, Value};
        s.op.do_ = vec![Prim::Run(Run {
            cmd: vec![Part::Lit("do a".into())],
            env: vec![EnvVar {
                name: "PW".into(),
                value: Value::Ref(secret("absent")),
            }],
            stdin: None,
        })];
    }
    let plan = world::temp_plan("p", vec![item]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{out:?}");
    assert!(
        out.line.contains("absent"),
        "the refusal names the reference: {}",
        out.line
    );
    assert!(w.ssh.with(|f| f.calls.is_empty()));
}
