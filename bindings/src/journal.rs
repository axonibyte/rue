//! The generic journal sinks (7.6): `file(path)` appends one JSON entry a
//! line and syncs it before acknowledging, so an acknowledged entry is on
//! disk; `stdout()` writes the same line to the process's stdout and
//! acknowledges when the write and flush succeed. Either refuses by
//! returning the error, which the engine reports as R0304. `key(path)`
//! loads the Ed25519 signing key the engine signs with.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use rue_core::journal::Entry;
use rue_engine::journal::Sink;
use rue_engine::sign::Signer;

fn line_of(e: &Entry) -> Result<Vec<u8>, String> {
    let mut line = serde_json::to_vec(e).map_err(|e| e.to_string())?;
    line.push(b'\n');
    Ok(line)
}

/// `journal to: file("/var/log/rue.ndjson")`.
#[derive(Debug, Clone)]
pub struct FileSink {
    path: PathBuf,
}

impl FileSink {
    pub fn new(path: &Path) -> FileSink {
        FileSink {
            path: path.to_path_buf(),
        }
    }
}

impl Sink for FileSink {
    fn name(&self) -> String {
        format!("file({})", self.path.display())
    }

    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        let line = line_of(e)?;
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .map_err(|err| format!("{}: {err}", self.path.display()))?;
        f.write_all(&line)
            .and_then(|()| f.sync_all())
            .map_err(|err| format!("{}: {err}", self.path.display()))
    }
}

/// `journal to: stdout()`.
#[derive(Debug, Clone, Default)]
pub struct StdoutSink;

impl Sink for StdoutSink {
    fn name(&self) -> String {
        "stdout()".into()
    }

    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        let line = line_of(e)?;
        let out = std::io::stdout();
        let mut lock = out.lock();
        lock.write_all(&line)
            .and_then(|()| lock.flush())
            .map_err(|err| format!("stdout: {err}"))
    }
}

/// `journal ... sign: key("/etc/rue/journal_ed25519")`.
pub fn key(path: &Path) -> Result<Signer, String> {
    Signer::load(path)
}
