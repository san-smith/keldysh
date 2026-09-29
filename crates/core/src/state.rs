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
}
