//! The three templates over one action list. Each renders the same
//! structure: a header naming the plan, instance and host; the baked
//! constants; the fired guard; the trigger test; helpers; one block per
//! covered step in reverse order; the fired marker.

pub mod powershell;
pub mod python;
pub mod sh;

use crate::Context;

/// The first comment line of every artifact.
pub(crate) fn banner(ctx: &Context<'_>) -> String {
    format!(
        "rue backstop artifact: plan {} on {} (os {}), instance {}, language {}. Rendered by rue-render; do not edit.",
        ctx.plan.id,
        ctx.host.name,
        ctx.host.os,
        ctx.instance.id,
        ctx.language.name()
    )
}
