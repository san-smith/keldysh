//! Replay verification: the recorded party against a fresh state.
//!
//! The verifier replays the command journal through
//! [`step`](keldysh_core::step) with the streams derived from the
//! party seed and compares the state hashes at the checkpoint steps.
//! A foreign rules version and a foreign starting state are rejected
//! before the first step executes (ADR-0002 §5.6); the first
//! divergent checkpoint aborts the replay with both hashes
//! (ADR-0002 §5.5).

use std::fmt;

use keldysh_core::{CanonicalState, RngStreams, Rules, RulesVersion, step};

use crate::record::ReplayRecord;

/// Why a replay did not reproduce the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayMismatch {
    /// The rules differ from the recorded ones — rejected before any
    /// step executes.
    Version {
        /// The version captured at the recording start.
        recorded: RulesVersion,
        /// The version of the rules under verification.
        actual: RulesVersion,
    },
    /// The given starting state is not the recorded one — rejected
    /// before any step executes.
    InitialState {
        /// The starting hash pinned by the record.
        expected: u64,
        /// The hash of the state given to verification.
        actual: u64,
    },
    /// The first divergent checkpoint: the state hash the record
    /// holds and the hash the replay produced.
    StepHash {
        /// The checkpoint step where the hashes diverged.
        step: u64,
        /// The hash recorded by the session.
        expected: u64,
        /// The hash produced by the replay.
        actual: u64,
    },
    /// A step failed during replay: the record is corrupt — a
    /// well-formed record of a party under these rules cannot fail a
    /// step the original session executed.
    StepFailed {
        /// The failing step.
        step: u64,
        /// The engine's diagnostic for the failure.
        reason: String,
    },
}

impl fmt::Display for ReplayMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayMismatch::Version { recorded, actual } => write!(
                f,
                "replay rejected: rules version mismatch — recorded {recorded}, \
                 given {actual}"
            ),
            ReplayMismatch::InitialState { expected, actual } => write!(
                f,
                "replay rejected: initial state hash mismatch — recorded \
                 {expected:#018x}, given {actual:#018x}"
            ),
            ReplayMismatch::StepHash {
                step,
                expected,
                actual,
            } => write!(
                f,
                "first divergence at checkpoint step {step}: recorded \
                 {expected:#018x}, replayed {actual:#018x}"
            ),
            ReplayMismatch::StepFailed { step, reason } => {
                write!(f, "step {step} failed during replay: {reason}")
            }
        }
    }
}

impl std::error::Error for ReplayMismatch {}

/// Verifies the record against a fresh starting state: replays the
/// command journal through [`step`](keldysh_core::step) and compares
/// the state hashes at the checkpoint steps. The state is consumed —
/// verification is an offline procedure; the journal events are
/// produced by the replay and compared by the caller when needed.
/// The record is keyed by the command type, so verifying against
/// different rules type-checks and fails at the version check.
pub fn verify<R: Rules>(
    record: &ReplayRecord<R::Command>,
    initial: R::State,
    rules: &R,
) -> Result<(), ReplayMismatch> {
    let actual_version = rules.version();
    if actual_version != record.version() {
        return Err(ReplayMismatch::Version {
            recorded: record.version(),
            actual: actual_version,
        });
    }

    let actual_initial = initial.state_hash();
    if actual_initial != record.initial_hash() {
        return Err(ReplayMismatch::InitialState {
            expected: record.initial_hash(),
            actual: actual_initial,
        });
    }

    let streams = RngStreams::new(record.party_seed());
    let mut state = initial;
    let mut checkpoints = record.checkpoint_hashes().iter();
    for commands in record.steps() {
        let step_index = commands.step();
        step(
            &mut state,
            step_index,
            commands.envelopes(),
            rules,
            &streams,
        )
        .map_err(|error| ReplayMismatch::StepFailed {
            step: step_index,
            reason: error.to_string(),
        })?;

        if record.checkpoints().is_checkpoint(step_index) {
            let expected = match checkpoints.next() {
                Some((recorded_step, hash)) if *recorded_step == step_index => *hash,
                _ => {
                    return Err(ReplayMismatch::StepFailed {
                        step: step_index,
                        reason: String::from(
                            "the recorded checkpoints do not match the record's policy",
                        ),
                    });
                }
            };
            let actual = state.state_hash();
            if actual != expected {
                return Err(ReplayMismatch::StepHash {
                    step: step_index,
                    expected,
                    actual,
                });
            }
        }
    }
    Ok(())
}
