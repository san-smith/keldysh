# Keldysh

Reusable deterministic simulation framework in Rust. The intended scope is step execution, stable ordering, generic commands and events, replay, snapshots, and deterministic test support. Game rules and planetary geography belong to other repositories.

## Layout

A Cargo workspace. Crates appear when there is real content, never as placeholder APIs.

- `crates/core` (`keldysh-core`) — the deterministic foundation and the step contract: named RNG streams of the `rng-streams-v1` strategy (ChaCha8 generators derived from a party seed and a stream name), the shared FNV-1a 64 hashing primitive, and the `Rules` trait with the `step` engine — the game declares its phases and logic, the framework executes them in the declared order with per-phase RNG streams, collected events, a phase-fingerprinted rules version that cannot drift from the declaration, and the canonical ordering of player commands (envelopes with delivery idempotency: redeliveries collapse, conflicts are errors); every state is canonically hashable (`CanonicalState`), and the event journal keeps the chronological record of step outcomes as pure output.
- `crates/replay` (`keldysh-replay`) — replay recording and verification: the passive recorder the session loop feeds (rules version, party seed, starting state hash, the command journal, checkpoint hashes under an `every(n)` policy) and the verifier that replays the record on a fresh state, rejecting a foreign version or starting state before the first step and reporting the first divergent checkpoint with both hashes; the snapshot is the versioned machine format of the state — the header's rules version and state hash reject foreign rules and corrupted payloads.

The deterministic test harness is extracted when its content arrives.

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
