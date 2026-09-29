//! Replay tests: the recorded party verifies on a fresh state and
//! reproduces state and events; a mutated command, a foreign version,
//! and a foreign starting state are caught — with the first
//! divergence and both hashes in the report.

use std::convert::Infallible;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use keldysh_core::{
    CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
    RulesVersion, put_u64, step,
};
use keldysh_replay::{Checkpoints, Recording, ReplayMismatch, ReplayRecord, verify};

const PARTY_SEED: u64 = 0x05EE_D1A7;

/// A two-phase counter: "apply" adds each command with a
/// stream-drawn bonus, "settle" records the closing value — the RNG
/// the replay must re-derive from the party seed.
struct CounterState {
    value: u64,
}

impl CanonicalState for CounterState {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.value);
    }
}

impl CounterState {
    fn initial() -> Self {
        Self { value: 1 }
    }
}

struct CounterRules;

static PHASES: &[Phase] = &[Phase { name: "apply" }, Phase { name: "settle" }];

impl CounterRules {
    fn apply_phase_logic(&self, state: &mut CounterState, commands: &[&u64], rng: &mut ChaCha8Rng) {
        for amount in commands {
            let bonus = rng.next_u64() % 5;
            state.value += *amount + bonus;
        }
    }
}

impl Rules for CounterRules {
    type State = CounterState;
    type Command = u64;
    type Event = u64;
    type Error = Infallible;

    fn phases(&self) -> &'static [Phase] {
        PHASES
    }

    fn version(&self) -> RulesVersion {
        RulesVersion::declaring(1, PHASES)
    }

    fn apply_phase(
        &self,
        phase: &'static Phase,
        state: &mut CounterState,
        commands: &[&u64],
        rng: &mut ChaCha8Rng,
        events: &mut Vec<u64>,
    ) -> Result<(), Infallible> {
        match phase.name {
            "apply" => self.apply_phase_logic(state, commands, rng),
            "settle" => events.push(state.value),
            other => unreachable!("the engine executes only declared phases: {other:?}"),
        }
        Ok(())
    }
}

/// The same counter with a bumped revision: the phase fingerprint is
/// unchanged, the version is not.
struct BumpedRules;

impl Rules for BumpedRules {
    type State = CounterState;
    type Command = u64;
    type Event = u64;
    type Error = Infallible;

    fn phases(&self) -> &'static [Phase] {
        PHASES
    }

    fn version(&self) -> RulesVersion {
        RulesVersion::declaring(2, PHASES)
    }

    fn apply_phase(
        &self,
        _phase: &'static Phase,
        _state: &mut CounterState,
        _commands: &[&u64],
        _rng: &mut ChaCha8Rng,
        _events: &mut Vec<u64>,
    ) -> Result<(), Infallible> {
        unreachable!("verification rejects the version before any step executes")
    }
}

/// The same counter with a foreign phase sequence: the fingerprint
/// differs, so does the version.
struct ForeignPhases;

static FOREIGN_PHASES: &[Phase] = &[Phase { name: "apply" }, Phase { name: "audit" }];

impl Rules for ForeignPhases {
    type State = CounterState;
    type Command = u64;
    type Event = u64;
    type Error = Infallible;

    fn phases(&self) -> &'static [Phase] {
        FOREIGN_PHASES
    }

    fn version(&self) -> RulesVersion {
        RulesVersion::declaring(1, FOREIGN_PHASES)
    }

    fn apply_phase(
        &self,
        _phase: &'static Phase,
        _state: &mut CounterState,
        _commands: &[&u64],
        _rng: &mut ChaCha8Rng,
        _events: &mut Vec<u64>,
    ) -> Result<(), Infallible> {
        unreachable!("verification rejects the version before any step executes")
    }
}

/// Plays `steps` steps of one command per day, feeding the recorder
/// the way a session loop does. The session applies `applied_at` and
/// records `recorded_at`: the honest sessions pass the same closure
/// twice — the split models a tampered record, the only way a
/// well-formed verifier input diverges (the recorder is passive).
fn play(
    steps: u64,
    checkpoints: Checkpoints,
    applied_at: impl Fn(u64) -> u64,
    recorded_at: impl Fn(u64) -> u64,
) -> ReplayRecord<u64> {
    let rules = CounterRules;
    let mut recording = Recording::start(&rules, PARTY_SEED, &CounterState::initial())
        .with_checkpoints(checkpoints);
    let mut state = CounterState::initial();
    let streams = RngStreams::new(PARTY_SEED);
    for step_index in 0..steps {
        let applied = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: applied_at(step_index),
        }];
        let recorded = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: recorded_at(step_index),
        }];
        recording.record_commands(step_index, &recorded);
        let outcome = step(&mut state, step_index, &applied, &rules, &streams).unwrap();
        drop(outcome);
        recording.record_checkpoint(step_index, &state);
    }
    recording.finish()
}

fn default_amount(step: u64) -> u64 {
    (step % 5) + 1
}

#[test]
fn a_recorded_party_verifies_on_a_fresh_state() {
    let first = play(100, Checkpoints::every(1), default_amount, default_amount);
    let second = play(100, Checkpoints::every(1), default_amount, default_amount);

    // The recording is deterministic; the journal and the checkpoints
    // agree between the runs.
    assert_eq!(first, second, "two recordings of one party are identical");
    assert_eq!(first.step_count(), 100);
    assert_eq!(first.checkpoint_hashes().len(), 100);
    assert_eq!(
        first.checkpoint_hashes().last().copied().unwrap().1,
        second.checkpoint_hashes().last().copied().unwrap().1,
        "the closing checkpoint is the final state's hash"
    );

    verify(&first, CounterState::initial(), &CounterRules).unwrap();
}

