//! Journal delivery, docs/ROADMAP.md 7.6: the engine chains (and signs)
//! each entry, keeps its own copy in the store, and delivers it to every
//! declared sink synchronously. All must acknowledge. A sink that refuses is
//! R0304: the plan refuses to proceed, and the refusal is itself journaled
//! to whichever sinks still acknowledge. The write-ahead entry for a step is
//! recorded (and so acknowledged) before its `do` runs; that ordering is
//! the lifecycle's, and a rediscovery row holds it.

use std::fmt;
use std::sync::{Arc, Mutex};

use rue_core::journal::{append, Entry, Event};
use rue_core::model::Instant;

use crate::sign::Signer;
use crate::store::{Store, StoreError};

/// A declared journal sink. `deliver` returns `Err` when the sink did not
/// acknowledge, with the reason it gave.
pub trait Sink: Send {
    fn name(&self) -> String;
    fn deliver(&mut self, e: &Entry) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    Store(StoreError),
    /// R0304: these sinks did not acknowledge the entry.
    SinkRefused {
        entry: Box<Entry>,
        refusals: Vec<(String, String)>,
    },
    Sign(String),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::Store(e) => write!(f, "journal store: {e}"),
            JournalError::SinkRefused { entry, refusals } => {
                write!(f, "R0304: entry {} not acknowledged by", entry.seq)?;
                for (name, why) in refusals {
                    write!(f, " {name} ({why})")?;
                }
                Ok(())
            }
            JournalError::Sign(s) => write!(f, "journal signing: {s}"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<StoreError> for JournalError {
    fn from(e: StoreError) -> JournalError {
        JournalError::Store(e)
    }
}

/// The identity of what an entry is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct About {
    pub plan: String,
    pub instance: String,
    pub host: String,
}

pub struct Journal {
    tail: Option<Entry>,
    sinks: Vec<Box<dyn Sink>>,
    signer: Option<Signer>,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Journal(tail seq {:?}, {} sinks, signed: {})",
            self.tail.as_ref().map(|e| e.seq),
            self.sinks.len(),
            self.signer.is_some()
        )
    }
}

impl Journal {
    /// Open over a store's chain: the tail is the last entry the store holds.
    pub fn open(
        store: &Store,
        sinks: Vec<Box<dyn Sink>>,
        signer: Option<Signer>,
    ) -> Result<Journal, JournalError> {
        Ok(Journal {
            tail: store.journal_tail()?,
            sinks,
            signer,
        })
    }

    pub fn tail(&self) -> Option<&Entry> {
        self.tail.as_ref()
    }

    pub fn signed(&self) -> bool {
        self.signer.is_some()
    }

    pub fn sink_names(&self) -> Vec<String> {
        self.sinks.iter().map(|s| s.name()).collect()
    }

    fn next(
        &self,
        at: Instant,
        about: &About,
        event: Event,
        secret_labels: Vec<String>,
    ) -> Result<Entry, JournalError> {
        let chain: Vec<Entry> = self.tail.iter().cloned().collect();
        let mut e = append(
            &chain,
            at,
            &about.plan,
            &about.instance,
            &about.host,
            event,
            secret_labels,
        );
        if let Some(s) = &self.signer {
            e.sig = Some(s.sign(&e).map_err(JournalError::Sign)?);
        }
        Ok(e)
    }

    /// Chain, sign, keep, deliver. The store's copy is written first (it is
    /// the engine's own record); then every sink, all of which must
    /// acknowledge. On a refusal the `Refused` entry that names the sinks
    /// is chained after it and delivered to the sinks that still
    /// acknowledge, and the error carries the refusals.
    pub fn record(
        &mut self,
        store: &Store,
        at: Instant,
        about: &About,
        event: Event,
        secret_labels: Vec<String>,
    ) -> Result<Entry, JournalError> {
        let e = self.next(at, about, event, secret_labels)?;
        store.append_journal(&e)?;
        self.tail = Some(e.clone());
        let refusals = self.deliver_all(&e);
        if refusals.is_empty() {
            return Ok(e);
        }
        let reason = refusals
            .iter()
            .map(|(n, why)| format!("{n}: {why}"))
            .collect::<Vec<_>>()
            .join("; ");
        let refused = self.next(
            at,
            about,
            Event::Refused {
                reason: format!("R0304: sink did not acknowledge: {reason}"),
            },
            Vec::new(),
        )?;
        store.append_journal(&refused)?;
        self.tail = Some(refused.clone());
        let _ = self.deliver_all(&refused);
        Err(JournalError::SinkRefused {
            entry: Box::new(e),
            refusals,
        })
    }

    fn deliver_all(&mut self, e: &Entry) -> Vec<(String, String)> {
        let mut refusals = Vec::new();
        for s in &mut self.sinks {
            if let Err(why) = s.deliver(e) {
                refusals.push((s.name(), why));
            }
        }
        refusals
    }
}

// ---------------------------------------------------------------------------
// Sinks the engine itself provides: memory, for tests and the simulation.

/// A sink that keeps every entry in memory and can be told to refuse.
#[derive(Debug, Clone, Default)]
pub struct MemorySink {
    pub name: String,
    pub entries: Arc<Mutex<Vec<Entry>>>,
    /// When set, every delivery is refused with this reason.
    pub refuse: Arc<Mutex<Option<String>>>,
}

impl MemorySink {
    pub fn new(name: &str) -> MemorySink {
        MemorySink {
            name: name.to_string(),
            ..MemorySink::default()
        }
    }
    pub fn entries(&self) -> Vec<Entry> {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn events(&self) -> Vec<Event> {
        self.entries().into_iter().map(|e| e.event).collect()
    }
    pub fn refuse_with(&self, reason: Option<&str>) {
        *self.refuse.lock().unwrap_or_else(|e| e.into_inner()) = reason.map(str::to_string);
    }
}

impl Sink for MemorySink {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        if let Some(r) = self
            .refuse
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(r);
        }
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(e.clone());
        Ok(())
    }
}
