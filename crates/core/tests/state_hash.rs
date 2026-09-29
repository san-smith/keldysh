//! Canonical-state-hash tests: stability across constructions,
//! insertion-order independence (the HashMap discipline of ADR-0002
//! §5.7), sensitivity to every serialized field, the pinned golden
//! vector, and the hash sequence through the step engine.

use std::collections::HashMap;
use std::convert::Infallible;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use keldysh_core::{
    CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
    RulesVersion, put_i64, put_len_prefixed, put_seq_len, put_u64, step,
};

/// A demo state exercising every encoder helper: unsigned and signed
/// integers, a byte string, a map (sorted keys), and a sequence.
struct Ledger {
    day: u64,
    delta: i64,
    memo: String,
    balances: HashMap<u64, u64>,
    entries: Vec<u64>,
}

impl CanonicalState for Ledger {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.day);
        put_i64(out, self.delta);
        put_len_prefixed(out, self.memo.as_bytes());
        let mut ids: Vec<u64> = self.balances.keys().copied().collect();
        ids.sort_unstable();
        put_seq_len(out, ids.len());
        for id in ids {
            put_u64(out, id);
            put_u64(out, self.balances[&id]);
        }
        put_seq_len(out, self.entries.len());
        for entry in &self.entries {
            put_u64(out, *entry);
        }
    }
}

/// The golden state: fixed content, filled in a deliberately
/// unsorted order — the digest must never notice.
fn golden() -> Ledger {
    let mut balances = HashMap::new();
    balances.insert(7, 700);
    balances.insert(2, 20);
    balances.insert(5, 50);
    Ledger {
        day: 42,
        delta: -7,
        memo: "checkpoint".into(),
        balances,
        entries: vec![3, 1, 2],
    }
}

#[test]
fn the_same_state_hashes_identically_across_constructions() {
    assert_eq!(
        golden().state_hash(),
        golden().state_hash(),
        "identical content, identical digest"
    );
}

#[test]
fn fixed_state_pins_the_golden_hash() {
    // The golden vector pins the canonical encoding itself: a change
    // in the helpers, the layout, or the serialization shows up here.
    // Regenerate deliberately, never silently.
    assert_eq!(golden().state_hash(), 0x4ef5_de3a_0d26_f125);
}

#[test]
fn string_capacity_never_reaches_the_digest() {
    let mut memo = String::with_capacity(4096);
    memo.push_str("checkpoint");
    let mut balances = HashMap::new();
    balances.insert(7, 700);
    balances.insert(2, 20);
    balances.insert(5, 50);
    let padded = Ledger {
        day: 42,
        delta: -7,
        memo,
        balances,
        entries: vec![3, 1, 2],
    };
    assert_eq!(
        padded.state_hash(),
        golden().state_hash(),
        "spare capacity is not state"
    );
}

#[test]
fn insertion_order_never_matters() {
    let hash_of = |contents: &[(u64, u64)]| {
        let mut balances = HashMap::new();
        for (id, value) in contents {
            balances.insert(*id, *value);
        }
        Ledger {
            day: 42,
            delta: -7,
            memo: "checkpoint".into(),
            balances,
            entries: vec![3, 1, 2],
        }
        .state_hash()
    };

    let ascending = [(2, 20), (5, 50), (7, 700)];
    let descending = [(7, 700), (5, 50), (2, 20)];
    // A deterministic scramble of the same three entries.
    let scrambled = [(5, 50), (7, 700), (2, 20)];

    let reference = hash_of(&ascending);
    assert_eq!(reference, golden().state_hash(), "ascending matches golden");
    assert_eq!(hash_of(&descending), reference, "descending agrees");
    assert_eq!(hash_of(&scrambled), reference, "scrambled agrees");
}

#[test]
fn changing_any_serialized_field_changes_the_hash() {
    let reference = golden().state_hash();

    let variants: Vec<(&str, Ledger)> = vec![
        (
            "day",
            Ledger {
                day: 43,
                ..golden()
            },
        ),
        (
            "delta",
            Ledger {
                delta: -6,
                ..golden()
            },
        ),
        (
            "memo",
            Ledger {
                memo: "checkpoints".into(),
                ..golden()
            },
        ),
        ("balance value", {
            let mut ledger = golden();
            ledger.balances.insert(5, 51);
            ledger
        }),
        ("balance key set", {
            let mut ledger = golden();
            ledger.balances.insert(9, 90);
            ledger
        }),
        (
            "entries",
            Ledger {
                entries: vec![3, 1, 3],
                ..golden()
            },
        ),
    ];
    assert!(!variants.is_empty(), "the canary covers the fields");
    for (field, variant) in variants {
        assert_ne!(
            variant.state_hash(),
            reference,
            "a changed {field} must change the digest"
        );
    }
}

/// A minimal two-phase counter: "apply" adds each command with a
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

/// Plays 100 steps of one command per day and records the state hash
/// after every step.
fn play() -> Vec<u64> {
    let mut state = CounterState { value: 1 };
    let streams = RngStreams::new(0xABCD_EF01);
    let mut hashes = Vec::with_capacity(100);
    for step_index in 0..100_u64 {
        let envelopes = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: (step_index % 7) + 1,
        }];
        step(&mut state, step_index, &envelopes, &CounterRules, &streams).unwrap();
        hashes.push(state.state_hash());
    }
    hashes
}

#[test]
fn hundred_steps_hash_identically_in_two_runs() {
    let first = play();
    let second = play();
    assert_eq!(first.len(), 100);
    assert_eq!(
        first, second,
        "two independent runs produce the identical hash sequence"
    );
}
