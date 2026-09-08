//! The file sink: one JSON entry a line, on disk before the acknowledgement,
//! readable back as the same chain; an unwritable path refuses (R0304 at
//! the engine). The key binding loads a signer.

use std::path::PathBuf;

use rue_bindings::journal::{key, FileSink};
use rue_core::journal::{append, Event, Hash};
use rue_core::model::Instant;
use rue_engine::journal::Sink;
use rue_engine::store::read_ndjson;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "rue-bindings-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_file_sink_appends_one_entry_a_line_and_the_lines_read_back_as_the_chain() {
    let d = TempDir::new("file-sink");
    let path = d.0.join("journal.ndjson");
    let mut sink = FileSink::new(&path);
    assert!(sink.name().contains("journal.ndjson"));
    let e1 = append(&[], Instant::new(1), "p", "i", "h", Event::Checked, vec![]);
    let e2 = append(
        std::slice::from_ref(&e1),
        Instant::new(2),
        "p",
        "i",
        "h",
        Event::Requested,
        vec![],
    );
    sink.deliver(&e1).unwrap();
    sink.deliver(&e2).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.ends_with('\n'));
    let back = read_ndjson(&path).unwrap();
    assert_eq!(back, vec![e1.clone(), e2]);
    assert_eq!(back[0].prev_hash, Hash::ZERO);
    rue_core::journal::verify(&back).unwrap();
}

#[test]
fn an_unwritable_path_refuses_with_the_reason() {
    let d = TempDir::new("file-sink-refuse");
    let mut sink = FileSink::new(&d.0.join("no-such-dir").join("journal.ndjson"));
    let e = append(&[], Instant::new(1), "p", "i", "h", Event::Checked, vec![]);
    let err = sink.deliver(&e).unwrap_err();
    assert!(err.contains("no-such-dir"), "{err}");
}

#[test]
fn the_key_binding_loads_an_ed25519_signer() {
    let d = TempDir::new("key");
    let path = d.0.join("id_ed25519");
    rue_engine::sign::generate(&path).unwrap();
    let signer = key(&path).unwrap();
    assert!(signer.public_openssh().starts_with("ssh-ed25519 "));
    assert!(key(&d.0.join("missing")).is_err());
}
