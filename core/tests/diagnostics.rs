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
fn there_are_fifty_six_codes_with_meanings() {
    assert_eq!(Code::ALL.len(), 56);
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
