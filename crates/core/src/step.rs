//! The step contract: the [`Rules`] trait the game implements and the
//! [`step`] engine that executes the declared phases.
//!
//! # The contract
//!
//! The game describes:
//!
//! - the [`State`], [`Command`](Rules::Command), and
//!   [`Event`](Rules::Event) types of its simulation,
//! - the [`Phase`] sequence — the order the parts of a step run in,
//!   part of the rules version identity,
//! - the per-phase logic in [`Rules::apply_phase`].
//!
//! The engine validates the declaration (unique phase names, the
//! version's phase fingerprint matching the declared sequence) and
//! executes the phases in order, handing each phase its own RNG stream
//! derived from the phase name — introducing a phase never shifts the
//! randomness of another phase (the locality of ADR-0005, by
//! construction).
//!
//! # Rules versions
//!
//! [`RulesVersion`] carries the fingerprint of the declared phase
//! sequence plus a semantic revision. The engine recomputes the
//! fingerprint and rejects the step when the version does not match:
//! changing the phases without re-declaring the version is impossible.
//! Build versions with [`RulesVersion::declaring`], which computes the
//! fingerprint for you.
//!
//! # Fail-fast errors
//!
//! A rule error aborts the step with the offending phase's name
//! ([`StepError::Phase`]). The step does not roll the state back:
//! rules must validate commands before mutating (the validation phase
//! precedes the mutation phases in a well-formed declaration).
//!
//! # Example
//!
//! A minimal two-phase counter: the "apply" phase applies commands
//! (with a small deterministic bonus drawn from the phase stream), the
//! "settle" phase records the closing event.
//!
//! ```
//! use keldysh_core::{step, CanonicalState, Phase, Rules, RulesVersion, RngStreams, put_u64};
//! use rand_chacha::rand_core::RngCore;
//! use rand_chacha::ChaCha8Rng;
//!
//! struct CounterRules;
//!
//! struct CounterState {
//!     value: u64,
//! }
//!
//! impl CanonicalState for CounterState {
//!     fn write_canonical(&self, out: &mut Vec<u8>) {
//!         put_u64(out, self.value);
//!     }
//! }
//!
//! #[derive(Debug, PartialEq, Eq)]
//! enum Command {
//!     Add(u64),
//! }
//!
//! enum Event {
//!     Applied { amount: u64, bonus: u64 },
//!     Settled { value: u64 },
//! }
//!
//! #[derive(Debug)]
//! struct Never;
//!
//! impl std::fmt::Display for Never {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "never")
//!     }
//! }
//!
//! impl std::error::Error for Never {}
//!
//! static PHASES: &[Phase] = &[Phase { name: "apply" }, Phase { name: "settle" }];
//!
//! impl Rules for CounterRules {
//!     type State = CounterState;
//!     type Command = Command;
//!     type Event = Event;
//!     type Error = Never;
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
//!         phase: &'static Phase,
//!         state: &mut CounterState,
//!         commands: &[&Command],
//!         rng: &mut ChaCha8Rng,
//!         events: &mut Vec<Event>,
//!     ) -> Result<(), Never> {
//!         match phase.name {
//!             "apply" => {
//!                 for command in commands {
//!                     let Command::Add(amount) = command;
//!                     let bonus = rng.next_u64() % 3;
//!                     state.value += *amount + bonus;
//!                     events.push(Event::Applied { amount: *amount, bonus });
//!                 }
//!             }
//!             "settle" => events.push(Event::Settled { value: state.value }),
//!             _ => unreachable!("the engine executes only declared phases"),
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let mut state = CounterState { value: 0 };
//! let envelopes = [Command::Add(5), Command::Add(7)]
//!     .into_iter()
//!     .enumerate()
//!     .map(|(index, payload)| keldysh_core::CommandEnvelope {
//!         player: keldysh_core::PlayerId(1),
//!         target_step: 0,
//!         idempotency: keldysh_core::IdempotencyId(index as u64 + 1),
//!         payload,
//!     })
//!     .collect::<Vec<_>>();
//!
//! let outcome = step(
//!     &mut state,
//!     0,
//!     &envelopes,
//!     &CounterRules,
//!     &RngStreams::new(42),
//! )?;
//!
//! // Each phase stream bonus is 0..3, deterministic per phase name.
//! assert!((12..=16).contains(&state.value), "two additions plus two bonuses");
//! assert_eq!(outcome.events.len(), 3, "two applications and the settle");
//!
//! // The state is checkpoint-hashable by construction: the game calls
//! // `state_hash` at the steps it wants to compare across runs.
//! assert_ne!(state.state_hash(), 0, "the digest covers the canonical bytes");
//! # Ok::<(), keldysh_core::StepError<Never>>(())
//! ```
//!

