//! Keldysh core: the deterministic foundation of the step-simulation
//! framework.
//!
//! This crate hosts the engine-level primitives every simulation on
//! Keldysh shares: the named RNG streams of the `rng-streams-v1`
//! strategy (ADR-0005). The game-facing step contract — states,
//! commands, events, the phase order — is built on top of these
//! primitives by the following stories; nothing here knows about any
//! specific game.
//!
//! # Invariants
//!
//! - A step is a pure function of `(State, sorted Commands, Rules, RNG
//!   state)`: wall-clock time, thread scheduling, and network ordering
//!   never influence results.
//! - Randomness comes only through explicitly seeded, stream-partitioned
//!   generators ([`RngStreams`]); obtaining one without the party seed is
//!   impossible by types.
//! - No platform-dependent floating point in simulation state or logic;
//!   hashing and seed derivation are pure integer arithmetic (FNV-1a 64,
//!   shared with the canonical state hash).
//!
//! # Example
//!
//! ```
//! use keldysh_core::RngStreams;
//! use rand_chacha::rand_core::RngCore;
//!
//! let streams = RngStreams::new(0x1234_5678_9abc_def0);
//! let mut economy = streams.stream("economy");
//! let mut bytes = vec![0u8; 16];
//! economy.fill_bytes(&mut bytes);
//!
//! // Re-entrant: the same name always yields the stream from position
//! // zero, whatever was consumed before.
//! let mut again = streams.stream("economy");
//! again.fill_bytes(&mut bytes);
//! ```
//!
//! The strategy is pinned by [`RNG_STRATEGY`]; changing the derivation
//! layout or the name semantics changes the strategy tag.

mod fnv;
mod rng;

pub use fnv::fnv1a64;
pub use rng::{RNG_STRATEGY, RngStreams};
