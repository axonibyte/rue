//! The artifact-language vocabulary, docs/ROADMAP.md sections 4.5 and 7.7:
//! which shell family a host's `os` implies, which language a host's
//! backstop artifact is rendered in, and which pairs have a template.
//!
//! This is OS-family knowledge in core rather than in `rue-render`, the one
//! exception to section 4.5's "three places": the checker refuses a plan
//! whose `:target` backstop has no template (E0403) at check time, and core
//! cannot depend on render. The templates and the quoting stay in render.

use crate::model::{ArtifactLanguage, HostRecord};

/// The shell a host's `run` strings are written in, from its `os`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shell {
    /// `sh`: FreeBSD, Linux, macOS, and any other filesystem host.
    Posix,
    /// Windows.
    Powershell,
}

/// The shell family of an `os` name (Appendix C: a vocabulary the site may
/// extend; every name but `windows` is POSIX).
pub fn shell_of(os: &str) -> Shell {
    if os == "windows" {
        Shell::Powershell
    } else {
        Shell::Posix
    }
}

/// The artifact language a host gets when it declares none: its native
/// shell.
pub fn default_language(os: &str) -> ArtifactLanguage {
    match shell_of(os) {
        Shell::Posix => ArtifactLanguage::Sh,
        Shell::Powershell => ArtifactLanguage::Powershell,
    }
}

/// The language a host's artifact is rendered in: declared, else native.
pub fn language_of(h: &HostRecord) -> ArtifactLanguage {
    h.artifact.unwrap_or_else(|| default_language(&h.os))
}

/// Whether a template exists for the pair: `sh` on a POSIX host,
/// PowerShell on Windows, Python (uv, PEP 723) on either.
pub fn supported(os: &str, language: ArtifactLanguage) -> bool {
    match language {
        ArtifactLanguage::Sh => shell_of(os) == Shell::Posix,
        ArtifactLanguage::Powershell => shell_of(os) == Shell::Powershell,
        ArtifactLanguage::Python => true,
    }
}
