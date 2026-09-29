//! This crate owns its share of the Keldysh contract — replay recording,
//! verification, and snapshots. The invariants, the version model, and what
//! breaks a replay are documented in the [`keldysh_core`] crate documentation.
//!
//! Replay recording and verification for the Keldysh framework.
//!
//! A replay record is everything a party's verification needs apart
//! from the starting state itself: the rules version, the party seed,
//! the starting state's canonical hash, the command journal by step,
//! and the checkpoint hashes under a policy. The starting state
//! travels outside the record — the caller supplies it; its
//! persistence is the snapshot format (E-03 S-02), a transportable
//! replay file is the game's concern (E-08).
//!
//! # Invariants
//!
//! - The recorder ([`Recording`]) is passive: the session loop runs
//!   [`step`](keldysh_core::step) and feeds the recorder — no second
//!   execution path exists, so a recording cannot diverge from the
//!   party it observes.
//! - RNG positions are never recorded; the replay re-derives the
//!   streams from the party seed (ADR-0002 §5.3).
//! - A foreign rules version and a foreign starting state are
//!   rejected before the first step; the first divergent checkpoint
//!   aborts the replay with both hashes (ADR-0002 §5.5–5.6).
//! - The snapshot ([`snapshot`]) is the state's container, not the
//!   session's: the header's rules version and state hash reject
//!   foreign rules and corrupted payloads; the party seed stays with
//!   the game's save format.
//!
//! # Example
//!
//! A three-step party recorded by the session loop, then verified on
//! a fresh starting state.
//!
//! ```
//! use std::convert::Infallible;
//!
//! use keldysh_core::{
//!     CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
//!     RulesVersion, put_u64, step,
//! };
//! use keldysh_replay::{Recording, verify};
//!
//! struct Counter {
//!     value: u64,
//! }
//!
//! impl CanonicalState for Counter {
//!     fn write_canonical(&self, out: &mut Vec<u8>) {
//!         put_u64(out, self.value);
//!     }
//! }
//!
//! struct CounterRules;
//!
//! static PHASES: &[Phase] = &[Phase { name: "apply" }];
//!
//! impl Rules for CounterRules {
//!     type State = Counter;
//!     type Command = u64;
//!     type Event = Infallible;
//!     type Error = Infallible;
//!
//!     fn phases(&self) -> &'static [Phase] {
//!         PHASES
//!     }
//!
//!     fn version(&self) -> RulesVersion {
//!         RulesVersion::declaring(1, PHASES)
//!     }
//!
//!     fn apply_phase(
//!         &self,
//!         _phase: &'static Phase,
//!         state: &mut Counter,
//!         commands: &[&u64],
//!         _rng: &mut rand_chacha::ChaCha8Rng,
//!         _events: &mut Vec<Infallible>,
//!     ) -> Result<(), Infallible> {
//!         for amount in commands {
//!             state.value += **amount;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let rules = CounterRules;
//! let streams = RngStreams::new(0x5EED);
//! let mut recording = Recording::start(&rules, 0x5EED, &Counter { value: 1 });
//! let mut state = Counter { value: 1 };
//! for step_index in 0..3_u64 {
//!     let envelopes = [CommandEnvelope {
//!         player: PlayerId(1),
//!         target_step: step_index,
//!         idempotency: IdempotencyId(step_index + 1),
//!         payload: 10,
//!     }];
//!     recording.record_commands(step_index, &envelopes);
//!     step(&mut state, step_index, &envelopes, &rules, &streams).unwrap();
//!     recording.record_checkpoint(step_index, &state);
//! }
//! let record = recording.finish();
//!
//! // The same party, replayed from a fresh starting state, verifies.
//! assert!(verify(&record, Counter { value: 1 }, &rules).is_ok());
//! ```

mod record;
pub mod snapshot;
mod verify;

pub use record::{Checkpoints, Recording, ReplayRecord};
pub use verify::{ReplayMismatch, verify};