use rand_chacha::ChaCha8Rng;

use crate::fnv::fnv1a64;
use crate::orders::{CommandEnvelope, CommandOrderError, canonical_order};
use crate::rng::RngStreams;
use crate::state::CanonicalState;

/// A named phase of the step: one entry of the declared sequence.
///
/// Names are stable identifiers (`'static`), unique within a rules
/// declaration, and double as the stream names of the phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Phase {
    /// The stable name of the phase; also the RNG stream name.
    pub name: &'static str,
}

/// The version of a rules declaration: the fingerprint of the phase
/// sequence plus the semantic revision.
///
/// The fingerprint is derived from the declared phases — changing the
/// sequence (order, membership, names) changes it, so a rules
/// declaration cannot alter its phases without changing the version.
/// Build versions with [`RulesVersion::declaring`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RulesVersion {
    phase_fingerprint: u64,
    revision: u64,
}

impl RulesVersion {
    /// Declares a version for the given phase sequence, computing the
    /// phase fingerprint.
    pub fn declaring(revision: u64, phases: &[Phase]) -> Self {
        Self {
            phase_fingerprint: phase_fingerprint(phases),
            revision,
        }
    }

    /// The fingerprint of the declared phase sequence.
    pub fn phase_fingerprint(&self) -> u64 {
        self.phase_fingerprint
    }

    /// The semantic revision of the rules.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl std::fmt::Display for RulesVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rules revision {} (phases {:#018x})",
            self.revision, self.phase_fingerprint
        )
    }
}

/// The fingerprint of a phase sequence: FNV-1a 64 over the length-
/// prefixed phase names, in order.
fn phase_fingerprint(phases: &[Phase]) -> u64 {
    let mut input = Vec::new();
    for phase in phases {
        input.extend_from_slice(&(phase.name.len() as u64).to_le_bytes());
        input.extend_from_slice(phase.name.as_bytes());
    }
    fnv1a64(&input)
}

/// The rules of a simulation: the game's side of the step contract.
///
/// The framework executes the declared phases in order and hands each
/// one its own RNG stream; the rules own the state transitions, the
/// command semantics, and the events.
pub trait Rules {
    /// The simulation state the phases mutate. Canonically
    /// serializable ([`CanonicalState`]): every Keldysh state is
    /// checkpoint-hashable by construction (ADR-0002 §5.5); the game
    /// computes the hash at its checkpoint steps.
    type State: CanonicalState;
    /// A command of the simulation. Comparable by value: the canonical
    /// ordering collapses identical redeliveries and rejects
    /// conflicting ones by comparing payloads.
    type Command: PartialEq;
    /// An event the rules record for the journal and UI.
    type Event;
    /// The error the rules can fail with.
    type Error: std::error::Error;

    /// The declared phase sequence: stable unique names, in execution
    /// order. Part of the rules version identity.
    fn phases(&self) -> &'static [Phase];

    /// The version of the rules declaration; the phase fingerprint must
    /// match [`Rules::phases`].
    fn version(&self) -> RulesVersion;

