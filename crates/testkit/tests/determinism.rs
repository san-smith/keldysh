//! Determinism-harness tests: the green path of both helpers, the
//! sanitizers that catch deliberate nondeterminism — per-run inside
//! the process, cross-process between them — and the E-02 exit demo:
//! 1000 steps of the counter agreeing in two processes.

use std::cell::Cell;
use std::collections::HashMap;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use keldysh_core::{
    CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
    RulesVersion, put_u64, step,
};
use keldysh_testkit::{Divergence, two_processes, two_runs};

const PARTY_SEED: u64 = 0x05EE_D3A7;

/// A two-phase counter: "apply" adds each command with a
/// stream-drawn bonus, "settle" records the closing value.
struct CounterState {
    value: u64,
}

impl CanonicalState for CounterState {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.value);
    }
}

struct CounterRules;

static PHASES: &[Phase] = &[Phase { name: "apply" }, Phase { name: "settle" }];

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
            "apply" => {
                for amount in commands {
                    let bonus = rng.next_u64() % 5;
                    state.value += *amount + bonus;
                }
            }
            "settle" => events.push(state.value),
            other => unreachable!("the engine executes only declared phases: {other:?}"),
        }
        Ok(())
    }
}

use std::convert::Infallible;

/// The deterministic party: a fresh counter party, checkpoint hash
/// every `every`-th step.
fn counter_checkpoints(steps: u64, every: u64) -> Vec<u64> {
    let rules = CounterRules;
    let mut state = CounterState { value: 1 };
    let streams = RngStreams::new(PARTY_SEED);
    let mut hashes = Vec::new();
    for step_index in 0..steps {
        let envelopes = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: (step_index % 7) + 1,
        }];
        step(&mut state, step_index, &envelopes, &rules, &streams).unwrap();
        if step_index % every == 0 {
            hashes.push(state.state_hash());
        }
    }
    hashes
}

/// The deliberately undisciplined state: the iteration of the map
/// leaks into a hashed field. The map itself stays outside the
/// canonical bytes — the violation under test is the order, not the
/// contents.
struct PollutedState {
    noise: u64,
    value: u64,
    map: HashMap<u64, u64>,
}

impl CanonicalState for PollutedState {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.noise);
        put_u64(out, self.value);
    }
}

struct PollutedRules;

impl Rules for PollutedRules {
    type State = PollutedState;
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
        state: &mut PollutedState,
        commands: &[&u64],
        _rng: &mut ChaCha8Rng,
        events: &mut Vec<u64>,
    ) -> Result<(), Infallible> {
        match phase.name {
            "apply" => {
                // The fold is order-sensitive: XOR or sum would be
                // invariant under the permutation and hide the very
                // violation the sanitizer hunts.
                let acc = state
                    .map
                    .keys()
                    .fold(0u64, |acc, key| acc.wrapping_mul(31).wrapping_add(*key));
                state.noise = acc;
                for amount in commands {
                    state.value += **amount;
                }
            }
            "settle" => events.push(state.noise),
            other => unreachable!("the engine executes only declared phases: {other:?}"),
        }
        Ok(())
    }
}

/// The polluted party: a fresh map (a fresh `RandomState`) per run —
/// the iteration order differs between runs and, with certainty,
/// between processes.
fn polluted_checkpoints(steps: u64) -> Vec<u64> {
    let rules = PollutedRules;
    let mut state = PollutedState {
        noise: 0,
        value: 0,
        map: (0..50).map(|key| (key, key * 3)).collect(),
    };
    let streams = RngStreams::new(PARTY_SEED);
    let mut hashes = Vec::new();
    for step_index in 0..steps {
        let envelopes = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: 1,
        }];
        step(&mut state, step_index, &envelopes, &rules, &streams).unwrap();
        if step_index % 10 == 0 {
            hashes.push(state.state_hash());
        }
    }
    hashes
}

#[test]
fn two_runs_accept_a_deterministic_party() {
    assert!(two_runs(|| counter_checkpoints(100, 10)).is_ok());
}

#[test]
fn two_processes_accept_a_deterministic_party() {
    assert!(
        two_processes("two_processes_accept_a_deterministic_party", || {
            counter_checkpoints(100, 10)
        })
        .is_ok()
    );
}

#[test]
fn two_runs_catch_per_run_nondeterminism() {
    let divergence = two_runs(|| polluted_checkpoints(50)).unwrap_err();
    assert!(
        matches!(divergence, Divergence::Checkpoint { checkpoint: 0, .. }),
        "the map order leaks at the first checkpoint: {divergence}"
    );
    assert!(
        divergence
            .to_string()
            .starts_with("first divergence at checkpoint 0")
    );
}

#[test]
fn two_processes_catch_cross_process_nondeterminism() {
    let divergence = two_processes("two_processes_catch_cross_process_nondeterminism", || {
        polluted_checkpoints(50)
    })
    .unwrap_err();
    assert!(
        matches!(divergence, Divergence::Checkpoint { checkpoint: 0, .. }),
        "the map order differs between the processes: {divergence}"
    );
}

#[test]
fn thousand_steps_agree_in_two_processes() {
    // The E-02 exit criterion: the demo simulation — 1000 steps of
    // the counter with arbitrary commands — identical checkpoint
    // hashes in two independent processes.
    assert_eq!(counter_checkpoints(1000, 10).len(), 100);
    assert!(
        two_processes("thousand_steps_agree_in_two_processes", || {
            counter_checkpoints(1000, 10)
        })
        .is_ok()
    );
}

#[test]
fn a_length_difference_is_reported() {
    let calls = Cell::new(0);
    let party = || {
        let call = calls.get();
        calls.set(call + 1);
        Vec::from([1u64, 2, 3])
            .into_iter()
            .take(if call == 0 { 3 } else { 2 })
            .collect()
    };
    assert_eq!(
        two_runs(party),
        Err(Divergence::Length {
            expected: 3,
            actual: 2
        })
    );
}
