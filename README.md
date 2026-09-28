# Keldysh

Reusable deterministic simulation framework in Rust. The intended scope is step execution, stable ordering, generic commands and events, replay, snapshots, and deterministic test support. Game rules and planetary geography belong to other repositories.

## Layout

A Cargo workspace. Crates appear when there is real content, never as placeholder APIs.

- `crates/core` (`keldysh-core`) — the deterministic foundation: named RNG streams of the `rng-streams-v1` strategy (ADR-0005 in the project documentation — ChaCha8 generators derived from a party seed and a stream name, locality and re-entrancy by construction) and the shared integer hashing primitive (FNV-1a 64). The game-facing step contract builds on these primitives.

Replay, snapshots, and the deterministic test harness are extracted when their content arrives.

## Commands

Run from the repository root:

```bash
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

## License

Dual-licensed under the MIT license or the Apache License 2.0; see `LICENSE-MIT` and `LICENSE-APACHE`.
