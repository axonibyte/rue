//! Footprints at runtime, docs/ROADMAP.md 4.3, 5.2 and 7.7: what the engine
//! writes to an instance directory for a step (the marker of every file
//! fact as `do` left it, the snapshots taken before `do`, the manifest of
//! regions held on the host), and the drift decisions it makes from them
//! at undo time, by the same rule the artifact applies when it fires.
//!
//! The formats are the contract of docs/DESIGN.md: `markers/<n>` holds one
//! line `<kind> <path> <sha256>` per file fact; `manifest` one line
//! `region <path> <anchor>` per region; `snapshots/<n>/<k>` the file of
//! footprint entry `k` before step `n` ran. A non-file fact cannot be
//! observed by a script and is undone as if intact.

use std::collections::BTreeMap;

use rue_core::model::{Drift, FootprintEntry, Kind};
use serde::{Deserialize, Serialize};

use crate::region;

/// One fact of a step as `do` left it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub kind: Kind,
    /// The fact's address: its path where it is a file, its shape where it
    /// is not. The `markers/<n>` file an artifact reads carries the file
    /// facts alone (7.7), so the two agree wherever the artifact can see.
    pub path: String,
    /// `missing` when the file was absent.
    pub digest: String,
}

pub const MISSING: &str = "missing";

pub fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Owned => "owned",
        Kind::Region => "region",
        Kind::Modified => "modified",
        Kind::Derived => "derived",
        Kind::AppendOnly => "append_only",
        Kind::Held => "held",
    }
}

fn kind_of(w: &str) -> Option<Kind> {
    Some(match w {
        "owned" => Kind::Owned,
        "region" => Kind::Region,
        "modified" => Kind::Modified,
        "derived" => Kind::Derived,
        "append_only" => Kind::AppendOnly,
        "held" => Kind::Held,
        _ => return None,
    })
}

/// The digest of a file's bytes, or `missing`.
pub fn digest_of(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(b) => region::sha256_hex(b),
        None => MISSING.to_string(),
    }
}

/// `markers/<n>` as text.
pub fn markers_text(markers: &[Marker]) -> String {
    let mut s = String::new();
    for m in markers {
        s.push_str(&format!("{} {} {}\n", kind_word(m.kind), m.path, m.digest));
    }
    s
}

pub fn parse_markers(text: &str) -> Vec<Marker> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            let kind = kind_of(it.next()?)?;
            let path = it.next()?.to_string();
            let digest = it.next()?.to_string();
            Some(Marker { kind, path, digest })
        })
        .collect()
}

/// `manifest` as text: every region this instance holds on the host.
pub fn manifest_text(regions: &[(String, String)]) -> String {
    let mut s = String::new();
    for (path, anchor) in regions {
        s.push_str(&format!("region {path} {anchor}\n"));
    }
    s
}

pub fn parse_manifest(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            if it.next()? != "region" {
                return None;
            }
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// Every fact of a footprint the engine can read back, with the address it
/// is known by: its path where it is a file, its shape where it is not.
///
/// This is the ENGINE's view, and `file_facts` below is the ARTIFACT's. A
/// rendered `sh` backstop can only see files, so 5.2 has it undo a non-file
/// fact as if intact; the engine is under no such limit, because it has an
/// executor and can ask. 5.2's drift table is written about facts and not
/// about files -- "the fact equals its post-`do` value: restore" -- and an
/// appliance's reported state is a `modified` fact reached through a hook.
/// Comparing only the ones that happen to live in a filesystem left such a
/// fact unable to drift at all: `:clobber` never journaled, `:defer` never
/// held, and a value changed under the plan was overwritten in silence.
///
/// A region is exempt and stays file-only, for the reason it always was:
/// it is the text between two markers inside a file, by construction, so a
/// region on anything else is not a fact to compare.
pub fn observed_facts(footprint: &[FootprintEntry]) -> Vec<(usize, &FootprintEntry, String)> {
    footprint
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, Kind::Owned | Kind::Region | Kind::Modified))
        .filter_map(|(k, e)| match region::file_path(&e.shape) {
            Some(p) => Some((k, e, p.to_string())),
            None if e.kind == Kind::Region => None,
            None => Some((k, e, e.shape.clone())),
        })
        .collect()
}

/// The file facts of a footprint with their entry index (`k` names the
/// snapshot): what a rendered artifact can observe, and what the marker
/// file it reads is written from.
pub fn file_facts(footprint: &[FootprintEntry]) -> Vec<(usize, &FootprintEntry, &str)> {
    footprint
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, Kind::Owned | Kind::Region | Kind::Modified))
        .filter_map(|(k, e)| region::file_path(&e.shape).map(|p| (k, e, p)))
        .collect()
}

