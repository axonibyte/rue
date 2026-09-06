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
