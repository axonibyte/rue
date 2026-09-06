//! The acceptance tenants (docs/ROADMAP.md section 8) and the negative cases
//! as Rust terms: the record of what Phase 0 proved, and the source of every
//! golden. A tenant is a site, a requester and one plan per host case; a
//! negative is a plan the checker must refuse with exactly one code.

pub mod common;
pub mod negative;
pub mod t1;
pub mod t2;
pub mod t3;
pub mod t4;

pub use common::{Case, Negative, Tenant};

/// The four tenants, in section 8's order.
pub fn tenants() -> Vec<Tenant> {
    vec![t1::tenant(), t2::tenant(), t3::tenant(), t4::tenant()]
}

/// The negative cases, in the case table's order.
pub fn negatives() -> Vec<Negative> {
    negative::cases()
}
