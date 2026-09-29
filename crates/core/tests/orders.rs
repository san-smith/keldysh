//! Command-ordering tests: any permutation of the same envelopes
//! produces the identical step result; identical redeliveries
//! collapse; conflicts and wrong targets are explicit errors.

use rand_chacha::ChaCha8Rng;

use keldysh_core::{
    CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
    RulesVersion, canonical_order, put_u64, step,
};

/// A demo payload: the actor and the amount, so the attribution and
/// the conflicts are observable in the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DemoCommand {
    actor: u64,
    amount: u64,
}

struct DemoState {
    total: u64,
}

impl CanonicalState for DemoState {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_u64(out, self.total);
    }
}

struct DemoRules;

static PHASES: &[Phase] = &[Phase { name: "apply" }];

impl Rules for DemoRules {
    type State = DemoState;
    type Command = DemoCommand;
    type Event = (u64, u64);
    type Error = std::convert::Infallible;

    fn phases(&self) -> &'static [Phase] {
        PHASES
    }

    fn version(&self) -> RulesVersion {
        RulesVersion::declaring(1, PHASES)
    }

    fn apply_phase(
        &self,
        _phase: &'static Phase,
        state: &mut DemoState,
        commands: &[&Self::Command],
        _rng: &mut ChaCha8Rng,
        events: &mut Vec<(u64, u64)>,
    ) -> Result<(), std::convert::Infallible> {
        for command in commands {
            state.total += command.amount;
            events.push((command.actor, command.amount));
        }
        Ok(())
    }
}

fn envelopes() -> Vec<CommandEnvelope<DemoCommand>> {
    vec![
        CommandEnvelope {
            player: PlayerId(3),
            target_step: 7,
            idempotency: IdempotencyId(30),
            payload: DemoCommand {
                actor: 3,
                amount: 30,
            },
        },
        CommandEnvelope {
            player: PlayerId(1),
            target_step: 7,
            idempotency: IdempotencyId(10),
            payload: DemoCommand {
                actor: 1,
                amount: 10,
            },
        },
        CommandEnvelope {
            player: PlayerId(2),
            target_step: 7,
            idempotency: IdempotencyId(20),
            payload: DemoCommand {
                actor: 2,
                amount: 20,
            },
        },
    ]
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for (index, head) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for mut tail in permutations(&rest) {
            let mut permutation = vec![head.clone()];
            permutation.append(&mut tail);
            out.push(permutation);
        }
    }
    out
}

#[test]
fn every_permutation_yields_the_identical_canonical_order() {
    let command_envelopes = envelopes();
    let reference = canonical_order(&command_envelopes, 7).unwrap();
    let reference_payloads: Vec<u64> = reference
        .iter()
        .map(|envelope| envelope.payload.amount)
        .collect();
    assert_eq!(reference_payloads, vec![10, 20, 30], "sorted by player");

    for permutation in permutations(&command_envelopes) {
        let ordered = canonical_order(&permutation, 7).unwrap();
        let payloads: Vec<u64> = ordered
            .iter()
            .map(|envelope| envelope.payload.amount)
            .collect();
        assert_eq!(
            payloads, reference_payloads,
            "the canonical order never depends on the delivery order"
        );
    }
}

#[test]
fn every_permutation_through_the_step_yields_identical_events() {
    let commands = envelopes();
    let reference = {
        let mut state = DemoState { total: 0 };
        let (events, outcome) = {
            let outcome = step(&mut state, 7, &commands, &DemoRules, &RngStreams::new(42)).unwrap();
            (outcome.events.clone(), outcome)
        };
        assert_eq!(state.total, 60, "10 + 20 + 30");
        (events, outcome.phases)
    };

    for permutation in permutations(&commands) {
        let mut state = DemoState { total: 0 };
        let (events, phases) = {
            let outcome = step(
                &mut state,
                7,
                &permutation,
                &DemoRules,
                &RngStreams::new(42),
            )
            .unwrap();
            (outcome.events.clone(), outcome.phases)
        };
        assert_eq!(
            (events, phases),
            reference,
            "the delivery order never changes the step"
        );
        assert_eq!(state.total, 60);
    }
}

#[test]
fn identical_redeliveries_collapse() {
    let commands = vec![
        CommandEnvelope {
            player: PlayerId(1),
            target_step: 7,
            idempotency: IdempotencyId(10),
            payload: DemoCommand {
                actor: 1,
                amount: 10,
            },
        },
        // The same delivery redelivered: same player, idempotency,
        // and payload.
        CommandEnvelope {
            player: PlayerId(1),
            target_step: 7,
            idempotency: IdempotencyId(10),
            payload: DemoCommand {
                actor: 1,
                amount: 10,
            },
        },
    ];
    let ordered = canonical_order(&commands, 7).unwrap();
    assert_eq!(
        ordered.len(),
        1,
        "the redelivery collapses into one command"
    );

    let mut state = DemoState { total: 0 };
    let (events, _) = {
        let outcome = step(&mut state, 7, &commands, &DemoRules, &RngStreams::new(42)).unwrap();
        (outcome.events.clone(), outcome)
    };
    assert_eq!(events, vec![(1, 10)], "the collapsed command applies once");
}

#[test]
fn conflicting_payloads_are_rejected() {
    let commands = vec![
        CommandEnvelope {
            player: PlayerId(1),
            target_step: 7,
            idempotency: IdempotencyId(10),
            payload: DemoCommand {
                actor: 1,
                amount: 10,
            },
        },
        CommandEnvelope {
            player: PlayerId(1),
            target_step: 7,
            idempotency: IdempotencyId(10),
            payload: DemoCommand {
                actor: 1,
                amount: 999,
            },
        },
    ];
    assert_eq!(
        canonical_order(&commands, 7).unwrap_err().to_string(),
        "player p:1 reused idempotency identifier i:10 for two different payloads (envelopes 0 and 1)"
    );
}

#[test]
fn wrong_target_steps_are_rejected() {
    let mut commands = envelopes();
    commands[0].target_step = 8;
    let error = canonical_order(&commands, 7).unwrap_err();
    assert!(matches!(
        error,
        keldysh_core::CommandOrderError::WrongTargetStep {
            expected: 7,
            actual: 8,
            ..
        }
    ));
}

#[test]
fn reserved_identifiers_are_rejected() {
    let mut commands = envelopes();
    commands[0].player = PlayerId(0);
    assert!(matches!(
        canonical_order(&commands, 7),
        Err(keldysh_core::CommandOrderError::ReservedIdentifier {
            field: "player",
            ..
        })
    ));

    let mut commands = envelopes();
    commands[0].idempotency = IdempotencyId(0);
    assert!(matches!(
        canonical_order(&commands, 7),
        Err(keldysh_core::CommandOrderError::ReservedIdentifier {
            field: "idempotency",
            ..
        })
    ));
}
