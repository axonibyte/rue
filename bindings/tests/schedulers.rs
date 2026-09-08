//! The scheduler bindings against the fake transport (7.3, 7.7): what
//! `cron()`, `task_scheduler()` and `launchd()` would run on a target.
//!
//! `cron()` is executed for real on the guests by the e2e harness; the
//! other two are built and checked here and executed nowhere this phase
//! (no real Windows machine, no macOS host).

use rue_core::model::{HostRecord, Instant};
use rue_engine::executor::{FakeExecutor, FakeHandle, LocusKind, Observation};
use rue_engine::host::Host;
use rue_engine::scheduler::{Job, Presence, Scheduler};

fn host(os: &str) -> Host {
    Host {
        record: HostRecord {
            name: "fw-01".into(),
            os: os.into(),
            reach: vec!["ssh".into()],
            filesystem: true,
            stdin_preamble: true,
            artifact: None,
        },
        address: "10.0.1.1".into(),
        scheduler: Some("cron".into()),
        rue_root: None,
        facts: Default::default(),
    }
}

fn job(language: rue_core::model::ArtifactLanguage, artifact: &str) -> Job {
    Job {
        instance: "i-1".into(),
        artifact: artifact.into(),
        language,
        os: "freebsd".into(),
    }
}

fn fake() -> FakeHandle {
    FakeExecutor::new(LocusKind::Ssh).shared()
}

fn commands(f: &FakeHandle) -> Vec<String> {
    f.calls()
        .iter()
        .flat_map(|c| {
            c.body.iter().map(|p| match p {
                rue_engine::executor::RPrim::Run { cmd, .. } => cmd.text.clone(),
                other => format!("{other:?}"),
            })
        })
        .collect()
}

#[test]
fn cron_edits_one_fenced_region_of_the_crontab_under_the_host_lock() {
    use rue_bindings::cron::Cron;
    let mut f = fake();
    let mut c = Cron;
    let h = host("freebsd");
    let j = job(
        rue_core::model::ArtifactLanguage::Sh,
        "/var/db/rue/instances/i-1/artifact.sh",
    );
    c.install(&mut f, &h, &j).unwrap();
    let cmd = commands(&f).pop().unwrap();
    assert!(
        cmd.contains("crontab -l 2>/dev/null | sed -e '/^# rue-region i-1 begin$/,/^# rue-region i-1 end$/d'"),
        "the instance's own region and no other: {cmd}"
    );
    assert!(
        cmd.contains("'* * * * * /bin/sh /var/db/rue/instances/i-1/artifact.sh'"),
        "{cmd}"
    );
    assert!(cmd.ends_with("| crontab -"), "{cmd}");
    // The whole edit ran under the host lock.
    let events = f.events();
    let lock = events.iter().position(|e| e == "lock").unwrap();
    let run = events.iter().position(|e| e == "run").unwrap();
    let unlock = events.iter().position(|e| e == "unlock").unwrap();
    assert!(lock < run && run < unlock, "{events:?}");

    // Arming writes no crontab: the artifact reads the deadline the
    // engine wrote, and the entry is periodic.
    let before = commands(&f).len();
    c.arm(&mut f, &h, &j, Instant::new(1)).unwrap();
    c.rearm(&mut f, &h, &j, Instant::new(2)).unwrap();
    assert_eq!(commands(&f).len(), before);

    // Disarm strips the region and installs the rest.
    c.disarm(&mut f, &h, &j).unwrap();
    let cmd = commands(&f).pop().unwrap();
    assert!(
        cmd.starts_with("crontab -l") && cmd.ends_with("| crontab -") && !cmd.contains("printf"),
        "{cmd}"
    );

    // Presence is the probe's exit status, three-valued.
    f.observe_as("cron entry for i-1", Observation::yes(""));
    assert_eq!(c.present(&mut f, &h, &j).unwrap(), Presence::Present);
    f.observe_as("cron entry for i-1", Observation::no(""));
    assert_eq!(c.present(&mut f, &h, &j).unwrap(), Presence::Absent);
    f.observe_as("cron entry for i-1", Observation::unknown(""));
    assert_eq!(c.present(&mut f, &h, &j).unwrap(), Presence::Unknown);
}

#[test]
fn the_task_scheduler_names_one_task_per_instance_and_refuses_what_it_cannot_quote() {
    use rue_bindings::task_scheduler::TaskScheduler;
    let mut f = fake();
    let mut t = TaskScheduler;
    let h = host("windows");
    let j = job(
        rue_core::model::ArtifactLanguage::Powershell,
        "C:\\ProgramData\\rue\\instances\\i-1\\artifact.ps1",
    );
    t.install(&mut f, &h, &j).unwrap();
    let cmd = commands(&f).pop().unwrap();
    assert!(
        cmd.contains("schtasks /Create /F /RU SYSTEM /SC MINUTE /MO 1 /TN \"rue-i-1\""),
        "{cmd}"
    );
    assert!(
        cmd.contains("/TR \"powershell -NoProfile -File C:\\ProgramData\\rue\\instances\\i-1\\artifact.ps1\""),
        "{cmd}"
    );
    t.disarm(&mut f, &h, &j).unwrap();
    assert!(commands(&f)
        .pop()
        .unwrap()
        .contains("schtasks /Delete /F /TN \"rue-i-1\""));
    f.observe_as("scheduled task for i-1", Observation::no(""));
    assert_eq!(t.present(&mut f, &h, &j).unwrap(), Presence::Absent);
    // A path with a double quote is refused, never guessed at.
    let bad = job(
        rue_core::model::ArtifactLanguage::Powershell,
        "C:\\a\"b\\artifact.ps1",
    );
    assert!(t.install(&mut f, &h, &bad).is_err());
}

#[test]
fn launchd_writes_its_plist_beside_the_artifact_and_boots_it() {
    use rue_bindings::launchd::Launchd;
    let mut f = fake();
    let mut l = Launchd;
    let h = host("darwin");
    let j = job(
        rue_core::model::ArtifactLanguage::Sh,
        "/var/db/rue/instances/i-1/artifact.sh",
    );
    l.install(&mut f, &h, &j).unwrap();
    let plist = f.with(|x| {
        x.files
            .get(&("fw-01".into(), "i-1".into(), "rue.i-1.plist".into()))
            .cloned()
    });
    let plist = String::from_utf8(plist.expect("the plist is in the instance directory")).unwrap();
    assert!(
        plist.contains("<key>Label</key><string>rue.i-1</string>"),
        "{plist}"
    );
    assert!(plist.contains("<string>/bin/sh</string>"), "{plist}");
    assert!(
        plist.contains("<key>StartInterval</key><integer>60</integer>"),
        "{plist}"
    );
    let cmd = commands(&f).pop().unwrap();
    assert_eq!(
        cmd,
        "launchctl bootstrap system '/var/db/rue/instances/i-1/rue.i-1.plist'"
    );
    l.disarm(&mut f, &h, &j).unwrap();
    assert_eq!(
        commands(&f).pop().unwrap(),
        "launchctl bootout system/rue.i-1"
    );
    assert!(!f.with(|x| x.files.contains_key(&(
        "fw-01".into(),
        "i-1".into(),
        "rue.i-1.plist".into()
    ))));
}
