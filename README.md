# Keldysh

Reusable deterministic simulation framework in Rust. The intended scope is step execution, stable ordering, generic commands and events, replay, snapshots, and deterministic test support. Game rules and planetary geography belong to other repositories.

This repository currently contains a bootstrap crate with no public API. See the cross-repository architecture specification in the private `docs` repository for the proposed boundaries. Public API documentation will be added here as the implementation develops.

Run `cargo test` from this directory once a Rust toolchain is installed.
