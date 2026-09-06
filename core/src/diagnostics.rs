//! Every diagnostic code of docs/ROADMAP.md section 6.7, as an enum.
//!
//! This is the one place a code exists as text in the Rust workspace:
//! `tools/lint-ecodes.sh` reads the `E0xxx =>` lines below as data, requires
//! them to agree with the roadmap's table and with the prototype's
//! enumeration in both directions, and forbids a raw `"E0xxx"` literal in
//! any other `.rs` file. Codes grow and never renumber (renumbered once,
//! before Phase 0; frozen from Phase 1).

use std::fmt;

macro_rules! codes {
    ($($code:ident => $meaning:literal,)*) => {
        /// A diagnostic code. Ordered as the table is.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Code {
            $($code,)*
        }

        impl Code {
            /// Every code, in table order.
            pub const ALL: &'static [Code] = &[$(Code::$code,)*];

            /// The code's text, `E0xxx`.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Code::$code => stringify!($code),)*
                }
            }

            /// The table's one-line meaning.
            pub fn meaning(self) -> &'static str {
                match self {
                    $(Code::$code => $meaning,)*
                }
            }
        }
    };
}

codes! {
    E0101 => "Parse error (expected/found)",
    E0102 => "Unknown name (with nearest-name suggestion)",
    E0103 => "Duplicate definition, or indistinguishable clauses",
    E0104 => "Import cycle",
    E0105 => "Language version marker missing, or newer than this compiler",
    E0106 => "Non-total construct (closure, recursion, unbounded repeat)",
    E0107 => "Kind mismatch",
    E0108 => "Comparison against :unknown",
    E0109 => "Interpolated value cannot be safely quoted for the target OS family",
    E0110 => "Output referenced before its step, or across a par sibling",
    E0111 => "Clause pattern names a fact not in the host contract",
    E0112 => "No clause matches this host",
    E0113 => "repeat over: list is not set-valued",
    E0114 => "when arms bind an output under different kinds",
    E0201 => "Op without undo is not knell, or vice versa",
    E0202 => ":target undo is not closed over target-local commands and facts",
    E0203 => "undo_locus: :none with an undo body",
    E0204 => "knell without a cost probe or cost: :none reason",
    E0205 => "held footprint without suspend/reestablish",
    E0206 => "Secret-producing op reachable from reestablish",
    E0207 => "Computed or compensating undo without undo_pre",
    E0208 => "Undo not provably idempotent",
    E0209 => "Secret interpolated into a run string",
    E0210 => "Secret referenced from a :target undo body",
    E0211 => "Secret env:/stdin: on an executor that cannot honour the stdin preamble",
    E0301 => "Footprint conflict (umbra)",
    E0302 => "May-conflict (penumbra), strict mode",
    E0303 => "par children not umbra-disjoint",
    E0304 => "reach op inside par",
    E0305 => "Same anchor declared twice on one fact within a plan",
    E0401 => "reach op without a preceding armed :target backstop",
    E0402 => "Renewal would commit before backstop rearm",
    E0403 => "Backstop locus not viable on host, or no artifact template for its OS",
    E0404 => "mode: :auto plan contains force:",
    E0405 => "Heartbeat interval not <= deadline/3",
    E0406 => "Artifact would be installed after a covered step",
    E0407 => ":target undo locus on a host with no run-capable executor",
    E0408 => "Snapshot for a :target undo exceeds the declared cap",
    E0409 => "Multi-host plan without an inferable owner host",
    E0410 => "reach op whose undo is drift: :defer",
    E0411 => "Secret routed to a non-secret sink",
    E0501 => "Intent undeterminable: both wane and commit(), or neither",
    E0502 => "commit() is not the last item on its path",
    E0503 => "Temporary plan's backstop after: differs from its wane",
    E0504 => "Permanent plan with a backstop has a path reaching neither confirm() nor commit()",
    E0505 => "Permanent plan has a non-refusing path that never reaches commit()",
    E0506 => "Unbounded wait: temporary plan without wane can reach Waiting/Held/Deferred, or a permanent plan's wait has neither window: nor a site max_wait",
    E0507 => "mode: :auto plan has a step gate needing a human, or a knell whose ack: is not :none",
    E0508 => "Gate unsatisfiable, names an unknown authenticator, or counts the requester",
    E0509 => "Gate satisfiable with zero human authenticators and no allow_zero_human",
    E0601 => "Unresolved binding",
    E0602 => "Binding contract violation",
    E0603 => "No journal declared; refusing to apply",
    E0604 => "No operators block; refusing to start outside daemon dry-run mode",
    E0605 => "A hook() binding is declared but no hooks registrar block names who may register it",
    E0606 => "Plan has a secret output and the site declares no secrets deliver_to",
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
