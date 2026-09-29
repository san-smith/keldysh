//! Snapshot tests: the roundtrip preserves the state hash, the
//! snapshot → mutate → restore cycle returns the original hash, a
//! continuation from the snapshot matches the reference run, and the
//! rejections — foreign magic, unknown format version, foreign rules
//! version, corrupted payload, trailing bytes, truncation — are
//! explicit.

use std::convert::Infallible;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use keldysh_core::{
    CanonicalReadError, CanonicalReader, CanonicalState, CommandEnvelope, IdempotencyId, Phase,
    PlayerId, RngStreams, Rules, RulesVersion, put_u64, step,
};
use keldysh_replay::snapshot::{self, SNAPSHOT_FORMAT, SnapshotState};
use keldysh_replay::{Recording, ReplayRecord, verify};

const PARTY_SEED: u64 = 0x05EE_D2A7;
const SNAPSHOT_STEP: u64 = 49;

/// A two-phase counter: "apply" adds each command with a
/// stream-drawn bonus, "settle" records the closing value.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CounterState {
    value: u64,
}

impl CanonicalState for CounterState {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.value);
    }
}

impl SnapshotState for CounterState {
    fn from_canonical(reader: &mut CanonicalReader<'_>) -> Result<Self, CanonicalReadError> {
        Ok(Self {
            value: reader.read_u64()?,
        })
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

/// The same counter with a bumped revision — a foreign version for
/// the rejection tests.
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
        unreachable!("the version check rejects before any payload is read")
    }
}

/// What one session produced: the record, the final state, the
/// per-step journal for continuations, and the mid-party snapshot.
struct Session {
    record: ReplayRecord<u64>,
    final_state: CounterState,
    journal: Vec<Vec<CommandEnvelope<u64>>>,
    snapshot: Option<Vec<u8>>,
}

/// Plays the session loop the way E-08 will: record, step, snapshot
/// at the chosen step.
fn session(steps: u64, snapshot_at: Option<u64>) -> Session {
    let rules = CounterRules;
    let mut recording = Recording::start(&rules, PARTY_SEED, &CounterState { value: 1 });
    let mut state = CounterState { value: 1 };
    let streams = RngStreams::new(PARTY_SEED);
    let mut journal = Vec::new();
    let mut snapshot = None;
    for step_index in 0..steps {
        let envelopes = vec![CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: (step_index % 5) + 1,
        }];
        recording.record_commands(step_index, &envelopes);
        step(&mut state, step_index, &envelopes, &rules, &streams).unwrap();
        journal.push(envelopes);
        if snapshot_at == Some(step_index) {
            snapshot = Some(snapshot::save(&state, &rules));
        }
        recording.record_checkpoint(step_index, &state);
    }
    Session {
        record: recording.finish(),
        final_state: state,
        journal,
        snapshot,
    }
}

/// Continues a restored state through the journal from `from`.
fn continue_from(
    mut state: CounterState,
    journal: &[Vec<CommandEnvelope<u64>>],
    from: u64,
) -> CounterState {
    let rules = CounterRules;
    let streams = RngStreams::new(PARTY_SEED);
    for step_index in from..journal.len() as u64 {
        step(
            &mut state,
            step_index,
            &journal[step_index as usize],
            &rules,
            &streams,
        )
        .unwrap();
    }
    state
}

#[test]
fn the_roundtrip_preserves_the_state() {
    let party = session(100, None);
    let bytes = snapshot::save(&party.final_state, &CounterRules);
    let restored = snapshot::restore(&bytes, &CounterRules).unwrap();

    assert_eq!(
        restored.state_hash(),
        party.final_state.state_hash(),
        "the roundtrip is byte-identical"
    );
}

