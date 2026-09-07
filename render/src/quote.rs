//! Per-family quoting, docs/ROADMAP.md section 6.4: a value baked into an
//! artifact or interpolated into a `run` string is quoted for the family
//! that will read it, and a value that cannot be quoted safely is E0109.
//!
//! Three families: POSIX `sh` (single quotes, an embedded quote as `'\''`),
//! PowerShell (single quotes, an embedded quote doubled), and a Python
//! string literal (backslash escapes). A NUL cannot be carried by any of
//! them (argv and file paths end at it). A control character other than
//! tab, newline and carriage return inside `sh` or PowerShell single
//! quotes is legal but unreadable and unverifiable in a listing, and is
//! refused there; the Python literal spells it as `\xNN` and accepts it.

use std::fmt;

/// The quoting family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    Posix,
    Powershell,
    Python,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Posix => "sh",
            Family::Powershell => "powershell",
            Family::Python => "python",
        }
    }
}

/// A value that cannot be safely quoted for the family (E0109).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unquotable {
    pub family: Family,
    pub value: String,
    /// The offending character, as the diagnostic names it.
    pub found: char,
}

impl fmt::Display for Unquotable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "value {:?} cannot be safely quoted for {}: contains U+{:04X}",
            self.value,
            self.family.name(),
            self.found as u32
        )
    }
}

fn readable(c: char) -> bool {
    !c.is_control() || matches!(c, '\t' | '\n' | '\r')
}

fn refuse(family: Family, s: &str, bad: impl Fn(char) -> bool) -> Result<(), Unquotable> {
    match s.chars().find(|c| bad(*c)) {
        Some(found) => Err(Unquotable {
            family,
            value: s.to_string(),
            found,
        }),
        None => Ok(()),
    }
}

/// `'…'` with `'\''` for an embedded quote.
pub fn posix(s: &str) -> Result<String, Unquotable> {
    refuse(Family::Posix, s, |c| !readable(c))?;
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}

/// `'…'` with `''` for an embedded quote.
pub fn powershell(s: &str) -> Result<String, Unquotable> {
    refuse(Family::Powershell, s, |c| !readable(c))?;
    Ok(format!("'{}'", s.replace('\'', "''")))
}

/// A Python string literal in single quotes.
pub fn python(s: &str) -> Result<String, Unquotable> {
    refuse(Family::Python, s, |c| c == '\0')?;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    Ok(out)
}

/// Quote for a family.
pub fn quote(family: Family, s: &str) -> Result<String, Unquotable> {
    match family {
        Family::Posix => posix(s),
        Family::Powershell => powershell(s),
        Family::Python => python(s),
    }
}
