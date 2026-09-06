//! rue-core: the core model (docs/ROADMAP.md section 5) and everything
//! computed from it -- the checker, the verdict and its prose, `explain`, the
//! runtime state machine and the cross-plan ledger -- as pure functions.
//!
//! This crate reasons only over declared footprints, loci and modes; its
//! theorem is *given honest declarations, the verdict is correct* (section
//! 4.3). It depends on no I/O crate and on nothing else in the workspace. The
//! Phase 0 prototype under `proto/` is the specification this crate
//! transcribes: every golden it produced must be reproduced byte for byte.

pub mod diagnostics;
pub mod ir;
pub mod json;
pub mod model;
pub mod util;