    /// Applies one phase to the state.
    ///
    /// `rng` is the phase's own stream — derived from the phase name
    /// and the party seed, always at position zero; consuming it in a
    /// phase-determined order is the phase's responsibility.
    fn apply_phase(
        &self,
        phase: &'static Phase,
        state: &mut Self::State,
        commands: &[&Self::Command],
        rng: &mut ChaCha8Rng,
        events: &mut Vec<Self::Event>,
    ) -> Result<(), Self::Error>;
}

/// The result of one executed step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepOutcome<Event> {
    /// The events the phases recorded, in execution order.
    pub events: Vec<Event>,
    /// The rules version the step ran under.
    pub version: RulesVersion,
    /// The executed phase names, in order.
    pub phases: Vec<&'static str>,
}

/// Why a step could not execute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepError<RuleError> {
    /// Two declared phases share a name; per-phase streams and
    /// dispatch require unique names.
    DuplicatePhase { name: &'static str },
    /// The version's phase fingerprint does not match the declared
    /// phases: the phases changed without re-declaring the version.
    VersionMismatch { declared: u64, computed: u64 },
    /// A rule error aborted the step; the phase is named.
    Phase {
        phase: &'static str,
        error: RuleError,
    },
    /// The command envelopes failed their canonical ordering.
    CommandOrder(CommandOrderError),
}

impl<RuleError: std::fmt::Display> std::fmt::Display for StepError<RuleError> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::DuplicatePhase { name } => {
                write!(f, "duplicate phase name {name:?}")
            }
            StepError::VersionMismatch { declared, computed } => write!(
                f,
                "rules version declares phase fingerprint {declared:#018x}, \
                 the declaration computes {computed:#018x}: re-declare the version \
                 (RulesVersion::declaring)"
            ),
            StepError::Phase { phase, error } => {
                write!(f, "phase {phase:?} failed: {error}")
            }
            StepError::CommandOrder(error) => write!(f, "command ordering failed: {error}"),
        }
    }
}

impl<RuleError> std::error::Error for StepError<RuleError>
where
    RuleError: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StepError::Phase { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Executes one step: the declared phases, in order, each with its own
/// RNG stream.
///
/// `commands` must arrive in the canonical order the caller maintains
/// (the command-ordering layer precedes the step). A rule error aborts
/// the step with the phase's name; the state is left as the failed
/// phase's entry — the step does not roll back.
pub fn step<R: Rules>(
    state: &mut R::State,
    current_step: u64,
    envelopes: &[CommandEnvelope<R::Command>],
    rules: &R,
    streams: &RngStreams,
) -> Result<StepOutcome<R::Event>, StepError<R::Error>> {
    // The canonical ordering is the framework's guarantee: the phases
    // always see the payloads in the one true order, whatever the
    // delivery order was.
    let ordered = canonical_order(envelopes, current_step).map_err(StepError::CommandOrder)?;
    let commands: Vec<&R::Command> = ordered.iter().map(|envelope| &envelope.payload).collect();
    let commands = commands.as_slice();
    let phases = rules.phases();
    let mut seen: Vec<&'static str> = Vec::new();
    for phase in phases {
        if seen.contains(&phase.name) {
            return Err(StepError::DuplicatePhase { name: phase.name });
        }
        seen.push(phase.name);
    }
    let version = rules.version();
    let computed = phase_fingerprint(phases);
    if version.phase_fingerprint != computed {
        return Err(StepError::VersionMismatch {
            declared: version.phase_fingerprint,
            computed,
        });
    }

    let mut events = Vec::new();
    let mut executed = Vec::with_capacity(phases.len());
    for phase in phases {
        let mut rng = streams.stream(phase.name);
        rules
            .apply_phase(phase, state, commands, &mut rng, &mut events)
            .map_err(|error| StepError::Phase {
                phase: phase.name,
                error,
            })?;
        executed.push(phase.name);
    }

    Ok(StepOutcome {
        events,
        version,
        phases: executed,
    })
}
