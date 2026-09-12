//! `rue-lsp`: diagnostics as a text is edited, and hover on a step
//! (ROADMAP Phase 5; `docs/issues/0006`).
//!
//! The server computes nothing of its own. A diagnostic here is a
//! diagnostic `rue check` would give, from the same front end and the same
//! checker, and a hover is what `rue explain` prints for that step. An
//! editor that invented its own answer -- a second opinion about a plan --
//! would be the worst thing this could be: an operator would learn which
//! of the two to believe, and it would not always be the right one.
//!
//! What the server does own is the *degree* to which it can answer. A file
//! open in an editor is often not resolvable: its inventory is elsewhere,
//! its host is chosen at the command line, it imports a file being written.
//! So there are two levels, and the server says which one it reached:
//! parse diagnostics always, and the checker's verdict when the file
//! resolves to a plan on its own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::{
    Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkupContent, MarkupKind, Position,
    Range, Uri,
};
use rue_core::check::check;
use rue_core::diagnostics::Diagnostic as RueDiagnostic;

pub mod server;

/// The texts the editor has open, by URL. An editor's buffer is the truth
/// while it is open -- what is on disk is what the *last save* said -- so
/// everything here reads from this map and not from the filesystem.
#[derive(Debug, Default)]
pub struct Documents(HashMap<Uri, String>);

impl Documents {
    pub fn open(&mut self, url: Uri, text: String) {
        self.0.insert(url, text);
    }

    pub fn change(&mut self, url: Uri, text: String) {
        self.0.insert(url, text);
    }

    pub fn close(&mut self, url: &Uri) {
        self.0.remove(url);
    }

    pub fn text(&self, url: &Uri) -> Option<&str> {
        self.0.get(url).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A byte offset to a line and a UTF-16 code unit, which is what LSP
/// positions are counted in. A rue text is usually ASCII and then this is
/// the offset; it is not always, and a comment with an em dash in it would
/// put every column after it out by one.
pub fn position_of(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = text[line_start..offset].encode_utf16().count() as u32;
    Position { line, character }
}

/// The reverse: a position to a byte offset, for a hover request.
pub fn offset_of(text: &str, at: Position) -> usize {
    let mut offset = 0;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        if n as u32 == at.line {
            let mut units = 0u32;
            for (i, c) in line.char_indices() {
                if units >= at.character {
                    return offset + i;
                }
                units += c.len_utf16() as u32;
            }
            return offset + line.len();
        }
        offset += line.len();
    }
    text.len()
}

/// A diagnostic's span is a line and a column, both counted from one; LSP
/// counts both from zero, and the end of the range is the end of that
/// line, because the front end locates a diagnostic at a point and an
/// editor has to underline something.
fn range_of(text: &str, d: &RueDiagnostic) -> Range {
    match &d.span {
        Some(span) => {
            let line = span.line.saturating_sub(1);
            let start = span.col.saturating_sub(1);
            let width = text
                .lines()
                .nth(line as usize)
                .map(|l| l.encode_utf16().count() as u32)
                .unwrap_or(start + 1);
            Range {
                start: Position::new(line, start),
                end: Position::new(line, width.max(start + 1)),
            }
        }
        // A diagnostic with no span is about the file: the first line is
        // where an editor can show it without pretending to know more.
        None => Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        },
    }
}

fn lsp_diagnostic(text: &str, d: &RueDiagnostic) -> Diagnostic {
    Diagnostic {
        range: range_of(text, d),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(lsp_types::NumberOrString::String(format!("{:?}", d.code))),
        source: Some("rue".into()),
        message: message_of(d),
        ..Default::default()
    }
}

/// The message an operator reads, with what the code's own documentation
/// adds: the version a rule arrived in, and the migration note where the
/// code carries one, so an editor answers "why is this suddenly refused?"
/// without a trip to the roadmap.
fn message_of(d: &RueDiagnostic) -> String {
    let mut m = d.message.clone();
    if let Some(since) = d.code.since() {
        m.push_str(&format!("\n\n(new in {since})"));
    }
    if let Some(note) = d.code.migration() {
        m.push_str(&format!("\n{note}"));
    }
    m
}

/// A verdict's diagnostic locates by step, not by span: the checker judges
/// a plan, and a plan's steps are numbered, not placed. An editor needs a
/// place, so the step's own line is found in the text -- the Nth item of
/// the plan body -- and the code and message are the checker's own.
fn checked_diagnostic(text: &str, d: &rue_core::verdict::Diagnostic) -> Diagnostic {
    let line = d.step.and_then(|n| step_line(text, n)).unwrap_or(0);
    let width = text
        .lines()
        .nth(line as usize)
        .map(|l| l.encode_utf16().count() as u32)
        .unwrap_or(1);
    let mut message = d.message.clone();
    if let Some(since) = d.code.since() {
        message.push_str(&format!("\n\n(new in {since})"));
    }
    if let Some(note) = d.code.migration() {
        message.push_str(&format!("\n{note}"));
    }
    Diagnostic {
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, width.max(1)),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(lsp_types::NumberOrString::String(format!("{:?}", d.code))),
        source: Some("rue".into()),
        message,
        ..Default::default()
    }
}

