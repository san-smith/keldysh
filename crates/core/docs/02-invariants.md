# The invariants and the author's discipline

Each invariant below is part of the framework's contract; the column on the
right names the types that enforce it and the tests that pin it. Rules that
violate the discipline must not be merged even if all tests pass — the
discipline is the contract, not a style guide.

| Invariant | Pinned by |
| --- | --- |
| A step is a pure function of `(State, sorted Commands, Rules, RNG)` | [`step`] runs the declared phases over the canonically ordered envelopes; the cross-process harness proves it |
| Randomness comes only through named streams | [`RngStreams`] — obtaining a generator without the party seed is impossible by types; every step re-derives each phase stream at position zero |
| No platform-dependent floating point in state or logic | The state discipline: values live on the integer lattices the game declares, arithmetic is checked and integer-only |
| Every state is canonically serializable and hashable | [`CanonicalState`] — the [`Rules::State`] bound makes every Keldysh state checkpoint-hashable by construction |
| Events are a projection, never an input | [`EventJournal`] and [`step`] — the engine returns events and never reads the journal; the state is the source of truth |
| Arbitrary map orders never leak into results | The traversal discipline below; the cross-process harness catches a violation |

## The author's discipline

When implementing [`Rules`]:

1. **Validate before mutating.** A step does not roll back: an error aborts the
   step at the failing phase and the state stays as the phase left it. The
   validation phase precedes the mutation phases, and a phase that can fail
   must not mutate until everything is known to succeed.
2. **Keep the state integer-only.** No platform-dependent floating point, no
   wall-clock time, no thread counts, no global entropy — none of it can be
   part of a pure function, and none of it survives the cross-platform
   verification run.
3. **Traverse deterministically.** [`BTreeMap`](std::collections::BTreeMap),
   sorted vectors, or stable explicit identifiers — never an iteration order
   that the hash table chooses.
4. **Draw randomness only from the phase stream.** The stream the engine hands
   to [`Rules::apply_phase`] is derived from the phase name and the party seed,
   always at position zero; consuming it in a phase-determined order is the
   phase's responsibility.
5. **Treat events as output.** Events never affect the state transition; they
   exist for the journal and the UI.
6. **Keep commands comparable values.** [`Rules::Command`] is compared by value
   to collapse identical redeliveries and reject conflicting ones — an
   idempotency identifier is never reused for a different payload, and `0` is
   the reserved "unset" of [`PlayerId`] and [`IdempotencyId`].

### The discipline, executable

A two-phase treasury: "validate" only checks, "apply" only mutates — an
overdraft aborts the step before anything moved. The balances live in a
[`BTreeMap`](std::collections::BTreeMap), so the canonical traversal is the
natural one.

```rust
use std::collections::BTreeMap;
use std::convert::Infallible;

use keldysh_core::{
    CanonicalState, CommandEnvelope, IdempotencyId, Phase, PlayerId, RngStreams, Rules,
    RulesVersion, put_seq_len, put_u64, step,
};

struct Treasury {
    balances: BTreeMap<u64, u64>,
}

impl CanonicalState for Treasury {
    fn write_canonical(&self, out: &mut Vec<u8>) {
        put_seq_len(out, self.balances.len());
        for (player, balance) in &self.balances {
            put_u64(out, *player);
            put_u64(out, *balance);
        }
    }
}

#[derive(Debug)]
struct Overdraft(u64);

impl std::fmt::Display for Overdraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "withdrawal of {} exceeds the balance or the limit", self.0)
    }
}

impl std::error::Error for Overdraft {}

struct TreasuryRules {
    withdrawal_limit: u64,
}

static PHASES: &[Phase] = &[Phase { name: "validate" }, Phase { name: "apply" }];

impl Rules for TreasuryRules {
    type State = Treasury;
    type Command = (u64, u64); // (player, amount)
    type Event = Infallible;
    type Error = Overdraft;

    fn phases(&self) -> &'static [Phase] {
        PHASES
    }

    fn version(&self) -> RulesVersion {
        RulesVersion::declaring(1, PHASES)
    }

    fn apply_phase(
        &self,
        phase: &'static Phase,
        state: &mut Treasury,
        commands: &[&(u64, u64)],
        _rng: &mut rand_chacha::ChaCha8Rng,
        _events: &mut Vec<Infallible>,
    ) -> Result<(), Overdraft> {
        match phase.name {
            "validate" => {
                for command in commands {
                    let (player, amount) = **command;
                    let balance = state.balances.get(&player).copied().unwrap_or(0);
                    if amount > balance || amount > self.withdrawal_limit {
                        return Err(Overdraft(amount));
                    }
                }
                Ok(())
            }
            "apply" => {
                for command in commands {
                    let (player, amount) = **command;
                    *state
                        .balances
                        .get_mut(&player)
                        .expect("validated in the previous phase") -= amount;
                }
                Ok(())
            }
            other => unreachable!("the engine executes only declared phases: {other:?}"),
        }
    }
}

let rules = TreasuryRules { withdrawal_limit: 50 };
let mut treasury = Treasury { balances: BTreeMap::new() };
treasury.balances.insert(7, 10);

let envelopes = [CommandEnvelope {
    player: PlayerId(1),
    target_step: 0,
    idempotency: IdempotencyId(1),
    payload: (7, 100),
}];

let error = step(&mut treasury, 0, &envelopes, &rules, &RngStreams::new(1)).unwrap_err();
assert!(
    matches!(error, keldysh_core::StepError::Phase { phase: "validate", .. }),
    "the overdraft is caught by the validation phase"
);
assert_eq!(treasury.balances[&7], 10, "nothing moved: the state is untouched");
```
