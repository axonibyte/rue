//! Journal signing, docs/ROADMAP.md 5.10: an Ed25519 key in OpenSSH format
//! held by the engine signs each entry's canonical body as an SSHSIG in the
//! namespace `rue-journal`; `rue journal verify --key` checks every
//! signature with the public half. The chain verifies without any of this;
//! signing is what a reader outside the engine's process can check.

use std::fmt::Write as _;
use std::path::Path;

use rue_core::journal::{self, body_bytes, Entry, Sig, DOMAIN};
use ssh_encoding::{Decode, Encode};
use ssh_key::{HashAlg, PrivateKey, PublicKey, SshSig};

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// The engine's signing key.
pub struct Signer {
    key: PrivateKey,
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signer({})", self.public_openssh())
    }
}

impl Signer {
    /// Read an OpenSSH private key file (unencrypted; Ed25519).
    pub fn load(path: &Path) -> Result<Signer, String> {
        let key =
            PrivateKey::read_openssh_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if key.is_encrypted() {
            return Err(format!("{}: the key is encrypted", path.display()));
        }
        if key.algorithm() != ssh_key::Algorithm::Ed25519 {
            return Err(format!(
                "{}: {} is not Ed25519",
                path.display(),
                key.algorithm()
            ));
        }
        Ok(Signer { key })
    }

    pub fn public_openssh(&self) -> String {
        self.key
            .public_key()
            .to_openssh()
            .unwrap_or_else(|e| format!("<unencodable public key: {e}>"))
    }

    pub fn sign(&self, e: &Entry) -> Result<Sig, String> {
        let sig = SshSig::sign(&self.key, DOMAIN, HashAlg::Sha256, &body_bytes(e))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        sig.encode(&mut bytes).map_err(|e| e.to_string())?;
        Ok(Sig {
            namespace: DOMAIN.to_string(),
            bytes_hex: hex(&bytes),
        })
    }
}

/// Generate an Ed25519 key pair at `path` (private, mode 0600 on Unix) and
/// `path.pub`, in OpenSSH format, and load it. No `ssh-keygen`, no
/// `~/.ssh`.
pub fn generate(path: &Path) -> Result<Signer, String> {
    let key = PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519)
        .map_err(|e| e.to_string())?;
    key.write_openssh_file(path, ssh_key::LineEnding::LF)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let public = key.public_key().to_openssh().map_err(|e| e.to_string())?;
    let pub_path = path.with_extension("pub");
    std::fs::write(&pub_path, format!("{public}\n"))
        .map_err(|e| format!("{}: {e}", pub_path.display()))?;
    Ok(Signer { key })
}

pub fn load_public(path: &Path) -> Result<PublicKey, String> {
    PublicKey::read_openssh_file(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse_public(text: &str) -> Result<PublicKey, String> {
    PublicKey::from_openssh(text).map_err(|e| e.to_string())
}

/// Verify one entry's signature with a public key.
pub fn verify_entry(pk: &PublicKey, e: &Entry) -> Result<(), String> {
    let sig = e
        .sig
        .as_ref()
        .ok_or_else(|| format!("entry {} is unsigned", e.seq))?;
    if sig.namespace != DOMAIN {
        return Err(format!(
            "entry {}: namespace {} is not {DOMAIN}",
            e.seq, sig.namespace
        ));
    }
    let bytes = unhex(&sig.bytes_hex).map_err(|m| format!("entry {}: {m}", e.seq))?;
    let s = SshSig::decode(&mut bytes.as_slice()).map_err(|m| format!("entry {}: {m}", e.seq))?;
    pk.verify(DOMAIN, &body_bytes(e), &s)
        .map_err(|m| format!("entry {}: signature does not verify: {m}", e.seq))
}

/// The chain end to end, and every signature when a key is given: with a
/// key, an unsigned entry is a failure, since a signed journal that admits
/// unsigned entries proves nothing.
pub fn verify_chain(entries: &[Entry], key: Option<&PublicKey>) -> Result<(), String> {
    journal::verify(entries).map_err(|e| e.to_string())?;
    if let Some(pk) = key {
        for e in entries {
            verify_entry(pk, e)?;
        }
    }
    Ok(())
}
