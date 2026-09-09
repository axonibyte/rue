//! The hook protocol's wire, docs/hook-protocol.md v1.
//!
//! One definition of what crosses the boundary between `rued` and a hook,
//! shared by the engine, the SDKs of docs/ROADMAP.md 7.11, the `rue-hook`
//! shim and `rue sdk-conform`. Nothing here does I/O, holds a connection or
//! knows what a hook is *for*: the engine's adapters (`rue_engine::hook`)
//! present a hook of a kind as the engine's trait for that kind, and this
//! crate says only what the frames look like.
//!
//! The pieces:
//!
//! * [`Op`] — every op of 7.5 as data: its kind, its name, the fields a
//!   reply must carry (R0303 otherwise), and whether it is one of the four
//!   messages a secret may travel in. This is the table the conformance
//!   suite drives and the freeze golden is taken from.
//! * [`Registration`] — the frame a hook opens with, and [`HOOK_PROTOCOL`],
//!   the version it must name (R0501 otherwise).
//! * [`body`] — a resolved body: every value already a string with its
//!   secrecy known, which is what `execute.run` carries.
//! * [`record`] — the records a reply returns: an observation, a bootstrap
//!   state, an instance directory listing, a host of an inventory.
//!
//! Secrets cross this boundary in exactly four messages (7.5): toward the
//! hook in `execute.run`'s body and in `secrets.deliver`'s value; toward
//! the engine in `execute.run`'s outputs and in `secrets.resolve`'s value.
//! [`Op::may_carry_secret`] is that rule as data, and the engine's R0305
//! guard reads it rather than repeating it.

pub mod body;
pub mod op;
pub mod record;
pub mod request;

pub use body::{RPrim, Resolved};
pub use op::{Direction, Op, HOOK_PROTOCOL, KINDS, OPS};
pub use record::{
    BootstrapState, InstanceDirState, InventoryHost, Observation, Output, ProbeRun, Registration,
};
