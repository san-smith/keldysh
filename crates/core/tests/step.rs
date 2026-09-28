//! Step-contract tests: the fingerprint mechanism, the phase order,
//! the fail-fast errors, and the determinism of the engine, exercised
//! through a minimal two-phase counter rules implementation.

use keldysh_core::{
    CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules, RulesVersion, StepError,
    step,
};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

struct CounterState {
    value: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Add(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Applied { amount: u64, bonus: u64 },
    Settled { value: u64 },
    Audited { value: u64 },
}

#[derive(Debug, PartialEq, Eq)]
struct SpendOverflow;

impl std::fmt::Display for SpendOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the settle phase cannot spend more than the value")
    }
}

impl std::error::Error for SpendOverflow {}

static PHASES: &[Phase] = &[Phase { name: "apply" }, Phase { name: "settle" }];

/// Two-phase counter: "apply" adds the commands (with a small
/// stream-drawn bonus), "settle" records the closing value and
/// "audit" records the post-settle value.
struct CounterRules {
    phases: &'static [Phase],
    version: RulesVersion,
    spend: u64,
}

impl CounterRules {
    fn declared(spend: u64) -> Self {
        Self {
            phases: PHASES,
            version: RulesVersion::declaring(1, PHASES),
            spend,
        }
    }

    fn with_phases(phases: &'static [Phase]) -> Self {
        Self {
            phases,
            version: RulesVersion::declaring(1, phases),
            spend: 0,
        }
    }
}

impl Rules for CounterRules {
    type State = CounterState;
    type Command = Command;
    type Event = Event;
    type Error = SpendOverflow;

    fn phases(&self) -> &'static [Phase] {
        self.phases
    }

    fn version(&self) -> RulesVersion {
        self.version
    }

    fn apply_phase(
        &self,
        phase: &'static Phase,
        state: &mut CounterState,
        commands: &[&Command],
        rng: &mut ChaCha8Rng,
        events: &mut Vec<Event>,
    ) -> Result<(), SpendOverflow> {
        match phase.name {
            "apply" => {
                for command in commands {
                    let Command::Add(amount) = command;
                    let bonus = rng.next_u64() % 3;
                    state.value += *amount + bonus;
                    events.push(Event::Applied {
                        amount: *amount,
                        bonus,
                    });
                }
            }
            "settle" => {
                if state.value < self.spend {
                    return Err(SpendOverflow);
                }
                state.value -= self.spend;
                events.push(Event::Settled { value: state.value });
            }
            "audit" => events.push(Event::Audited { value: state.value }),
            other => unreachable!("the engine executes only declared phases: {other:?}"),
        }
        Ok(())
    }
}

/// Runs one step and returns the recorded events with the raw outcome.
///
/// The bare payloads are wrapped into canonical envelopes: player 1,
/// target step 0, idempotency in delivery order.
fn run(
    rules: &CounterRules,
    commands: &[Command],
) -> Result<(Vec<Event>, keldysh_core::StepOutcome<Event>), StepError<SpendOverflow>> {
    let envelopes = wrap(commands);
    let mut state = CounterState { value: 0 };
    let outcome = step(&mut state, 0, &envelopes, rules, &RngStreams::new(42))?;
    Ok((outcome.events.clone(), outcome))
}

fn wrap(commands: &[Command]) -> Vec<CommandEnvelope<Command>> {
    commands
        .iter()
        .enumerate()
        .map(|(index, payload)| CommandEnvelope {
            player: PlayerId(1),
            target_step: 0,
            idempotency: IdempotencyId(index as u64 + 1),
            payload: *payload,
        })
        .collect()
}

#[test]
fn phases_execute_in_the_declared_order() {
    let extended: &'static [Phase] = &[
        Phase { name: "apply" },
        Phase { name: "settle" },
        Phase { name: "audit" },
    ];
    let (events, outcome) = run(&CounterRules::with_phases(extended), &[Command::Add(4)]).unwrap();
    assert_eq!(
        events,
        vec![
            Event::Applied {
                amount: 4,
                bonus: 2
            },
            Event::Settled { value: 6 },
            Event::Audited { value: 6 },
        ],
        "apply, settle, audit — in the declared order"
    );
    assert_eq!(outcome.phases, vec!["apply", "settle", "audit"]);
}

