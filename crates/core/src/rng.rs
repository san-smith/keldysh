//! Named RNG streams: the `rng-streams-v1` strategy (ADR-0005).
//!
//! Randomness in a simulation is partitioned into named streams: every
//! concern (a stage, a system, a subsystem) owns a stream addressed by
//! the pair `(party seed, stream name)`. The strategy is a text
//! contract shared with the geography generator — both projects
//! implement it identically, and the golden vectors pin the mirror.
//!
//! Properties (from ADR-0005):
//!
//! - **Locality**: introducing a new stream name never changes the
//!   sequences of existing names.
//! - **Seed sensitivity**: the party seed shifts every stream.
//! - **Prefix sensitivity**: `"combat"` and `"combat.round"` diverge.
//! - **Re-entrancy**: a stream is always derived at position zero; the
//!   owner consumes it in its own order.
//!
//! The party seed plays the role the world seed plays in the
//! generator; the derivation and the stream construction are
//! identical.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::fnv::fnv1a64;

/// The strategy identifier written into derivations and pinned by
/// tests: changing the layout or the name semantics changes the tag.
pub const RNG_STRATEGY: &str = "rng-streams-v1";

const STRATEGY_TAG: u8 = 1;

/// Factory of named ChaCha8 streams for one party seed.
///
/// The factory is valueless apart from the party seed: constructing it,
/// cloning it, or re-creating it has no effect on stream contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngStreams {
    party_seed: u64,
}

impl RngStreams {
    /// Creates the stream factory for a party seed.
    pub fn new(party_seed: u64) -> Self {
        Self { party_seed }
    }

    /// The party seed this factory derives streams from.
    pub fn party_seed(&self) -> u64 {
        self.party_seed
    }

    /// Derives the stream for `name` at position zero.
    ///
    /// Pure and re-entrant: the result depends only on `(party_seed,
    /// name)`. The caller must treat the returned generator as owned by
    /// one system and consume it in a system-determined order.
    pub fn stream(&self, name: &str) -> ChaCha8Rng {
        let mut input = Vec::with_capacity(name.len() + 9);
        input.push(STRATEGY_TAG);
        input.extend_from_slice(name.as_bytes());
        input.extend_from_slice(&self.party_seed.to_le_bytes());
        ChaCha8Rng::seed_from_u64(fnv1a64(&input))
    }

    /// Fills `out` with the leading bytes of the named stream.
    ///
    /// Convenience for systems that only need a few deterministic
    /// bytes; equivalent to consuming [`RngStreams::stream`] from
    /// position zero.
    pub fn stream_bytes(&self, name: &str, out: &mut [u8]) {
        self.stream(name).fill_bytes(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take_bytes(rng: &mut ChaCha8Rng, n: usize) -> Vec<u8> {
        let mut out = vec![0u8; n];
        rng.fill_bytes(&mut out);
        out
    }

    #[test]
    fn same_seed_and_name_reproduce_bytes_exactly() {
        let mut first = RngStreams::new(0x1234).stream("combat");
        let mut second = RngStreams::new(0x1234).stream("combat");
        assert_eq!(take_bytes(&mut first, 256), take_bytes(&mut second, 256));
    }

    #[test]
    fn different_names_diverge() {
        let streams = RngStreams::new(0x1234);
        let combat = take_bytes(&mut streams.stream("combat"), 64);
        let research = take_bytes(&mut streams.stream("research"), 64);
        assert_ne!(combat, research);
    }

    #[test]
    fn party_seed_shifts_every_stream() {
        for name in ["combat", "economy", "research"] {
            let before = take_bytes(&mut RngStreams::new(1).stream(name), 64);
            let after = take_bytes(&mut RngStreams::new(2).stream(name), 64);
            assert_ne!(before, after, "seed must shift stream {name}");
        }
    }

    #[test]
    fn locality_introducing_a_name_leaves_others_untouched() {
        let streams = RngStreams::new(7);
        let names = ["combat", "economy"];
        let mut explicit = Vec::new();
        for name in names {
            explicit.push(take_bytes(&mut streams.stream(name), 64));
        }

        // A later system version adds a new stream name.
        let _new = streams.stream("diplomacy");
        let mut after = Vec::new();
        for name in names {
            after.push(take_bytes(&mut streams.stream(name), 64));
        }
        assert_eq!(explicit, after, "existing streams must not shift");
    }

    #[test]
    fn names_are_prefix_sensitive() {
        let streams = RngStreams::new(9);
        let short = take_bytes(&mut streams.stream("combat"), 32);
        let nested = take_bytes(&mut streams.stream("combat.round"), 32);
        assert_ne!(short, nested);
    }

    #[test]
    fn strategy_is_pinned() {
        assert_eq!(RNG_STRATEGY, "rng-streams-v1");
    }

    /// Cross-implementation golden vectors: the byte sequences of the
    /// reference implementation (Vernadsky `vernadsky-core`, the same
    /// `rng-streams-v1` contract) for the same inputs. The mirror must
    /// agree byte-for-byte; a drift here means one of the two
    /// implementations (or the shared dependency) changed behavior.
    #[test]
    fn matches_the_reference_implementation() {
        let seed = 0x1234_5678_9abc_def0;
        let vectors: [(&str, &str); 4] = [
            (
                "economy",
                "0b56fb0234c164b1c5ea13f0d67d49f4e47f240a038d0a3447d88f1d9c238b01",
            ),
            (
                "combat",
                "13dbbfe12130b400169debf23598eac705c31b85a84e6e83f45de5742a4849e7",
            ),
            (
                "combat.round",
                "45dbfc9f8685983409eef0549b8253d5e0d4bcbbcf6f503221bd5c650b27a1d6",
            ),
            // A different party seed must shift every stream.
            (
                "economy@seed7",
                "60be77f25b52d1117504ad2a34cfa38e2ebfc7e7c85e9031174987f4fb62e332",
            ),
        ];
        for (name, expected) in vectors {
            let seed = if name.ends_with("@seed7") { 7 } else { seed };
            let name = name.strip_suffix("@seed7").unwrap_or(name);
            assert_eq!(
                take_bytes(&mut RngStreams::new(seed).stream(name), 32),
                hex(expected),
                "stream {name:?} of seed {seed:#x}"
            );
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
