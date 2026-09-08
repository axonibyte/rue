//! rue-engine: the runtime of docs/ROADMAP.md section 7.
//!
//! Core reasons over declared facts and takes every `now` as an input; the
//! engine is where the clock, the store, the executors and the world live.
//! It arrives by unit (docs/DESIGN.md): this unit brings the clock every
//! later module reads time through, so no module ever calls the system time
//! directly and the simulation and the tests can drive time by hand.

pub mod clock;