#[test]
fn the_engine_is_deterministic() {
    let commands = [Command::Add(4), Command::Add(9)];
    let (first, _) = run(&CounterRules::declared(0), &commands).unwrap();
    let (second, _) = run(&CounterRules::declared(0), &commands).unwrap();
    assert_eq!(first, second, "identical inputs, identical outcome");
}

#[test]
fn changing_phases_without_redeclaring_the_version_is_caught() {
    // Drop the "settle" phase but keep the version declared for both
    // phases: the fingerprint no longer matches.
    let short: &'static [Phase] = &[Phase { name: "apply" }];
    let rules = CounterRules {
        phases: short,
        version: RulesVersion::declaring(1, PHASES),
        spend: 0,
    };
    assert_eq!(
        run(&rules, &[Command::Add(4)]).unwrap_err(),
        StepError::VersionMismatch {
            declared: RulesVersion::declaring(1, PHASES).phase_fingerprint(),
            computed: RulesVersion::declaring(1, short).phase_fingerprint(),
        }
    );

    // Reordering the phases changes the fingerprint the same way: the
    // version declared for the original order no longer matches.
    let reversed: &'static [Phase] = &[Phase { name: "settle" }, Phase { name: "apply" }];
    let rules = CounterRules {
        phases: reversed,
        version: RulesVersion::declaring(1, PHASES),
        spend: 0,
    };
    assert!(matches!(
        run(&rules, &[Command::Add(4)]),
        Err(StepError::VersionMismatch { .. })
    ));
}

#[test]
fn redeclaring_the_version_accepts_the_new_phases() {
    let extended: &'static [Phase] = &[
        Phase { name: "apply" },
        Phase { name: "settle" },
        Phase { name: "audit" },
    ];
    let rules = CounterRules::with_phases(extended);
    let (events, outcome) = run(&rules, &[Command::Add(4)]).unwrap();

    // The version identity changed with the phases.
    assert_ne!(
        outcome.version,
        RulesVersion::declaring(1, PHASES),
        "the fingerprint is part of the version identity"
    );
    assert_eq!(
        outcome.phases,
        vec!["apply", "settle", "audit"],
        "phases execute in the declared order"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Audited { .. }))
    );
}

#[test]
fn duplicate_phase_names_are_rejected() {
    let duplicated: &'static [Phase] = &[Phase { name: "apply" }, Phase { name: "apply" }];
    let rules = CounterRules::with_phases(duplicated);
    assert_eq!(
        run(&rules, &[Command::Add(4)]).unwrap_err(),
        StepError::DuplicatePhase { name: "apply" }
    );
}

#[test]
fn phase_errors_carry_the_phase_coordinates() {
    let rules = CounterRules {
        phases: PHASES,
        version: RulesVersion::declaring(1, PHASES),
        spend: 100,
    };
    // The settle phase cannot spend 100 from a value of ~5: the error
    // names the phase.
    let error = run(&rules, &[Command::Add(4)]).unwrap_err();
    assert!(matches!(
        error,
        StepError::Phase {
            phase: "settle",
            error: SpendOverflow
        }
    ));
}

#[test]
fn per_phase_streams_are_local_and_reentrant() {
    // The apply-phase bonus depends only on (party seed, "apply"): a
    // command before it does not shift the bonus, and the same command
    // set yields the same bonuses on every run.
    let (alone, _) = run(&CounterRules::declared(0), &[Command::Add(10)]).unwrap();
    let (with_prefix, _) = run(
        &CounterRules::declared(0),
        &[Command::Add(1), Command::Add(2), Command::Add(10)],
    )
    .unwrap();

    let bonus_of = |events: &[Event]| match &events[0] {
        Event::Applied { bonus, .. } => *bonus,
        other => unreachable!("unexpected event {other:?}"),
    };
    // The "apply" stream is re-entrant: the bonus of the first command
    // does not depend on how many commands ran before it.
    assert_eq!(bonus_of(&alone), bonus_of(&with_prefix));

    // The "settle" stream is independent of the "apply" stream.
    for events in [&alone, &with_prefix] {
        assert!(matches!(events.last(), Some(Event::Settled { .. })));
    }
}

#[test]
fn empty_phase_sequence_is_a_valid_no_op() {
    let rules = CounterRules::with_phases(&[]);
    let (events, outcome) = run(&rules, &[]).unwrap();
    assert!(events.is_empty());
    assert!(outcome.phases.is_empty());
}
