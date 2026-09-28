# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The repository became a Cargo workspace; the bootstrap crate is gone.
- `keldysh-core` (`crates/core`), the deterministic foundation: named RNG streams of the `rng-streams-v1` strategy — ChaCha8 generators derived from a party seed and a stream name (FNV-1a 64 seed derivation over the strategy tag, the name, and the seed), local, re-entrant, and seed-sensitive by construction — plus the shared FNV-1a 64 hashing primitive. Cross-implementation golden vectors pin byte-identical agreement with the reference implementation.
- Bootstrap crate scaffold: package metadata, dual MIT or Apache-2.0 license, and a committed `Cargo.lock` for reproducible builds.
- CI on GitHub Actions: rustfmt, clippy, rustdoc, and tests on a pinned toolchain, plus a guard against references to private repositories.
- `AGENTS.md` describing scope, determinism rules, and dependency policy.