#[test]
fn snapshot_mutate_restore_returns_the_original_hash() {
    let party = session(100, Some(SNAPSHOT_STEP));
    let bytes = party.snapshot.as_ref().unwrap();

    // The snapshot hashes to the state at the snapshot step.
    let first_restore = snapshot::restore(bytes, &CounterRules).unwrap();
    let snapshot_hash = first_restore.state_hash();

    // Mutate: continue the party for ten steps past the snapshot.
    let mut mutated = first_restore;
    let rules = CounterRules;
    let streams = RngStreams::new(PARTY_SEED);
    for step_index in SNAPSHOT_STEP + 1..SNAPSHOT_STEP + 11 {
        step(
            &mut mutated,
            step_index,
            &party.journal[step_index as usize],
            &rules,
            &streams,
        )
        .unwrap();
    }
    assert_ne!(mutated.state_hash(), snapshot_hash);

    // Restore the same snapshot again: the original hash returns.
    let second_restore = snapshot::restore(bytes, &CounterRules).unwrap();
    assert_eq!(second_restore.state_hash(), snapshot_hash);
}

#[test]
fn a_continuation_from_the_snapshot_matches_the_reference_run() {
    // The reference: 100 steps straight through.
    let reference = session(100, Some(SNAPSHOT_STEP));

    // The detour: restore the mid-party snapshot, continue to the end
    // through the journal the session kept.
    let bytes = reference.snapshot.as_ref().unwrap();
    let restored = snapshot::restore(bytes, &CounterRules).unwrap();
    let continued = continue_from(restored, &reference.journal, SNAPSHOT_STEP + 1);

    assert_eq!(
        continued.state_hash(),
        reference.final_state.state_hash(),
        "the detour through the snapshot never shows in the final state"
    );

    // The full record still verifies from the very start.
    verify(&reference.record, CounterState { value: 1 }, &CounterRules).unwrap();
}

#[test]
fn a_foreign_rules_version_is_rejected_with_both_versions() {
    let party = session(10, Some(4));
    let bytes = party.snapshot.as_ref().unwrap();
    assert_eq!(
        snapshot::restore(bytes, &BumpedRules),
        Err(snapshot::SnapshotError::RulesVersionMismatch {
            recorded: CounterRules.version(),
            actual: BumpedRules.version(),
        })
    );
    assert_eq!(
        snapshot::restore(bytes, &BumpedRules)
            .unwrap_err()
            .to_string(),
        format!(
            "snapshot rules version mismatch — recorded {}, restoring under {}",
            CounterRules.version(),
            BumpedRules.version()
        )
    );
}

#[test]
fn foreign_magic_and_unknown_versions_are_rejected() {
    let bytes = session(10, Some(4)).snapshot.unwrap();

    let garbage = vec![0xff; 64];
    assert_eq!(
        snapshot::restore(&garbage, &CounterRules),
        Err(snapshot::SnapshotError::ForeignMagic)
    );

    // Patch the format version in place: 1 → 2.
    let mut future = bytes.clone();
    future[5..9].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        snapshot::restore(&future, &CounterRules),
        Err(snapshot::SnapshotError::UnknownFormatVersion {
            found: 2,
            supported: SNAPSHOT_FORMAT,
        })
    );
    assert_eq!(SNAPSHOT_FORMAT, 1, "the reader reads format version 1");
}

#[test]
fn a_corrupted_payload_is_detected_by_the_header_hash() {
    let bytes = session(10, Some(4)).snapshot.unwrap();

    // Flip one payload byte: the decode may still parse — the hash
    // catches the corruption.
    let mut corrupted = bytes.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0x01;

    let mismatch = snapshot::restore(&corrupted, &CounterRules).unwrap_err();
    let snapshot::SnapshotError::HashMismatch { recorded, actual } = mismatch else {
        panic!("expected a hash mismatch, got a different rejection");
    };
    assert_ne!(recorded, actual, "the flip reached the restored state");
    assert!(mismatch.to_string().contains("corrupted"));
}

#[test]
fn trailing_bytes_and_truncation_are_rejected() {
    let bytes = session(10, Some(4)).snapshot.unwrap();

    let mut padded = bytes.clone();
    padded.push(0);
    assert_eq!(
        snapshot::restore(&padded, &CounterRules),
        Err(snapshot::SnapshotError::TrailingBytes { remaining: 1 })
    );

    let truncated = &bytes[..bytes.len() - 3];
    assert!(matches!(
        snapshot::restore(truncated, &CounterRules),
        Err(snapshot::SnapshotError::Malformed { .. })
    ));
}
