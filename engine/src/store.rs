//! The instance store, docs/ROADMAP.md 7.1 and 7.13: locked, atomic,
//! versioned.
//!
//! ```text
//! <store>/schema             the schema version, as text
//! <store>/lock               held exclusively by the daemon that owns the store
//! <store>/instances/<id>.json  one canonical-JSON record per instance
//! <store>/ledger.json        the cross-plan reservations (section 5.12)
//! <store>/journal.ndjson     the engine's own copy of the chain, one entry a line
//! ```
//!
//! Every file is written to a temporary name beside it and renamed, so a
//! crash leaves the previous record, never half of the next. The lock is
//! taken at open and held for the store's life: a second daemon on the same
//! store is refused, not raced. The schema is checked before anything is
//! read; an unknown or newer schema is R0502 and nothing is migrated
//! silently: `rued migrate` is explicit and dry-runnable.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rue_core::interference::Fact;
use rue_core::journal::Entry;
use rue_core::json::canonical;
use rue_core::ledger::{Instance as Held, Ledger};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// The schema this build writes and reads.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// No schema file: a store from before schemas were written (schema 0),
    /// or not a store at all.
    Missing,
    /// A schema file that is not a number.
    Unreadable(String),
    /// A schema this build does not know (newer, or from a different
    /// lineage).
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Another daemon holds the store.
    Locked(PathBuf),
    /// R0502.
    Schema(SchemaError),
    NotOwned {
        path: PathBuf,
        owner: u32,
        me: u32,
    },
    Io(String),
    Corrupt(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Locked(p) => write!(f, "store {} is held by another daemon", p.display()),
            StoreError::Schema(SchemaError::Missing) => write!(
                f,
                "R0502: the store has no schema (schema 0); run `rued migrate`"
            ),
            StoreError::Schema(SchemaError::Unreadable(s)) => {
                write!(f, "R0502: the store's schema file is unreadable: {s}")
            }
            StoreError::Schema(SchemaError::Unknown(v)) => write!(
                f,
                "R0502: store schema {v} is unknown to this build (which knows {SCHEMA}); a newer rued wrote it"
            ),
            StoreError::NotOwned { path, owner, me } => write!(
                f,
                "store {} is owned by uid {owner}, not the daemon account (uid {me}); refusing to migrate",
                path.display()
            ),
            StoreError::Io(s) => write!(f, "store i/o: {s}"),
            StoreError::Corrupt(s) => write!(f, "store corrupt: {s}"),
        }
    }
}

impl std::error::Error for StoreError {}

fn io(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

/// Write bytes to `path` through a temporary name beside it and a rename,
/// with the file synced before the rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("rue-tmp");
    {
        let mut f = File::create(&tmp).map_err(io)?;
        f.write_all(bytes).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    fs::rename(&tmp, path).map_err(io)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StoreError> {
    let bytes = fs::read(path).map_err(io)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| StoreError::Corrupt(format!("{}: {e}", path.display())))
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), StoreError> {
    let value = serde_json::to_value(v).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    let bytes = canonical::encode(&value).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    write_atomic(path, &bytes)
}

/// The schema a store directory carries.
pub fn schema_of(root: &Path) -> Result<u32, SchemaError> {
    let text = match fs::read_to_string(root.join("schema")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(SchemaError::Missing),
        Err(e) => return Err(SchemaError::Unreadable(e.to_string())),
    };
    text.trim()
        .parse::<u32>()
        .map_err(|_| SchemaError::Unreadable(text.trim().to_string()))
}

#[cfg(unix)]
fn lock_exclusive(f: &File) -> Result<bool, StoreError> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: flock on a descriptor this File owns; the flags are constants.
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(true)
    } else {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(io(e))
        }
    }
}

#[cfg(windows)]
fn lock_exclusive(_f: &File) -> Result<bool, StoreError> {
    // The lock file is opened with no sharing (see `open_lock`), which is
    // the exclusive open Windows offers; reaching here means it succeeded.
    Ok(true)
}

/// Open the store's lock file; the refusal names the store, not the file.
fn open_lock(root: &Path) -> Result<File, StoreError> {
    let path = root.join("lock");
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(0);
    }
    match o.open(path) {
        Ok(f) => Ok(f),
        // ERROR_SHARING_VIOLATION (32): another process holds the file open
        // with no sharing, which is the lock.
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(32) => {
            Err(StoreError::Locked(root.to_path_buf()))
        }
        Err(e) => Err(io(e)),
    }
}

