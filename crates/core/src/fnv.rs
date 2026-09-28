//! FNV-1a 64: the shared integer hashing primitive.
//!
//! The same constant-free-by-dependency primitive the canonical state
//! hash (ADR-0002) and the stream seed derivation (ADR-0005) use:
//! deterministic, dependency-free, and identical across platforms.

/// The FNV offset basis.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hashes the bytes with FNV-1a 64.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a 64 test vectors from the reference specification.
    #[test]
    fn matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
