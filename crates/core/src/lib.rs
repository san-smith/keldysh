//! Keldysh core: the deterministic foundation of the step-simulation
//! framework.
//!
//! This crate hosts the deterministic step machinery every simulation
//! on Keldysh shares: the named RNG streams of the `rng-streams-v1`
//! strategy (ADR-0005) and the step contract — the [`Rules`] trait with
//! its declared [`Phase`] sequence and the [`step`] engine executing
//! phases in the declared order. Nothing here knows about any specific
//! game.
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
//! - Every state is canonically serializable ([`CanonicalState`]): the
//!   canonical hash ([`CanonicalState::state_hash`]) detects drift
//!   between runs at the checkpoint steps the consumer chooses.
//! - Events are a projection, never an input: the engine returns them
//!   per step and never reads the journal ([`EventJournal`]) — the state
//!   stays the source of truth.
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
mod journal;
mod orders;
mod rng;
mod state;
mod step;

pub use fnv::fnv1a64;
pub use journal::{EventJournal, JournalError, StepEvents};
pub use orders::{CommandEnvelope, CommandOrderError, IdempotencyId, PlayerId, canonical_order};
pub use rng::{RNG_STRATEGY, RngStreams};
pub use state::{CanonicalState, put_i64, put_len_prefixed, put_seq_len, put_u64};
pub use step::{Phase, Rules, RulesVersion, StepError, StepOutcome, step};