/// A ledger as the store keeps it: the same reservations core's `Ledger`
/// holds, in a serializable shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerRecord {
    pub held: Vec<HeldRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldRecord {
    pub id: String,
    pub host: String,
    pub umbra: Vec<(String, Option<String>)>,
    pub exclusivity: Option<String>,
}

impl LedgerRecord {
    pub fn from_ledger(l: &Ledger) -> LedgerRecord {
        LedgerRecord {
            held: l
                .holdings()
                .iter()
                .map(|h| HeldRecord {
                    id: h.id.clone(),
                    host: h.host.clone(),
                    umbra: h
                        .umbra
                        .iter()
                        .map(|f| (f.shape.clone(), f.anchor.clone()))
                        .collect(),
                    exclusivity: h.exclusivity.clone(),
                })
                .collect(),
        }
    }

    /// Rebuild core's ledger. Holdings never conflict with each other (they
    /// were admitted one by one), so re-admission in reverse order cannot
    /// refuse; a refusal here is a corrupt record.
    pub fn to_ledger(&self) -> Result<Ledger, StoreError> {
        let mut l = Ledger::new();
        for h in self.held.iter().rev() {
            l = l
                .request(Held {
                    id: h.id.clone(),
                    host: h.host.clone(),
                    umbra: h
                        .umbra
                        .iter()
                        .map(|(s, a)| Fact::new(s, a.as_deref()))
                        .collect(),
                    exclusivity: h.exclusivity.clone(),
                    rehearsal: false,
                })
                .map_err(|(c, m)| StoreError::Corrupt(format!("ledger: {c:?} {m}")))?;
        }
        Ok(l)
    }
}

/// An open store: the lock is held until it is dropped.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    _lock: File,
}

impl Store {
    /// Create a store at `root` (which may exist and be empty) and open it.
    pub fn create(root: &Path) -> Result<Store, StoreError> {
        fs::create_dir_all(root.join("instances")).map_err(io)?;
        match schema_of(root) {
            Err(SchemaError::Missing) => {}
            Ok(v) => {
                return Err(StoreError::Corrupt(format!(
                    "{} is already a store (schema {v})",
                    root.display()
                )))
            }
            Err(e) => return Err(StoreError::Schema(e)),
        }
        write_atomic(&root.join("schema"), format!("{SCHEMA}\n").as_bytes())?;
        Store::open(root)
    }

