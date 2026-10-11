# Keldysh

Keldysh is a reusable, deterministic, step-based simulation framework in Rust. It owns the step loop, stable ordering, generic command and event types, replay with snapshots, and deterministic test support. It is game-agnostic: game rules, provinces, factions, planetary geography, networking, and graphics belong to other projects.

Status: pre-release (0.1.0, not published). The step contract, the event journal, replay recording and verification, the snapshot format, and the determinism harness are in place. The framework contract — the invariants, the version model, and what breaks a replay — is documented as a book in `keldysh-core`'s crate documentation (see Documentation).

## Layout

A Cargo workspace. Crates appear when there is real content, never as placeholder APIs.

- `crates/core` (`keldysh-core`) — the deterministic foundation and the step contract: named RNG streams of the `rng-streams-v1` strategy (ChaCha8 generators derived from a party seed and a stream name), the shared FNV-1a 64 hashing primitive, and the `Rules` trait with the `step` engine — the game declares its phases and logic, the framework executes them in the declared order with per-phase RNG streams, collected events, a phase-fingerprinted rules version that cannot drift from the declaration, and the canonical ordering of player commands (envelopes with delivery idempotency: redeliveries collapse, conflicts are errors); every state is canonically hashable (`CanonicalState`), and the event journal keeps the chronological record of step outcomes as pure output.
- `crates/replay` (`keldysh-replay`) — replay recording and verification: the passive recorder the session loop feeds (rules version, party seed, starting state hash, the command journal, checkpoint hashes under an `every(n)` policy) and the verifier that replays the record on a fresh state, rejecting a foreign version or starting state before the first step and reporting the first divergent checkpoint with both hashes; the snapshot is the versioned machine format of the state — the header's rules version and state hash reject foreign rules and corrupted payloads.
- `crates/testkit` (`keldysh-testkit`) — the determinism test harness: `two_runs` replays the party twice in the process, `two_processes` replays it in a fresh process too (self-exec of the test binary in worker mode), catching what in-process tests cannot see — the per-process seeding of `HashMap`'s random state. The comparison reports the first divergent checkpoint with both hashes.

## Documentation

The framework contract is a structured book assembled from `crates/core/docs/` into `keldysh-core`'s crate documentation; the chapters' examples are doctests and their links are checked in CI:

```bash
cargo doc -p keldysh-core --open
```

The chapters: the overview (the crate map and the step contract), the invariants with the game author's discipline, the version model, and what breaks a replay.

## Building and testing

The toolchain is pinned in `rust-toolchain.toml` (1.92.0). Run from the repository root:

```bash
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Every determinism guarantee needs a test: same inputs, two runs, identical results. Cross-implementation golden vectors pin `keldysh-core`'s RNG and hashing primitives byte-for-byte.

## Scope and conventions

A step is a pure function of `(State, sorted Commands, Rules, RNG state)`: wall-clock time, thread scheduling, and map iteration order must never influence results. The full determinism rules (non-negotiable), the dependency policy, and the contribution conventions live in `AGENTS.md`.

## License

Dual-licensed under the MIT license or the Apache License 2.0; see `LICENSE-MIT` and `LICENSE-APACHE`.
