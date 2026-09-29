# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The canonical state hash (ADR-0002 §5.5): the `CanonicalState` trait the game implements to write its state's canonical encoding — fixed-width little-endian integers, length-prefixed byte strings, sequence counts, and sorted traversal of map-like collections — with `canonical_bytes` and the FNV-1a 64 `state_hash` derived by the crate. `Rules::State` carries the `CanonicalState` bound, so every Keldysh state is checkpoint-hashable by construction; the consumer computes the hash at the checkpoint steps it chooses, `step` does not serialize.
- Command envelopes and their canonical ordering (ADR-0002 §5.4): `CommandEnvelope` with typed `PlayerId` and `IdempotencyId` (reserved zero), canonicalized inside `step` by the key `(target step, player, idempotency identifier)` — identical redeliveries collapse, conflicting payloads and wrong target steps are errors with coordinates, and the phases always see the payloads in the one true order. `Rules::Command` is comparable by value for the same purpose.
- The step contract: the `Rules` trait (associated State/Command/Event/Error, the declared `Phase` sequence, per-phase application with a dedicated RNG stream) and the `step` engine executing the phases in the declared order with events collected. `RulesVersion` carries the phase-sequence fingerprint the engine verifies — changing the phases without re-declaring the version is rejected by a test. Rule errors abort the step with the phase's name (no rollback: rules validate before mutating). Events are returned per step; the journal and replay verification build on this contract.
- The repository became a Cargo workspace; the bootstrap crate is gone.
- `keldysh-core` (`crates/core`), the deterministic foundation: named RNG streams of the `rng-streams-v1` strategy — ChaCha8 generators derived from a party seed and a stream name (FNV-1a 64 seed derivation over the strategy tag, the name, and the seed), local, re-entrant, and seed-sensitive by construction — plus the shared FNV-1a 64 hashing primitive. Cross-implementation golden vectors pin byte-identical agreement with the reference implementation.
- Bootstrap crate scaffold: package metadata, dual MIT or Apache-2.0 license, and a committed `Cargo.lock` for reproducible builds.
- CI on GitHub Actions: rustfmt, clippy, rustdoc, and tests on a pinned toolchain, plus a guard against references to private repositories.
- `AGENTS.md` describing scope, determinism rules, and dependency policy.
