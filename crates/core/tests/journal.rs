//! Event-journal tests: chronology enforced by the container, empty
//! steps recorded as entries, and the non-influence sanitizer — a
//! party that journals every step and a party that discards the
//! outcomes hash identically.

use std::convert::Infallible;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use keldysh_core::{
    CanonicalState, CommandEnvelope, EventJournal, IdempotencyId, JournalError, Phase, PlayerId,
    RngStreams, Rules, RulesVersion, put_u64, step,
};

/// A minimal counter: "apply" adds each command with a stream-drawn
/// bonus, "settle" records the closing value — the same shape the
/// step and state-hash suites use.
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

/// Plays 100 steps of one command per day; `journal_events` decides
/// whether the session keeps the journal or drops the outcomes. The
/// hash sequence is recorded either way.
fn play(journal_events: bool) -> (Vec<u64>, EventJournal<u64>) {
    let mut state = CounterState { value: 1 };
    let streams = RngStreams::new(0x0CED_7EA1);
    let mut journal = EventJournal::new();
    let mut hashes = Vec::with_capacity(100);
    for step_index in 0..100_u64 {
        let envelopes = [CommandEnvelope {
            player: PlayerId(1),
            target_step: step_index,
            idempotency: IdempotencyId(step_index + 1),
            payload: (step_index % 5) + 1,
        }];
        let outcome = step(&mut state, step_index, &envelopes, &CounterRules, &streams).unwrap();
        hashes.push(state.state_hash());
        if journal_events {
            journal.record(step_index, outcome.events).unwrap();
        }
    }
    (hashes, journal)
}

#[test]
fn journaling_never_influences_the_step() {
    let (with_journal, journal) = play(true);
    let (without_journal, _) = play(false);

    assert_eq!(
        with_journal, without_journal,
        "the journal is pure output: recording never changes the run"
    );

    // One entry per step, the settle event in each, chronological.
    assert_eq!(journal.iter().count(), 100);
    assert_eq!(journal.last_step(), Some(99));
    let steps: Vec<u64> = journal.iter().map(|entry| entry.step()).collect();
    assert_eq!(steps.first(), Some(&0));
    assert_eq!(steps.last(), Some(&99));
    assert!(
        steps.windows(2).all(|window| window[0] < window[1]),
        "entries are strictly ascending"
    );
    assert_eq!(journal.total_events(), 100, "one settle event per step");
}

#[test]
fn eventless_steps_are_recorded_as_empty_entries() {
    let mut journal = EventJournal::new();
    journal.record(0, vec![1]).unwrap();
    journal.record(1, Vec::<u64>::new()).unwrap();
    journal.record(2, vec![2, 3]).unwrap();

    let counts: Vec<usize> = journal.iter().map(|entry| entry.events().len()).collect();
    assert_eq!(counts, [1, 0, 2], "the empty step keeps its entry");
    assert_eq!(journal.total_events(), 3);
}

#[test]
fn stale_and_repeated_records_are_rejected_and_leave_it_consistent() {
    let mut journal = EventJournal::new();
    journal.record(10, vec![0]).unwrap();

    assert_eq!(
        journal.record(10, vec![1]).unwrap_err(),
        JournalError::OutOfOrderStep {
            last_recorded: 10,
            rejected: 10,
        },
        "the same step is never recorded twice"
    );
    assert_eq!(
        journal.record(9, vec![2]).unwrap_err(),
        JournalError::OutOfOrderStep {
            last_recorded: 10,
            rejected: 9,
        }
    );

    // Both rejections left the journal unchanged.
    assert_eq!(journal.iter().count(), 1);
    assert_eq!(journal.total_events(), 1);
    assert_eq!(journal.last_step(), Some(10));

    // The next valid step is still accepted.
    journal.record(11, vec![3]).unwrap();
    assert_eq!(journal.last_step(), Some(11));
    assert_eq!(journal.total_events(), 2);
}
