//! The code enumeration: well formed, distinct, ascending, with a meaning
//! each. The agreement with the roadmap's table and the prototype is the
//! tools/lint-ecodes.sh guard's job.

use rue_core::diagnostics::Code;

#[test]
fn every_code_is_e_followed_by_four_digits() {
    for c in Code::ALL {
        let s = c.as_str();
        assert_eq!(s.len(), 5, "{s}");
        assert!(s.starts_with('E'), "{s}");
        assert!(s[1..].chars().all(|ch| ch.is_ascii_digit()), "{s}");
        assert_eq!(c.to_string(), s);
    }
}

#[test]
fn codes_are_distinct_and_ascending() {
    let names: Vec<&str> = Code::ALL.iter().map(|c| c.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        names, sorted,
        "codes must be distinct and in ascending order"
    );
}

#[test]
fn there_are_fifty_nine_codes_with_meanings() {
    assert_eq!(Code::ALL.len(), 59);
    for c in Code::ALL {
        assert!(!c.meaning().is_empty(), "{c} has no meaning");
    }
}

mod front_end {
    use rue_core::diagnostics::*;

    #[test]
    fn codes_serialize_as_their_text_and_refuse_others() {
        let s = serde_json::to_string(&Code::E0102).unwrap();
        assert_eq!(s, format!("\"{}\"", Code::E0102));
        assert_eq!(serde_json::from_str::<Code>(&s).unwrap(), Code::E0102);
        assert!(serde_json::from_str::<Code>("\"E9999\"").is_err());
        assert_eq!(Code::parse(Code::E0606.as_str()), Some(Code::E0606));
    }

    #[test]
    fn a_diagnostic_renders_its_span_expected_found_and_nearest() {
        let d = Diagnostic {
            code: Code::E0102,
            span: Some(Span {
                file: "plan.rue".into(),
                line: 12,
                col: 5,
            }),
            expected: None,
            found: Some("unknown name shed_lod".into()),
            nearest: Some("shed_load".into()),
            message: "unknown name".into(),
        };
        assert_eq!(d.render(), format!("plan.rue:12:5: {}: unknown name; found unknown name shed_lod; did you mean shed_load?", Code::E0102));
        let bare = Diagnostic {
            code: Code::E0501,
            span: None,
            expected: Some("wane or commit()".into()),
            found: None,
            nearest: None,
            message: "no intent".into(),
        };
        assert_eq!(
            bare.render(),
            format!("{}: no intent; expected wane or commit()", Code::E0501)
        );
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(
            json["code"],
            serde_json::Value::String(Code::E0102.to_string())
        );
        assert_eq!(serde_json::from_value::<Diagnostic>(json).unwrap(), d);
    }

    #[test]
    fn nearest_suggests_within_two_edits_first_on_a_tie() {
        let names = ["shed_load", "start_guest", "fence_corpse"];
        assert_eq!(nearest("shed_lod", names), Some("shed_load".into()));
        assert_eq!(nearest("sheD_loAd", names), Some("shed_load".into()));
        assert_eq!(nearest("unrelated", names), None);
        assert_eq!(nearest("shd_lod", names), Some("shed_load".into()));
        assert_eq!(
            nearest("sh_lod", names),
            None,
            "three edits away is no suggestion"
        );
        assert_eq!(nearest("ab", ["abc", "abd"]), Some("abc".into()));
    }
}

/// The codes of v0.1.0, the first release, as that release's
/// `core/src/diagnostics.rs` listed them.
const IN_V0_1_0: &[&str] = &[
    "E0101", "E0102", "E0103", "E0104", "E0105", "E0106", "E0107", "E0108", "E0109", "E0110",
    "E0111", "E0112", "E0113", "E0114", "E0201", "E0202", "E0203", "E0204", "E0205", "E0206",
    "E0207", "E0208", "E0209", "E0210", "E0211", "E0301", "E0302", "E0303", "E0304", "E0305",
    "E0401", "E0402", "E0403", "E0404", "E0405", "E0406", "E0407", "E0408", "E0409", "E0410",
    "E0411", "E0501", "E0502", "E0503", "E0504", "E0505", "E0506", "E0507", "E0508", "E0509",
    "E0601", "E0602", "E0603", "E0604", "E0605", "E0606",
];

#[test]
fn every_code_added_since_the_first_release_says_when_and_how_to_migrate() {
    // A text written for an earlier release that a newer rule refuses is
    // told that the rule is new and what to change; a code the first
    // release already had carries no such note.
    for c in Code::ALL {
        let old = IN_V0_1_0.contains(&c.as_str());
        assert_eq!(c.since().is_none(), old, "{c}: since {:?}", c.since());
        assert_eq!(
            c.migration().is_none(),
            old,
            "{c}: migration {:?}",
            c.migration()
        );
        let m = c.with_migration("m".into());
        if old {
            assert_eq!(m, "m");
        } else {
            assert!(m.starts_with("m (new in v") && m.ends_with(')'), "{m}");
        }
    }
}
