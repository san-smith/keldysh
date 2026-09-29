//! Canonical state serialization and the state hash (ADR-0002 §5.5).
//!
//! The canonical state hash detects drift between runs and platforms:
//! the same logical state must always produce the same digest. The
//! game owns its state's layout, so it also owns the canonical
//! encoding — through the [`CanonicalState`] trait — while the crate
//! provides the encoder helpers and the FNV-1a 64 digest, the shared
//! primitive of ADR-0003/0005. The hash is drift detection, not
//! cryptography.
//!
//! # The canonical encoding
//!
//! The helpers below define the one encoding every state must use;
//! hand-rolled variants break the cross-run agreement of the hashes:
//!
//! - integers are fixed-width little-endian (signed — two's
//!   complement) ([`put_u64`], [`put_i64`]);
//! - byte strings carry a u64 little-endian length prefix
//!   ([`put_len_prefixed`]);
//! - sequences carry a u64 little-endian element count
//!   ([`put_seq_len`]);
//! - collections without a stable iteration order (`HashMap`,
//!   `HashSet`) are written through their sorted keys, never in map
//!   order — the state discipline of ADR-0002 §5.7. The hash is the
//!   one boundary where an undisciplined state catches itself.
//!
//! State values live on the integer lattices of ADR-0004: no
//! platform-dependent floating point is ever serialized.
//!
//! # Example
//!
//! A state with a map-like collection: the canonical encoding sorts
//! the keys, so the insertion order never reaches the digest.
//!
//! ```
//! use keldysh_core::{CanonicalState, put_len_prefixed, put_seq_len, put_u64};
//! use std::collections::HashMap;
//!
//! struct Ledger {
//!     balances: HashMap<u64, u64>,
//!     memo: String,
//! }
//!
//! impl CanonicalState for Ledger {
//!     fn write_canonical(&self, out: &mut Vec<u8>) {
//!         let mut ids: Vec<u64> = self.balances.keys().copied().collect();
//!         ids.sort_unstable();
//!         put_seq_len(out, ids.len());
//!         for id in ids {
//!             put_u64(out, id);
//!             put_u64(out, self.balances[&id]);
//!         }
//!         put_len_prefixed(out, self.memo.as_bytes());
//!     }
//! }
//!
//! let mut first = Ledger { balances: HashMap::new(), memo: "day 1".into() };
//! first.balances.insert(1, 10);
//! first.balances.insert(2, 40);
//!
//! // The same logical state, filled in the opposite order.
//! let mut second = Ledger { balances: HashMap::new(), memo: "day 1".into() };
//! second.balances.insert(2, 40);
//! second.balances.insert(1, 10);
//!
//! assert_eq!(
//!     first.state_hash(),
//!     second.state_hash(),
//!     "the insertion order never reaches the digest"
//! );
//! ```

use std::fmt;

use crate::fnv::fnv1a64;

/// Writes `value` as fixed-width little-endian bytes.
pub fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes `value` as two's-complement little-endian bytes.
pub fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes `bytes` with a u64 little-endian length prefix.
pub fn put_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u64(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// Writes the element count of a sequence as u64 little-endian bytes.
pub fn put_seq_len(out: &mut Vec<u8>, count: usize) {
    put_u64(out, count as u64);
}

/// A state that can write its canonical serialization.
///
/// The game implements [`CanonicalState::write_canonical`]; the crate
/// derives the byte vector and the FNV-1a 64 digest. Every `Rules`
/// state carries this bound, so every Keldysh state is
/// checkpoint-hashable by construction (ADR-0002 §5.5).
pub trait CanonicalState {
    /// Writes the canonical bytes of the state to `out`.
    ///
    /// The encoding discipline is fixed by the module-level helpers:
    /// use them instead of hand-rolled variants. Values that differ in
    /// the simulation must differ in the bytes; values that do not
    /// (insertion order, spare capacity, padding) must not.
    fn write_canonical(&self, out: &mut Vec<u8>);

    /// The canonical byte serialization of the state.
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_canonical(&mut out);
        out
    }

    /// The canonical state hash: FNV-1a 64 over the canonical bytes.
    ///
    /// Drift detection between runs and platforms, not cryptography;
    /// the consumer computes it at the checkpoint steps, `step` does
    /// not (checkpoint serialization is sparse in long runs).
    fn state_hash(&self) -> u64 {
        fnv1a64(&self.canonical_bytes())
    }
}

/// Why canonical bytes could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalReadError {
    /// The bytes ended before `needed` more bytes at `offset`.
    UnexpectedEnd {
        /// The position the read started at.
        offset: usize,
        /// The number of bytes the read required.
        needed: usize,
    },
    /// A length field exceeded what the platform's `usize` can hold.
    LengthTooLarge {
        /// The position the length was read at.
        offset: usize,
        /// The rejected length.
        length: u64,
    },
}

