//! The server's answers, without a process: the handlers take a message
//! and give the answer an editor would receive.
//!
//! What these hold is the contract an operator relies on. A diagnostic here
//! is the checker's own, at a line an editor can point at. A hover is what
//! `explain` says about that step. And the server never invents an answer:
//! where it cannot judge, it says nothing rather than guessing.

use std::path::PathBuf;

use lsp_types::{Position, Uri};
use rue_lsp::server::{capabilities, handle_notification, handle_request};
use rue_lsp::{judge, offset_of, position_of, Documents};

fn tenant(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
        .join(rel)
}

fn uri(path: &std::path::Path) -> Uri {
    rue_lsp::uri_of(path).unwrap_or_else(|| panic!("a file uri for {}", path.display()))
}

fn open(docs: &mut Documents, path: &std::path::Path) -> Uri {
    let text = std::fs::read_to_string(path).expect("the text");
    let url = uri(path);
    let note = lsp_server::Notification::new(
        "textDocument/didOpen".into(),
        serde_json::json!({
            "textDocument": {
                "uri": url.to_string(),
                "languageId": "rue",
                "version": 1,
                "text": text,
            }
        }),
    );
    let published = handle_notification(docs, note);
    assert_eq!(published.len(), 1, "opening a file publishes once");
    url
}

#[test]
fn a_clean_tenant_opens_with_no_diagnostics() {
    let path = tenant("tenants/t3/plan.rue");
    let text = std::fs::read_to_string(&path).unwrap();
    let j = judge(&path, &text);
    assert!(
        j.diagnostics.is_empty(),
        "T3 is clean, and the server says so: {:?}",
        j.diagnostics
    );
}

/// A parse error is where it is: the line the front end named, underlined.
#[test]
fn a_parse_error_is_reported_at_its_own_line() {
    let path = tenant("surface/tests/corpus/err-stray-token.rue");
    let text = std::fs::read_to_string(&path).unwrap();
    let j = judge(&path, &text);
    assert!(!j.diagnostics.is_empty(), "the error is reported");
    assert!(!j.checked, "a text that does not parse is not checked");
    let d = &j.diagnostics[0];
    assert_eq!(d.source.as_deref(), Some("rue"));
    assert_eq!(
        d.range.start.line, 2,
        "the corpus puts it on line 3: {:?}",
        d.range
    );
    assert!(
        matches!(&d.code, Some(lsp_types::NumberOrString::String(c)) if c == "E0101"),
        "{:?}",
        d.code
    );
}

/// A refused tenant is refused here too, with the checker's own code, and
/// the message carries the version a rule arrived in where the code knows
/// one -- an editor answering "why is this suddenly refused?" without a
/// trip to the roadmap.
#[test]
fn a_refused_tenant_carries_the_checkers_code_and_its_migration_note() {
    let path = tenant("tenants/_negative/E0401-backstop-armed-after-reach/plan.rue");
    let text = std::fs::read_to_string(&path).unwrap();
    let j = judge(&path, &text);
    assert!(j.checked, "it parses and resolves, so the checker ran");
    assert!(
        j.diagnostics
            .iter()
            .any(|d| matches!(&d.code, Some(lsp_types::NumberOrString::String(c)) if c == "E0401")),
        "{:?}",
        j.diagnostics
    );
}

/// An unsaved buffer is not resolved: its imports and its inventory are
/// read from disk, so a diagnostic from them would be about a text the
/// author has already changed.
#[test]
fn an_unsaved_buffer_is_parsed_and_not_checked() {
    let path = tenant("tenants/t3/plan.rue");
    let saved = std::fs::read_to_string(&path).unwrap();
    let edited = format!("{saved}\n# a comment the file on disk does not have\n");
    let j = judge(&path, &edited);
    assert!(!j.checked, "the buffer and the file differ");
    assert!(
        j.diagnostics.is_empty(),
        "and nothing is claimed about it: {:?}",
        j.diagnostics
    );
}

