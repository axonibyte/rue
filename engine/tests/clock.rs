//! The clock: the fake moves only by hand and in either direction; the
//! system clock reads wall time in whole seconds and never runs backward
//! between two reads.

use std::sync::Arc;
use std::thread;

use rue_core::model::{Duration, Instant};
use rue_engine::clock::{Clock, FakeClock, SystemClock};

#[test]
fn the_fake_clock_holds_still_advances_by_a_duration_and_can_be_set_back() {
    let c = FakeClock::at(Instant::new(1_000));
    assert_eq!(c.now(), Instant::new(1_000));
    assert_eq!(c.now(), Instant::new(1_000), "nothing moves it but a call");
    c.advance(Duration::new(90));
    assert_eq!(c.now(), Instant::new(1_090));
    c.set(Instant::new(500));
    assert_eq!(c.now(), Instant::new(500), "a target's clock may step back");
}

#[test]
fn the_fake_clock_is_shared_across_threads_as_one_clock() {
    let c = Arc::new(FakeClock::at(Instant::new(0)));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let c = Arc::clone(&c);
            thread::spawn(move || c.advance(Duration::new(1)))
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(c.now(), Instant::new(8));
}

#[test]
fn the_system_clock_reads_wall_time_in_seconds_and_does_not_run_backward() {
    let c = SystemClock;
    let a = c.now();
    let b = c.now();
    // 2026-01-01T00:00:00Z: this crate did not exist before then.
    assert!(a.unix_s >= 1_767_225_600, "wall time is {a:?}");
    assert!(b.unix_s >= a.unix_s);
}