    /// Open an existing store: the schema must be this build's, and the
    /// lock must be free.
    pub fn open(root: &Path) -> Result<Store, StoreError> {
        let v = schema_of(root).map_err(StoreError::Schema)?;
        if v != SCHEMA {
            return Err(StoreError::Schema(SchemaError::Unknown(v)));
        }
        let lock = open_lock(root)?;
        if !lock_exclusive(&lock)? {
            return Err(StoreError::Locked(root.to_path_buf()));
        }
        fs::create_dir_all(root.join("instances")).map_err(io)?;
        Ok(Store {
            root: root.to_path_buf(),
            _lock: lock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn instance_path(&self, id: &str) -> PathBuf {
        self.root.join("instances").join(format!("{id}.json"))
    }

    pub fn write_instance<T: Serialize>(&self, id: &str, rec: &T) -> Result<(), StoreError> {
        write_json(&self.instance_path(id), rec)
    }

    pub fn read_instance<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>, StoreError> {
        let p = self.instance_path(id);
        if !p.exists() {
            return Ok(None);
        }
        read_json(&p).map(Some)
    }

    pub fn remove_instance(&self, id: &str) -> Result<(), StoreError> {
        match fs::remove_file(self.instance_path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }

    /// Every instance id, sorted.
    pub fn instance_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut ids = Vec::new();
        for e in fs::read_dir(self.root.join("instances")).map_err(io)? {
            let e = e.map_err(io)?;
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".json") {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn write_ledger(&self, l: &Ledger) -> Result<(), StoreError> {
        write_json(
            &self.root.join("ledger.json"),
            &LedgerRecord::from_ledger(l),
        )
    }

    pub fn read_ledger(&self) -> Result<Ledger, StoreError> {
        let p = self.root.join("ledger.json");
        if !p.exists() {
            return Ok(Ledger::new());
        }
        read_json::<LedgerRecord>(&p)?.to_ledger()
    }

    /// Append one entry to the engine's copy of the chain, synced.
    pub fn append_journal(&self, e: &Entry) -> Result<(), StoreError> {
        let mut line = serde_json::to_vec(e).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        line.push(b'\n');
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.root.join("journal.ndjson"))
            .map_err(io)?;
        f.write_all(&line).map_err(io)?;
        f.sync_all().map_err(io)
    }

    pub fn read_journal(&self) -> Result<Vec<Entry>, StoreError> {
        read_ndjson(&self.root.join("journal.ndjson"))
    }

    pub fn journal_tail(&self) -> Result<Option<Entry>, StoreError> {
        Ok(self.read_journal()?.pop())
    }

    /// Small named values the engine keeps beside the instances (the settle
    /// flag, the signing key's public half, ...).
    pub fn write_meta(&self, name: &str, v: &BTreeMap<String, String>) -> Result<(), StoreError> {
        write_json(&self.root.join(format!("{name}.json")), v)
    }

    pub fn read_meta(&self, name: &str) -> Result<BTreeMap<String, String>, StoreError> {
        let p = self.root.join(format!("{name}.json"));
        if !p.exists() {
            return Ok(BTreeMap::new());
        }
        read_json(&p)
    }
}

/// Entries from a newline-delimited JSON file; a missing file is empty.
pub fn read_ndjson(path: &Path) -> Result<Vec<Entry>, StoreError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(e)),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            serde_json::from_str(l)
                .map_err(|e| StoreError::Corrupt(format!("{} line {}: {e}", path.display(), i + 1)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Migration (section 7.13)

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub from: u32,
    pub to: u32,
    /// What was (or would be) done, one line each.
    pub steps: Vec<String>,
    pub dry_run: bool,
}

#[cfg(unix)]
fn owned_by_me(root: &Path) -> Result<Option<(u32, u32)>, StoreError> {
    use std::os::unix::fs::MetadataExt;
    let owner = fs::metadata(root).map_err(io)?.uid();
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    Ok((owner != me).then_some((owner, me)))
}

#[cfg(windows)]
fn owned_by_me(_root: &Path) -> Result<Option<(u32, u32)>, StoreError> {
    // Ownership on Windows is an ACL question the service's account
    // answers by being able to open the directory at all.
    Ok(None)
}

/// Migrate a store to this build's schema. Refuses a store owned by another
/// account and a schema newer than this build; a dry run reports the steps
/// and writes nothing. The `Migrated` journal entry is the daemon's, on its
/// next start (`Store::open` finds `migrated.json`).
pub fn migrate(root: &Path, dry_run: bool, by: &str) -> Result<Migration, StoreError> {
    if !root.is_dir() {
        return Err(StoreError::Io(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    if let Some((owner, me)) = owned_by_me(root)? {
        return Err(StoreError::NotOwned {
            path: root.to_path_buf(),
            owner,
            me,
        });
    }
    let from = match schema_of(root) {
        Ok(v) => v,
        Err(SchemaError::Missing) => 0,
        Err(e) => return Err(StoreError::Schema(e)),
    };
    if from > SCHEMA {
        return Err(StoreError::Schema(SchemaError::Unknown(from)));
    }
    let mut steps = Vec::new();
    if from == SCHEMA {
        steps.push(format!("schema {SCHEMA} already; nothing to do"));
        return Ok(Migration {
            from,
            to: SCHEMA,
            steps,
            dry_run,
        });
    }
    // 0 -> 1: the store before schemas. Its instances/ and journal are
    // already in this shape; what it lacks is the schema file and the
    // record of the migration.
    steps.push(format!("write schema {SCHEMA}"));
    steps.push(format!(
        "record Migrated{{from: {from}, to: {SCHEMA}, by: {by}}} for the next start"
    ));
    if !dry_run {
        let lock = open_lock(root)?;
        if !lock_exclusive(&lock)? {
            return Err(StoreError::Locked(root.to_path_buf()));
        }
        fs::create_dir_all(root.join("instances")).map_err(io)?;
        let mut m = BTreeMap::new();
        m.insert("from".to_string(), from.to_string());
        m.insert("to".to_string(), SCHEMA.to_string());
        m.insert("by".to_string(), by.to_string());
        write_json(&root.join("migrated.json"), &m)?;
        write_atomic(&root.join("schema"), format!("{SCHEMA}\n").as_bytes())?;
    }
    Ok(Migration {
        from,
        to: SCHEMA,
        steps,
        dry_run,
    })
}
