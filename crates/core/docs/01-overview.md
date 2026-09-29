# Keldysh — the deterministic simulation contract

Keldysh is a reusable, deterministic, step-based simulation framework. It owns
the machinery that makes a simulation reproducible; the game built on it owns
everything specific to itself. Nothing in Keldysh knows about provinces, gold,
or days.

The whole framework executes one contract:

```text
(State[n], Commands[n], Rules, RNG[n]) → (State[n+1], Events[n])
```

The result of a step is a pure function of the state, the canonically ordered
commands, the declared rules with their version, and the named RNG streams.
Wall-clock time, thread scheduling, and delivery order never enter the result.
Because the function is pure, a party can be recorded, replayed, snapshotted,
restored, and verified — and two independent processes run the same party into
the same hashes.

## The crates

| Crate | Owns |
| --- | --- |
| `keldysh-core` (this crate) | The step contract: the [`Rules`] trait and the [`step`] engine, named RNG streams of `rng-streams-v1`, the FNV-1a 64 hashing primitive, command envelopes with canonical ordering, the canonical state hash ([`CanonicalState`]), the event journal |
| `keldysh-replay` | Recording and verification of parties (`Recording`, `verify`) and the snapshot format (`snapshot`), keyed by the versions the header carries |
| `keldysh-testkit` | The determinism harness: two runs of a party in the process and across processes |

## Who owns what

The game describes its side of the contract through the [`Rules`] trait:

- the [`Rules::State`], [`Rules::Command`], and [`Rules::Event`] types of its
  simulation and the error its rules can fail with;
- the [`Phase`] sequence — the order the parts of a step run in, part of the
  rules version identity;
- the per-phase logic in [`Rules::apply_phase`];
- the canonical encoding of its state ([`CanonicalState::write_canonical`]).

The framework executes everything that must never depend on the game's
discipline: the phase order with uniqueness and fingerprint validation, the
canonical ordering of command envelopes (identical redeliveries collapse,
conflicts are errors), per-phase RNG streams derived from the phase name, event
collection, and fail-fast errors carrying the failing phase's name.

A step never rolls back: rules validate commands before mutating (the
validation phase precedes the mutation phases in a well-formed declaration),
and a rule error aborts the step naming the phase.

## Quick start: named RNG streams

```text
(RandomState of a party seed + a stream name) → a ChaCha8 generator
```

```rust
use keldysh_core::RngStreams;
use rand_chacha::rand_core::RngCore;

let streams = RngStreams::new(0x1234_5678_9abc_def0);
let mut economy = streams.stream("economy");
let mut bytes = vec![0u8; 16];
economy.fill_bytes(&mut bytes);

// Re-entrant: the same name always yields the stream from position
// zero, whatever was consumed before.
let mut again = streams.stream("economy");
again.fill_bytes(&mut bytes);
```