/// What the engine does with one file fact at undo time (5.2), given its
/// digest now against its marker, whether its region markers are intact,
/// and whether another active instance holds a region on the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Unchanged since `do`: undo as planned.
    Undo,
    /// Changed under `:clobber`: undo anyway, journal DriftClobbered.
    Clobber,
    /// Changed under `:defer`: leave it, journal DriftHeld.
    Defer,
    /// A region whose markers are damaged, no sibling holds one: restore the
    /// whole file from the snapshot, journal DriftClobbered.
    RestoreWhole,
    /// A region whose markers are damaged while a sibling holds one: leave
    /// it, journal DriftHeld (the foreign-region condition).
    DeferForeign,
}

pub fn decide(
    kind: Kind,
    policy: Drift,
    changed: bool,
    region_intact: Option<bool>,
    foreign_region: bool,
) -> Decision {
    match kind {
        Kind::Region => match region_intact {
            // Markers intact: strip regardless of content.
            Some(true) | None => Decision::Undo,
            Some(false) => match policy {
                Drift::Clobber if !foreign_region => Decision::RestoreWhole,
                Drift::Clobber => Decision::DeferForeign,
                Drift::Defer => Decision::Defer,
            },
        },
        _ => {
            if !changed {
                Decision::Undo
            } else {
                match policy {
                    Drift::Clobber => Decision::Clobber,
                    Drift::Defer => Decision::Defer,
                }
            }
        }
    }
}

/// The plan-wide set of observable facts, by shape: what a step's `do`
/// must not touch outside its own footprint (R0201). Digests before and
/// after a `do` are compared over this set minus the step's own facts.
///
/// Keyed by shape rather than by path, so that "never destroy what is not
/// in your footprint" covers the facts that are not files. A step that
/// moved a sibling step's appliance state went unnoticed while this was a
/// map of paths.
pub type Watched = BTreeMap<String, String>;

/// Facts whose digests changed between two watches, by shape.
pub fn changed_facts(before: &Watched, after: &Watched) -> Vec<String> {
    before
        .iter()
        .filter(|(p, d)| after.get(*p) != Some(d))
        .map(|(p, _)| p.clone())
        .collect()
}

pub use rue_core::model::bind_shape;

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn markers_and_manifests_round_trip_in_the_artifact_s_format() {
        let ms = vec![
            Marker {
                kind: Kind::Owned,
                path: "/etc/x".into(),
                digest: "ab".into(),
            },
            Marker {
                kind: Kind::Region,
                path: "/etc/y".into(),
                digest: MISSING.into(),
            },
        ];
        let text = markers_text(&ms);
        assert_eq!(text, "owned /etc/x ab\nregion /etc/y missing\n");
        assert_eq!(parse_markers(&text), ms);
        let regions = vec![("/etc/y".to_string(), "blk".to_string())];
        let m = manifest_text(&regions);
        assert_eq!(m, "region /etc/y blk\n");
        assert_eq!(parse_manifest(&m), regions);
        assert_eq!(parse_manifest("garbage\n"), vec![]);
        assert_eq!(digest_of(None), MISSING);
    }

    #[test]
    fn the_decision_table_is_the_artifact_s() {
        use Decision as D;
        use Drift as P;
        assert_eq!(decide(Kind::Owned, P::Clobber, false, None, false), D::Undo);
        assert_eq!(
            decide(Kind::Owned, P::Clobber, true, None, false),
            D::Clobber
        );
        assert_eq!(
            decide(Kind::Modified, P::Defer, true, None, false),
            D::Defer
        );
        assert_eq!(
            decide(Kind::Modified, P::Defer, false, None, false),
            D::Undo
        );
        // A region: intact markers strip regardless of content.
        assert_eq!(
            decide(Kind::Region, P::Clobber, true, Some(true), false),
            D::Undo
        );
        assert_eq!(
            decide(Kind::Region, P::Defer, true, Some(true), true),
            D::Undo
        );
        // Damaged markers: whole-file restore only with no sibling region.
        assert_eq!(
            decide(Kind::Region, P::Clobber, true, Some(false), false),
            D::RestoreWhole
        );
        assert_eq!(
            decide(Kind::Region, P::Clobber, true, Some(false), true),
            D::DeferForeign
        );
        assert_eq!(
            decide(Kind::Region, P::Defer, true, Some(false), false),
            D::Defer
        );
    }

    #[test]
    fn a_watched_set_names_what_changed() {
        let mut b = Watched::new();
        b.insert("/a".into(), "1".into());
        b.insert("/b".into(), "2".into());
        let mut a = b.clone();
        a.insert("/b".into(), "3".into());
        assert_eq!(changed_facts(&b, &a), vec!["/b".to_string()]);
        a.remove("/a");
        assert_eq!(
            changed_facts(&b, &a),
            vec!["/a".to_string(), "/b".to_string()]
        );
    }
}
