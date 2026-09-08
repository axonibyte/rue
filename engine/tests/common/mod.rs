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

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