impl fmt::Display for CanonicalReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalReadError::UnexpectedEnd { offset, needed } => write!(
                f,
                "the canonical bytes end at offset {offset}: {needed} more bytes required"
            ),
            CanonicalReadError::LengthTooLarge { offset, length } => write!(
                f,
                "length {length} at offset {offset} exceeds the platform's addressable size"
            ),
        }
    }
}

impl std::error::Error for CanonicalReadError {}

/// The reading side of the canonical encoding: a cursor over the
/// bytes the [`put_*`](put_u64) helpers wrote.
///
/// The methods mirror the writers one to one — the same discipline
/// serves the state hash and the snapshots. Errors carry the offset
/// the failed read started at.
pub struct CanonicalReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CanonicalReader<'a> {
    /// Reads the canonical bytes from their beginning.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// The position of the cursor: the number of bytes consumed.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Whether every byte has been consumed; the reader of a complete
    /// encoding ends empty.
    pub fn is_empty(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    /// Reads `n` raw bytes.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], CanonicalReadError> {
        let end = self
            .offset
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len());
        let end = match end {
            Some(end) => end,
            None => {
                return Err(CanonicalReadError::UnexpectedEnd {
                    offset: self.offset,
                    needed: n,
                });
            }
        };
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    /// Reads a fixed-width little-endian integer.
    pub fn read_u64(&mut self) -> Result<u64, CanonicalReadError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }

    /// Reads a two's-complement little-endian integer.
    pub fn read_i64(&mut self) -> Result<i64, CanonicalReadError> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }

    /// Reads a length-prefixed byte string: the u64 little-endian
    /// length, then the bytes.
    pub fn read_len_prefixed(&mut self) -> Result<&'a [u8], CanonicalReadError> {
        let offset = self.offset;
        let len = self.read_u64()?;
        let len = usize::try_from(len).map_err(|_| CanonicalReadError::LengthTooLarge {
            offset,
            length: len,
        })?;
        self.read_bytes(len)
    }

    /// Reads the element count of a sequence.
    pub fn read_seq_len(&mut self) -> Result<usize, CanonicalReadError> {
        let offset = self.offset;
        let len = self.read_u64()?;
        usize::try_from(len).map_err(|_| CanonicalReadError::LengthTooLarge {
            offset,
            length: len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A state that writes nothing: the empty serialization.
    struct Empty;

    impl CanonicalState for Empty {
        fn write_canonical(&self, _out: &mut Vec<u8>) {}
    }

    #[test]
    fn integers_are_little_endian() {
        let mut out = Vec::new();
        put_u64(&mut out, 0x0123_4567_89ab_cdef);
        assert_eq!(out, vec![0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]);

        out.clear();
        put_i64(&mut out, -1);
        assert_eq!(out, vec![0xff; 8], "two's complement");
    }

    #[test]
    fn byte_strings_and_sequences_are_length_prefixed() {
        let mut out = Vec::new();
        put_len_prefixed(&mut out, b"ab");
        assert_eq!(out, [2, 0, 0, 0, 0, 0, 0, 0, b'a', b'b']);

        out.clear();
        put_seq_len(&mut out, 3);
        assert_eq!(out, [3, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn empty_serialization_hashes_to_the_offset_basis() {
        assert_eq!(Empty.state_hash(), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn the_reader_mirrors_the_writers() {
        let mut out = Vec::new();
        put_u64(&mut out, 0x0123_4567_89ab_cdef);
        put_i64(&mut out, -2);
        put_len_prefixed(&mut out, b"payload");
        put_seq_len(&mut out, 42);

        let mut reader = CanonicalReader::new(&out);
        assert_eq!(reader.read_u64().unwrap(), 0x0123_4567_89ab_cdef);
        assert_eq!(reader.read_i64().unwrap(), -2);
        assert_eq!(reader.read_len_prefixed().unwrap(), b"payload");
        assert_eq!(reader.read_seq_len().unwrap(), 42);
        assert!(
            reader.is_empty(),
            "a complete encoding leaves no unread bytes"
        );
    }

    #[test]
    fn truncation_is_reported_at_the_offset() {
        let mut out = Vec::new();
        put_u64(&mut out, 1);
        put_len_prefixed(&mut out, b"abc");

        let mut reader = CanonicalReader::new(&out[..11]);
        assert_eq!(reader.read_u64().unwrap(), 1);
        // The length field itself cannot be read: 3 bytes remain, 8
        // are required.
        assert_eq!(
            reader.read_len_prefixed().unwrap_err(),
            CanonicalReadError::UnexpectedEnd {
                offset: 8,
                needed: 8
            }
        );
        assert_eq!(
            CanonicalReadError::UnexpectedEnd {
                offset: 8,
                needed: 8
            }
            .to_string(),
            "the canonical bytes end at offset 8: 8 more bytes required"
        );
    }
}
