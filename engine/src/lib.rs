//! rue-engine: the runtime of docs/ROADMAP.md section 7.
//!
//! Core reasons over declared facts and takes every `now` as an input; the
//! engine is where the clock, the store, the executors and the world live.
//! It arrives by unit (docs/DESIGN.md): this unit brings the clock every
//! later module reads time through, so no module ever calls the system time
//! directly and the simulation and the tests can drive time by hand.

pub mod backstop;
pub mod clock;
pub mod control;
pub mod executor;
pub mod footprint;
pub mod hook;
pub mod host;
pub mod journal;
pub mod lifecycle;
pub mod peer;
pub mod region;
pub mod resolve;
pub mod scheduler;
pub mod sign;
pub mod store;
