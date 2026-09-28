# AGENTS.md — Keldysh

Keldysh is a reusable, deterministic, step-based simulation framework in Rust. It owns the step loop, stable ordering, generic command/event types, replay and snapshots, and deterministic test support. It is game-agnostic: no provinces, factions, geography, or domain rules of any specific project, and no networking or graphical stack.

## Status

A Cargo workspace with the first real crate: `crates/core` (`keldysh-core`) hosts the deterministic foundation — named RNG streams of the `rng-streams-v1` strategy (ADR-0005) and the shared FNV-1a 64 hashing primitive. The step contract, command ordering, event journal, and canonical state hash build on this foundation in the following stories; `crates/{replay,testkit}` are extracted when their content arrives (E-03). Keep the root buildable and testable at every commit.

## Commands

Run from the repository root:

```bash
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

## Determinism rules (non-negotiable)

These apply to anything that becomes part of the simulation core:

- A step is a pure function of `(State, sorted Commands, Rules, RNG state)`. Wall-clock time, thread scheduling, and network ordering must never influence results.
- Never iterate a `HashMap`/`HashSet` in a way that affects simulation outcomes. Use `BTreeMap`, sorted vectors, or stable explicit IDs.
- No platform-dependent floating point in simulation state or logic. Prefer integers or fixed point; when arithmetic is introduced, specify rounding and overflow behavior per quantity type.
- Randomness goes through explicit, seeded, stream-partitioned RNG — never through global or external entropy.
- Parallel computation is allowed only with deterministic collection of results.

These rules are part of the framework's contract; code or tests that violate them must not be merged even if all tests pass.

## Scope and dependencies

- This repository builds and tests standalone. Do not add dependencies on sibling checkouts; external dependencies come from crates.io or pinned Git revisions only, and no committed `[patch]` sections.
- Licenses: dual `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`). Keep `publish = false` until publication is explicitly decided.
- Commit `Cargo.lock` and update it together with dependency changes; it does not constrain library consumers but keeps CI builds reproducible.
- Game mechanics, map data, transports, and UI belong to other projects. If a feature needs domain concepts, the boundary is wrong — stop and redesign at the abstraction level.

## Conventions

- Contributions and issue discussion in English.
- Public API gets rustdoc comments with examples before stabilization; undocumented public items are a defect.
- Every determinism guarantee needs a test: same inputs, two runs, identical results.