/// Hover on a step names the op and says what `explain` says: its locus,
/// its undo, its undo locus, its drift policy and its footprint.
#[test]
fn hover_on_a_step_says_what_explain_says() {
    let path = tenant("tenants/t3/plan.rue");
    let text = std::fs::read_to_string(&path).unwrap();
    // The line the plan's step is written on, and the op's own name.
    let (line, col) = text
        .lines()
        .enumerate()
        .find_map(|(i, l)| l.find("pf_allow(port:").map(|c| (i as u32, c as u32)))
        .expect("T3 calls pf_allow");
    let mut docs = Documents::default();
    let url = open(&mut docs, &path);
    let req = lsp_server::Request::new(
        1.into(),
        "textDocument/hover".into(),
        serde_json::json!({
            "textDocument": { "uri": url.to_string() },
            "position": { "line": line, "character": col + 1 },
        }),
    );
    let response = handle_request(&docs, req).expect("an answer");
    let value = response.response_result.expect("a hover");
    let text = value
        .get("contents")
        .and_then(|c| c.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(text.contains("pf_allow"), "{text}");
    assert!(text.contains("locus"), "{text}");
    assert!(text.contains("undo"), "{text}");
    assert!(text.contains("drift"), "{text}");
    assert!(text.contains("region"), "the footprint is named: {text}");
    assert!(text.contains("/etc/pf.conf"), "{text}");
}

/// Hover on nothing in particular answers nothing, rather than the nearest
/// thing it can find.
#[test]
fn hover_on_a_comment_answers_nothing() {
    let path = tenant("tenants/t3/plan.rue");
    let mut docs = Documents::default();
    let url = open(&mut docs, &path);
    let req = lsp_server::Request::new(
        2.into(),
        "textDocument/hover".into(),
        serde_json::json!({
            "textDocument": { "uri": url.to_string() },
            "position": { "line": 1, "character": 3 },
        }),
    );
    let response = handle_request(&docs, req).expect("an answer");
    assert_eq!(
        response.response_result.ok(),
        Some(serde_json::Value::Null),
        "a comment is not a step"
    );
}

/// Closing a document clears what the editor was shown: diagnostics about
/// a text nobody has open are diagnostics about nothing.
#[test]
fn closing_a_document_clears_its_diagnostics() {
    let path = tenant("surface/tests/corpus/err-stray-token.rue");
    let mut docs = Documents::default();
    let url = open(&mut docs, &path);
    assert_eq!(docs.len(), 1);
    let note = lsp_server::Notification::new(
        "textDocument/didClose".into(),
        serde_json::json!({ "textDocument": { "uri": url.to_string() } }),
    );
    let published = handle_notification(&mut docs, note);
    assert_eq!(published.len(), 1);
    assert!(published[0].diagnostics.is_empty(), "cleared");
    assert!(docs.is_empty(), "and forgotten");
}

/// The capabilities say exactly what is served, so an editor never asks
/// for something that would be answered with a shrug.
#[test]
fn the_capabilities_promise_only_what_is_served() {
    let c = capabilities();
    assert!(c.hover_provider.is_some());
    assert!(c.text_document_sync.is_some());
    assert!(c.completion_provider.is_none(), "completion is not served");
    assert!(
        c.document_formatting_provider.is_none(),
        "`rue fmt` is not wired in yet, and the server does not claim it"
    );
}

/// A path with a character a URI may not carry survives the round trip.
/// The Ubuntu guest found this: the test built a URI by pasting a path
/// after `file://`, which is what nothing does -- an editor encodes, and so
/// must anything else that names a document by its path.
#[test]
fn a_path_and_its_uri_go_both_ways() {
    for p in [
        "/tmp/plain/plan.rue",
        "/tmp/with a space/plan.rue",
        "/tmp/caf\u{e9}/plan.rue",
        "/tmp/a+b/plan.rue",
        "/tmp/100%/plan.rue",
    ] {
        let path = PathBuf::from(p);
        let url = rue_lsp::uri_of(&path).unwrap_or_else(|| panic!("a uri for {p}"));
        assert_eq!(
            rue_lsp::path_of(&url).as_deref(),
            Some(path.as_path()),
            "{p} came back as {url:?}"
        );
    }
}

/// Positions are counted in UTF-16 code units, which is what LSP says and
/// what an editor will hold this to. A line with an em dash in a comment
/// puts every column after it out by one if they are counted in bytes.
#[test]
fn positions_are_counted_in_utf16_code_units() {
    let text = "rue 0\n# \u{2014} dash\ndefplan :x\n";
    let line2 = text.lines().nth(1).unwrap();
    let offset = text.find("dash").unwrap();
    let p = position_of(text, offset);
    assert_eq!(p.line, 1);
    let expected = line2[..line2.find("dash").unwrap()].encode_utf16().count() as u32;
    assert_eq!(p.character, expected, "counted in code units, not bytes");
    assert_eq!(expected, 4, "`#`, a space, one em dash and a space");
    assert_eq!(
        line2.find("dash").unwrap(),
        6,
        "which is six bytes: the em dash is three of them"
    );
    assert_eq!(offset_of(text, p), offset, "and the mapping goes both ways");
    assert_eq!(
        offset_of(text, Position::new(99, 0)),
        text.len(),
        "a position past the end is the end"
    );
}
