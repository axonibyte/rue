//! The canonical byte encoding for hashing: length-prefixed, field-ordered,
//! domain-separated (docs/ROADMAP.md section 5.10). The journal chain and the
//! request digest hash these bytes, so two values encode alike only when they
//! are the same value: every value carries a tag byte, so `None`, the empty
//! string and the empty list can never collide, and a record's fields are
//! written in declared order without names.
//!
//! ```text
//! 0x00                          none
//! 0x01 <value>                  some
//! 0x02 0x00|0x01                bool
//! 0x03 u64 big-endian           u64 (seconds, sequence numbers, step numbers)
//! 0x05 u64be(len) bytes         octets (UTF-8 text and raw bytes alike)
//! 0x06 u64be(count) elements    list
//! 0x07 u64be(count) fields      record, fields in declared order
//! enum    := octets(variant name) record(payload)    -- a unit variant has an empty record
//! message := octets(domain) record(fields)
//! ```

/// Something with a canonical encoding.
pub trait Canon {
    fn canon(&self, e: &mut Encoder);
}

/// Appends canonical bytes.
#[derive(Debug, Default, Clone)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Encoder {
        Encoder::default()
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    pub fn none(&mut self) {
        self.buf.push(0x00);
    }

    pub fn some<T: Canon + ?Sized>(&mut self, v: &T) {
        self.buf.push(0x01);
        v.canon(self);
    }

    pub fn option<T: Canon>(&mut self, v: &Option<T>) {
        match v {
            None => self.none(),
            Some(x) => self.some(x),
        }
    }

    pub fn bool(&mut self, b: bool) {
        self.buf.push(0x02);
        self.buf.push(u8::from(b));
    }

    pub fn u64(&mut self, n: u64) {
        self.buf.push(0x03);
        self.buf.extend_from_slice(&n.to_be_bytes());
    }

    pub fn octets(&mut self, bytes: &[u8]) {
        self.buf.push(0x05);
        self.buf
            .extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        self.buf.extend_from_slice(bytes);
    }

    pub fn str(&mut self, s: &str) {
        self.octets(s.as_bytes());
    }

    pub fn list<T: Canon>(&mut self, items: &[T]) {
        self.buf.push(0x06);
        self.buf
            .extend_from_slice(&(items.len() as u64).to_be_bytes());
        for i in items {
            i.canon(self);
        }
    }

    /// A record of `count` fields; the caller then encodes each field in
    /// declared order.
    pub fn record(&mut self, count: u64) {
        self.buf.push(0x07);
        self.buf.extend_from_slice(&count.to_be_bytes());
    }

    /// An enum: its variant name, then its payload as a record.
    pub fn variant(&mut self, name: &str, fields: u64) {
        self.str(name);
        self.record(fields);
    }
}

impl Canon for str {
    fn canon(&self, e: &mut Encoder) {
        e.str(self);
    }
}

impl Canon for String {
    fn canon(&self, e: &mut Encoder) {
        e.str(self);
    }
}

impl Canon for u64 {
    fn canon(&self, e: &mut Encoder) {
        e.u64(*self);
    }
}

impl Canon for u32 {
    fn canon(&self, e: &mut Encoder) {
        e.u64(u64::from(*self));
    }
}

impl Canon for bool {
    fn canon(&self, e: &mut Encoder) {
        e.bool(*self);
    }
}

impl<T: Canon> Canon for Vec<T> {
    fn canon(&self, e: &mut Encoder) {
        e.list(self);
    }
}

impl<T: Canon> Canon for Option<T> {
    fn canon(&self, e: &mut Encoder) {
        e.option(self);
    }
}

/// A domain-separated message: the domain string, then the value.
pub fn message<T: Canon + ?Sized>(domain: &str, v: &T) -> Vec<u8> {
    let mut e = Encoder::new();
    e.str(domain);
    v.canon(&mut e);
    e.finish()
}
