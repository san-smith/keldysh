//! The passive recorder of a party and the record it produces.
//!
//! The recorder is a sink the session loop feeds: no second execution
//! path exists, so a recording cannot diverge from the party it
//! observes. The starting state travels outside the record — the
//! caller supplies it — and only its canonical hash is pinned. RNG
//! positions are never recorded: replay re-derives the streams from
//! the party seed (ADR-0002 §5.3).

use keldysh_core::{CanonicalState, CommandEnvelope, Rules, RulesVersion};

/// The checkpoint policy: a state hash is recorded every `n`-th step.
///
/// Step numbers are 0-based: [`Checkpoints::every(1)`](Checkpoints::every)
/// checkpoints every step (the default of the test suites), `every(7)`
/// checkpoints steps 0, 7, 14, …. The policy is part of the record —
/// the verifier reproduces the same points. The divergence granularity
/// is the checkpoint: a drift inside an interval surfaces at the first
/// checkpoint after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoints {
    every: u64,
}

impl Checkpoints {
    /// Checkpoints every `n`-th step; `n` is clamped to at least 1.
    pub fn every(n: u64) -> Self {
        Self { every: n.max(1) }
    }

    /// The interval of the policy.
    pub fn interval(&self) -> u64 {
        self.every
    }

    /// Whether `step` carries a checkpoint under this policy.
    pub fn is_checkpoint(&self, step: u64) -> bool {
        step.is_multiple_of(self.every)
    }
}

/// One recorded step of the command journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StepCommands<C> {
    step: u64,
    envelopes: Vec<CommandEnvelope<C>>,
}

impl<C> StepCommands<C> {
    pub(crate) fn step(&self) -> u64 {
        self.step
    }

    pub(crate) fn envelopes(&self) -> &[CommandEnvelope<C>] {
        &self.envelopes
    }
}

/// The replay record: everything a party's verification needs apart
/// from the starting state itself.
///
/// The record is generic over the command type, not over the rules:
/// verifying a record against *different* rules — the version drift
/// the verifier exists to catch — must type-check and fail at the
/// version check, not at the call site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRecord<C> {
    version: RulesVersion,
    party_seed: u64,
    initial_hash: u64,
    checkpoints: Checkpoints,
    commands: Vec<StepCommands<C>>,
    checkpoint_hashes: Vec<(u64, u64)>,
}

impl<C> ReplayRecord<C> {
    /// The rules version captured at the recording start; a foreign
    /// version rejects the replay before any step executes.
    pub fn version(&self) -> RulesVersion {
        self.version
    }

    /// The party seed; the replay derives the RNG streams from it.
    pub fn party_seed(&self) -> u64 {
        self.party_seed
    }

    /// The canonical hash of the starting state.
    pub fn initial_hash(&self) -> u64 {
        self.initial_hash
    }

    /// The checkpoint policy of the recording.
    pub fn checkpoints(&self) -> Checkpoints {
        self.checkpoints
    }

    /// The recorded checkpoints: `(step, state hash)` pairs in order.
    pub fn checkpoint_hashes(&self) -> &[(u64, u64)] {
        &self.checkpoint_hashes
    }

    /// The number of recorded command steps.
    pub fn step_count(&self) -> usize {
        self.commands.len()
    }

    pub(crate) fn steps(&self) -> &[StepCommands<C>] {
        &self.commands
    }
}

/// The passive recorder of a party: the session loop runs
/// [`step`](keldysh_core::step) and feeds the recorder — the pattern
/// is [`Recording::start`] before the first step, then per step
/// [`Recording::record_commands`] with the delivered envelopes,
/// the step itself, and [`Recording::record_checkpoint`] with the
/// post-step state; [`Recording::finish`] closes the record.
pub struct Recording<R: Rules> {
    version: RulesVersion,
    party_seed: u64,
    initial_hash: u64,
    checkpoints: Checkpoints,
    commands: Vec<StepCommands<R::Command>>,
    checkpoint_hashes: Vec<(u64, u64)>,
}

impl<R: Rules> Recording<R> {
    /// Starts the recording of a party: captures the rules version,
    /// the party seed, and the starting state's canonical hash.
    pub fn start(rules: &R, party_seed: u64, initial: &R::State) -> Self {
        Self {
            version: rules.version(),
            party_seed,
            initial_hash: initial.state_hash(),
            checkpoints: Checkpoints::every(1),
            commands: Vec::new(),
            checkpoint_hashes: Vec::new(),
        }
    }

    /// Sets the checkpoint policy; the default is every step.
    pub fn with_checkpoints(mut self, checkpoints: Checkpoints) -> Self {
        self.checkpoints = checkpoints;
        self
    }

    /// Records the command envelopes the session delivers for `step`.
    pub fn record_commands(&mut self, step: u64, envelopes: &[CommandEnvelope<R::Command>])
    where
        R::Command: Clone,
    {
        self.commands.push(StepCommands {
            step,
            envelopes: envelopes.to_vec(),
        });
    }

    /// Records the state hash after `step` ran — stored only when
    /// `step` carries a checkpoint under the policy.
    pub fn record_checkpoint(&mut self, step: u64, state: &R::State) {
        if self.checkpoints.is_checkpoint(step) {
            self.checkpoint_hashes.push((step, state.state_hash()));
        }
    }

    /// Closes the recording.
    pub fn finish(self) -> ReplayRecord<R::Command> {
        ReplayRecord {
            version: self.version,
            party_seed: self.party_seed,
            initial_hash: self.initial_hash,
            checkpoints: self.checkpoints,
            commands: self.commands,
            checkpoint_hashes: self.checkpoint_hashes,
        }
    }
}
