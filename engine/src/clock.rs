//! The clock: every `now` the engine passes to core comes from here.
//!
//! `SystemClock` reads the wall clock in whole seconds, the unit `Instant`
//! carries (section 5.9: observations take `now`; expiry is a closed
//! boundary decided by core, never here). `FakeClock` is set and advanced
//! by tests and by the simulation, so a deadline, a settle window or a
//! heartbeat interval is exercised without waiting for it.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rue_core::model::{Duration, Instant};

/// A source of `now`. Shared between threads, so it is `Send + Sync`.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// The wall clock, in whole seconds since the Unix epoch.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        // A wall clock before 1970 is a misconfigured host; the engine reads
        // it as the epoch rather than panicking in a thread it owns.
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Instant::new(secs)
    }
}

/// A clock a test moves by hand. It never advances on its own.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Instant>,
}

impl FakeClock {
    pub fn at(now: Instant) -> FakeClock {
        FakeClock {
            now: Mutex::new(now),
        }
    }

    /// Jump to an instant; going backward is allowed, since a target whose
    /// clock steps back is one of the cases the engine must survive.
    pub fn set(&self, now: Instant) {
        *self.now.lock().unwrap_or_else(|e| e.into_inner()) = now;
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        *now = now.plus(by);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }
}
