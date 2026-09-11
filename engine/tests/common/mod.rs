//! Shared by the engine's integration tests: a temporary directory that is
//! removed on drop, and an Ed25519 key pair written in OpenSSH format
//! without touching anything of the user's.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "rue-engine-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

/// A directory the test could not remove is a failure of the test, not a
/// thing to discard: a discarded error here left hundreds of directories in
/// /tmp, each a race some thread of the test had lost. Not while already
/// panicking, so the first failure is the one reported.
impl Drop for TempDir {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if !std::thread::panicking() => {
                panic!("{} was not removed: {e}", self.0.display())
            }
            Err(_) => {}
        }
    }
}

/// An unencrypted Ed25519 key pair under `dir`, generated in-process (no
/// ssh-keygen, nothing of the user's read); the path of the private key.
pub fn keypair(dir: &Path, name: &str) -> PathBuf {
    let key = dir.join(name);
    rue_engine::sign::generate(&key).expect("generate");
    key
}

pub mod world;