/// Where a plan's step `n` is written. The body's items are one per line by
/// the formatter's own layout (6.9), so the nth item line inside the last
/// `defplan` is the step -- an approximation the editor can act on, and the
/// only one available while a verdict locates by number.
fn step_line(text: &str, n: u32) -> Option<u32> {
    let mut in_plan = false;
    let mut seen = 0u32;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("defplan ") {
            in_plan = true;
            seen = 0;
            continue;
        }
        if !in_plan || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t == "end" {
            in_plan = false;
            continue;
        }
        // The plan's option lines are not items.
        if [
            "wane ",
            "gate ",
            "backstop ",
            "require ",
            "mode:",
            "strictness:",
            "exclusivity:",
            "fires_by_construction:",
        ]
        .iter()
        .any(|k| t.starts_with(k))
        {
            continue;
        }
        seen += 1;
        if seen == n {
            return Some(i as u32);
        }
    }
    None
}

/// What the server could say about a text, and how far it got.
pub struct Judgment {
    pub diagnostics: Vec<Diagnostic>,
    /// True when the checker ran: the file resolved to a plan on its own.
    /// False means these are parse and resolve diagnostics alone, and the
    /// absence of others means nothing.
    pub checked: bool,
}

/// Judge one text: parse it, resolve it if it can be resolved, and check
/// the plan if there is one.
///
/// `path` is where the text lives, because an import and an inventory are
/// resolved relative to it. The text is the editor's buffer, not the file,
/// so a resolve that reads the file from disk sees the last save -- which
/// is why the parse half always runs on the buffer.
pub fn judge(path: &Path, text: &str) -> Judgment {
    let parsed = rue_surface::parse(text, &path.to_string_lossy());
    if !parsed.diagnostics.is_empty() {
        return Judgment {
            diagnostics: parsed
                .diagnostics
                .iter()
                .map(|d| lsp_diagnostic(text, d))
                .collect(),
            checked: false,
        };
    }
    // The text parses. Resolving needs the file on disk, its imports and
    // its inventory; an editor's buffer that has not been saved resolves
    // to the saved text, so a resolve diagnostic is reported only when the
    // buffer and the file agree. Anything else would underline a line the
    // author has already fixed.
    let saved = std::fs::read_to_string(path).ok();
    if saved.as_deref() != Some(text) {
        return Judgment {
            diagnostics: Vec::new(),
            checked: false,
        };
    }
    match resolve_for_editor(path) {
        Ok((ir, host)) => {
            let verdict = check(&ir.site, &ir.requester, &ir.plan);
            Judgment {
                diagnostics: verdict
                    .diagnostics
                    .iter()
                    .map(|d| {
                        let mut lsp = checked_diagnostic(text, d);
                        if let Some(h) = &host {
                            lsp.message.push_str(&format!(
                                "\n\n(this file dispatches by clause; \
                                     checked for host {h})"
                            ));
                        }
                        lsp
                    })
                    .collect(),
                checked: true,
            }
        }
        Err(diags) => Judgment {
            diagnostics: diags.iter().map(|d| lsp_diagnostic(text, d)).collect(),
            checked: false,
        },
    }
}

/// Resolve a file the way an editor has to: with no command line.
///
/// A plan whose clauses dispatch on the host cannot be resolved without
/// one (E0112), and that is not a defect in the text -- it is an argument
/// `rue check` is given and an editor is not. The same goes for a hook
/// inventory, which needs a record named (E0607). Rather than underline
/// every clause-dispatched tenant in the project, the server names a host
/// itself -- the first the file's own inventory lists -- and says so in
/// every diagnostic it then reports. What it must never do is report
/// E0112 as though the author had written something wrong.
///
/// The host it picks is returned so the caller can say which one it is.
/// An answer about one host, labelled, is worth having; an unlabelled one
/// would be a claim about a file that has no single answer.
pub fn resolve_for_editor(
    path: &Path,
) -> Result<(rue_core::ir::PlanIr, Option<String>), Vec<RueDiagnostic>> {
    let opts = rue_surface::resolve::Options::default();
    let first = match rue_surface::resolve::resolve(path, &opts) {
        Ok(ir) => return Ok((ir, None)),
        Err(diags) => diags,
    };
    let needs_a_host = first
        .iter()
        .all(|d| matches!(d.code, rue_core::diagnostics::Code::E0112));
    if !needs_a_host || first.is_empty() {
        return Err(first);
    }
    let Ok(bindings) = rue_surface::resolve::site_bindings(path) else {
        return Err(first);
    };
    for host in bindings.inventory.hosts.iter().map(|h| h.name.clone()) {
        let opts = rue_surface::resolve::Options {
            host: Some(host.clone()),
            ..Default::default()
        };
        if let Ok(ir) = rue_surface::resolve::resolve(path, &opts) {
            return Ok((ir, Some(host)));
        }
    }
    Err(first)
}