#[test]
fn a_mutated_command_diverges_at_the_mutated_step() {
    let reference = play(100, Checkpoints::every(1), default_amount, default_amount);
    let tampered = play(100, Checkpoints::every(1), default_amount, |step| {
        if step == 30 { 99 } else { default_amount(step) }
    });

    let expected = reference
        .checkpoint_hashes()
        .iter()
        .find(|(step, _)| *step == 30)
        .unwrap()
        .1;

    let mismatch = verify(&tampered, CounterState::initial(), &CounterRules).unwrap_err();
    let ReplayMismatch::StepHash {
        step,
        expected: recorded,
        actual,
    } = mismatch
    else {
        panic!("expected a step-hash divergence, got a different mismatch");
    };
    assert_eq!(step, 30, "with every(1) the divergent step is the report");
    assert_eq!(recorded, expected, "the record holds the honest hash");
    assert_ne!(
        actual, recorded,
        "the tampered journal replayed differently"
    );
    assert_eq!(
        ReplayMismatch::StepHash {
            step,
            expected: recorded,
            actual
        }
        .to_string(),
        format!(
            "first divergence at checkpoint step 30: recorded {recorded:#018x}, \
             replayed {actual:#018x}"
        )
    );
}

#[test]
fn sparse_checkpoints_localize_the_drift() {
    let base = play(100, Checkpoints::every(7), default_amount, default_amount);
    // Steps 0, 7, …, 98 carry checkpoints.
    assert_eq!(base.checkpoint_hashes().len(), 15);
    assert_eq!(
        base.checkpoints().interval(),
        7,
        "the policy is part of the record"
    );

    let tampered = play(100, Checkpoints::every(7), default_amount, |step| {
        if step == 30 { 99 } else { default_amount(step) }
    });

    // Checkpoints 0..28 agree; the drift at step 30 surfaces at the
    // first checkpoint after it.
    let expected = base
        .checkpoint_hashes()
        .iter()
        .find(|(step, _)| *step == 35)
        .unwrap()
        .1;

    let mismatch = verify(&tampered, CounterState::initial(), &CounterRules).unwrap_err();
    let ReplayMismatch::StepHash {
        step,
        expected: recorded,
        actual,
    } = mismatch
    else {
        panic!("expected a step-hash divergence, got a different mismatch");
    };
    assert_eq!(
        step, 35,
        "the drift surfaces at the first checkpoint after it"
    );
    assert_eq!(recorded, expected, "the record holds the honest hash");
    assert_ne!(
        actual, recorded,
        "the tampered journal replayed differently"
    );
}

#[test]
fn foreign_versions_are_rejected_before_any_step_executes() {
    let record = play(100, Checkpoints::every(1), default_amount, default_amount);

    // A bumped revision: same phases, different version.
    assert_eq!(
        verify(&record, CounterState::initial(), &BumpedRules),
        Err(ReplayMismatch::Version {
            recorded: CounterRules.version(),
            actual: BumpedRules.version(),
        })
    );

    // A foreign phase sequence: the fingerprint changes the version.
    assert!(matches!(
        verify(&record, CounterState::initial(), &ForeignPhases),
        Err(ReplayMismatch::Version { .. })
    ));

    // The version check precedes the initial-state check.
    assert!(matches!(
        verify(&record, CounterState { value: 99 }, &BumpedRules),
        Err(ReplayMismatch::Version { .. })
    ));
}

#[test]
fn a_foreign_starting_state_is_rejected_before_any_step_executes() {
    let record = play(100, Checkpoints::every(1), default_amount, default_amount);
    assert_eq!(
        verify(&record, CounterState { value: 99 }, &CounterRules),
        Err(ReplayMismatch::InitialState {
            expected: record.initial_hash(),
            actual: CounterState { value: 99 }.state_hash(),
        })
    );
}

#[test]
fn a_corrupt_record_reports_the_step_failure() {
    // A session bug recorded envelopes whose target step does not
    // match their journal step: the replay reports the failure at the
    // step instead of panicking.
    let rules = CounterRules;
    let mut recording = Recording::start(&rules, PARTY_SEED, &CounterState::initial());
    let wrong = [CommandEnvelope {
        player: PlayerId(1),
        target_step: 3,
        idempotency: IdempotencyId(1),
        payload: 5,
    }];
    recording.record_commands(7, &wrong);
    let record = recording.finish();

    assert_eq!(
        verify(&record, CounterState::initial(), &rules),
        Err(ReplayMismatch::StepFailed {
            step: 7,
            reason: keldysh_core::StepError::<Infallible>::CommandOrder(
                keldysh_core::CommandOrderError::WrongTargetStep {
                    index: 0,
                    expected: 7,
                    actual: 3,
                }
            )
            .to_string(),
        })
    );
}

#[test]
fn an_empty_party_verifies() {
    let record = play(0, Checkpoints::every(1), |_| 0, |_| 0);
    assert_eq!(record.step_count(), 0);
    assert!(record.checkpoint_hashes().is_empty());
    verify(&record, CounterState::initial(), &CounterRules).unwrap();
}
