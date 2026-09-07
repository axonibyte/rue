//! Tier 1 for quoting (docs/ROADMAP.md 6.4): every value round-trips through
//! a family-aware unquoter, an embedded quote is escaped the family's way,
//! and what cannot be carried is E0109.

use rue_render::quote::{posix, powershell, python, quote, Family, Unquotable};

/// Read one word the way a POSIX shell reads it: a single-quoted segment
/// ends at the first quote, and a backslash outside quotes escapes one
/// character. The whole input must be one word.
fn unposix(q: &str) -> String {
    let mut out = String::new();
    let mut it = q.chars().peekable();
    let mut quoted = false;
    while let Some(c) = it.next() {
        match (quoted, c) {
            (true, '\'') => quoted = false,
            (true, c) => out.push(c),
            (false, '\'') => quoted = true,
            (false, '\\') => out.push(it.next().expect("a character after the backslash")),
            (false, c) => panic!("{q}: unquoted {c:?} would be a word break or a glob"),
        }
    }
    assert!(!quoted, "{q}: unterminated quote");
    out
}

/// Read one PowerShell single-quoted literal: `''` is a quote, any other
/// quote ends the literal, and nothing may follow it.
fn unpowershell(q: &str) -> String {
    let mut chars = q.chars().peekable();
    assert_eq!(chars.next(), Some('\''), "{q}");
    let mut out = String::new();
    loop {
        match chars.next() {
            Some('\'') => {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    out.push('\'');
                } else {
                    break;
                }
            }
            Some(c) => out.push(c),
            None => panic!("{q}: unterminated literal"),
        }
    }
    assert!(chars.next().is_none(), "{q}: text after the literal");
    out
}

fn unpython(q: &str) -> String {
    assert!(q.starts_with('\'') && q.ends_with('\''), "{q}");
    let body = &q[1..q.len() - 1];
    let mut out = String::new();
    let mut it = body.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next().unwrap() {
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'x' => {
                let h: String = it.by_ref().take(2).collect();
                out.push(char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap());
            }
            other => panic!("unexpected escape \\{other}"),
        }
    }
    out
}

const SAMPLES: &[&str] = &[
    "",
    "plain",
    "with space",
    "it's",
    "''",
    "a'b'c",
    "$HOME `pwd` \"quoted\"",
    "back\\slash",
    "tab\tnew\nline\r",
    "unicode: ☃ ñ",
    "; rm -rf / #",
    "$(reboot)",
    "%USERPROFILE% & del",
];

#[test]
fn every_sample_round_trips_in_every_family() {
    for s in SAMPLES {
        assert_eq!(unposix(&posix(s).unwrap()), *s, "posix {s:?}");
        assert_eq!(
            unpowershell(&powershell(s).unwrap()),
            *s,
            "powershell {s:?}"
        );
        assert_eq!(unpython(&python(s).unwrap()), *s, "python {s:?}");
    }
}

#[test]
fn the_exact_spellings() {
    assert_eq!(posix("it's").unwrap(), "'it'\\''s'");
    assert_eq!(powershell("it's").unwrap(), "'it''s'");
    assert_eq!(python("it's\n\\").unwrap(), "'it\\'s\\n\\\\'");
    assert_eq!(python("\u{1}").unwrap(), "'\\x01'");
    assert_eq!(quote(Family::Posix, "x").unwrap(), "'x'");
}

#[test]
fn nothing_inside_a_quoted_value_ends_the_quote_early() {
    // A value that tries to close the quote and inject a command comes out
    // as one literal argument in every family.
    let hostile = "'; reboot; echo '";
    assert_eq!(posix(hostile).unwrap(), "''\\''; reboot; echo '\\'''");
    assert_eq!(powershell(hostile).unwrap(), "'''; reboot; echo '''");
    assert_eq!(python(hostile).unwrap(), "'\\'; reboot; echo \\''");
}

#[test]
fn a_nul_is_e0109_everywhere_and_a_raw_control_is_e0109_in_the_shells() {
    for f in [Family::Posix, Family::Powershell, Family::Python] {
        match quote(f, "a\0b") {
            Err(Unquotable { family, found, .. }) => {
                assert_eq!(family, f);
                assert_eq!(found, '\0');
            }
            other => panic!("{f:?}: {other:?}"),
        }
    }
    assert!(posix("\u{1b}[31m").is_err());
    assert!(powershell("\u{7f}").is_err());
    assert_eq!(python("\u{1b}").unwrap(), "'\\x1b'");
    // Tab, newline and carriage return are readable and accepted.
    assert!(posix("a\tb\nc\r").is_ok());
    assert!(powershell("a\tb\nc\r").is_ok());
    let e = posix("bad\u{1}").unwrap_err();
    assert!(e.to_string().contains("U+0001"), "{e}");
}