/// Hover: the op under the cursor, as `explain` describes it.
///
/// The name under the cursor is looked up among the file's own `defop`s
/// through the resolved plan, so what is shown is the op rue would run --
/// arguments bound, clause chosen -- and not the text of a definition that
/// may not be the one this host takes.
pub fn hover(path: &Path, text: &str, at: Position) -> Option<Hover> {
    let offset = offset_of(text, at);
    let (word, range) = word_at(text, offset)?;
    let saved = std::fs::read_to_string(path).ok();
    if saved.as_deref() != Some(text) {
        return None;
    }
    let (ir, for_host) = resolve_for_editor(path).ok()?;
    let op = rue_core::algebra::numbered(&ir.plan.body)
        .into_iter()
        .filter_map(|(_, it)| rue_core::algebra::op_of(it))
        .find(|o| o.id == word)?;
    let mut md = format!("**{}**\n\n", op.id);
    md.push_str(&format!("- locus: `{}`\n", locus_of(&op.locus)));
    md.push_str(&format!(
        "- undo: `{}`\n",
        match &op.undo {
            rue_core::model::Undo::NoUndo => format!(
                "NO UNDO \u{2014} knell; {}",
                rue_core::explain::undo_line(op)
            ),
            _ => rue_core::explain::undo_line(op),
        }
    ));
    md.push_str(&format!("- undo locus: `{:?}`\n", op.undo_locus));
    md.push_str(&format!(
        "- drift: `{}`\n",
        op.effective_drift()
            .map(|d| format!("{d:?}").to_lowercase())
            .unwrap_or_else(|| "n/a".into())
    ));
    if op.footprint.is_empty() {
        md.push_str("- footprint: none\n");
    } else {
        md.push_str("- footprint:\n");
        for e in &op.footprint {
            md.push_str(&format!(
                "  - `{}`: `{}`\n",
                format!("{:?}", e.kind).to_lowercase(),
                e.shape
            ));
        }
    }
    if let Some(h) = for_host {
        md.push_str(&format!(
            "\nThis file dispatches by clause; shown for host `{h}`.\n"
        ));
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(Range {
            start: position_of(text, range.0),
            end: position_of(text, range.1),
        }),
    })
}

fn locus_of(l: &rue_core::model::Locus) -> String {
    match l {
        rue_core::model::Locus::Controller => "controller".into(),
        rue_core::model::Locus::Target => "target".into(),
        rue_core::model::Locus::Host(rue_core::model::HostRef::Static(h)) => format!("host({h})"),
        rue_core::model::Locus::Host(rue_core::model::HostRef::Bound(b)) => {
            format!("host({b}) bound at runtime")
        }
    }
}

/// The identifier around a byte offset, and where it starts and ends.
fn word_at(text: &str, offset: usize) -> Option<(String, (usize, usize))> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '?';
    if offset > text.len() {
        return None;
    }
    let start = text[..offset]
        .rfind(|c| !is_word(c))
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = text[offset..]
        .find(|c| !is_word(c))
        .map(|i| offset + i)
        .unwrap_or(text.len());
    if start >= end {
        return None;
    }
    Some((text[start..end].to_string(), (start, end)))
}

/// A URI as a path, for the file a request names. `lsp_types::Uri` is a
/// URI and not a URL: it does not know about files, so the `file:` scheme
/// is unwrapped here and anything else is refused rather than guessed at.
pub fn path_of(url: &Uri) -> Option<PathBuf> {
    if url.scheme().map(|s| s.as_str()) != Some("file") {
        return None;
    }
    let path = url.path().as_str();
    let decoded = percent_decode(path);
    Some(PathBuf::from(decoded))
}

/// A path as a `file:` URI, percent-encoding every character a URI may not
/// carry unescaped. An editor does this itself; what needs it here is
/// anything that has a path and wants to name a document -- a test, or a
/// future verb that opens one.
pub fn uri_of(path: &Path) -> Option<Uri> {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out.parse().ok()
}

/// `%20` and its kin, which an editor writes into a URI for any path with
/// a space in it.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
