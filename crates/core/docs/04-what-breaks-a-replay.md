# What breaks a replay

A replay record pins five things: the rules version, the party seed, the
starting state hash, the command journal by step, and the checkpoint hashes
under the record's policy. Verification replays the journal on a fresh starting
state and compares the state hashes at the checkpoints. The starting state
travels outside the record — the caller supplies it: a scenario-built state or
a restored snapshot.

## Caught explicitly, before the first step

- **A foreign rules version** — rejected with both versions. This is the
  designed rejection: the record belongs to another rules build.
- **A foreign starting state** — the given state does not hash to the record's
  starting hash.
- **A foreign snapshot** — a differing magic, an unknown format version, a
  foreign rules version (both listed), a corrupted payload (the header hash),
  trailing bytes, or truncation.

## Caught by verification — but it means the version lied

- **Rule semantics changed without a bump.** The replay runs under the same
  declared version and diverges at the first checkpoint: the report names the
  checkpoint and both hashes. The mechanism worked; the version did not tell
  the truth. The fix is the bump plus a new record.
- **A tampered or corrupted record.** A mutated command diverges at its own
  step under the every-step policy, and at the first checkpoint after the drift
  under a sparser one. A record whose envelopes do not target their journal
  step reports the step failure with the engine's diagnostics instead of
  panicking.

## Not caught on one platform — forbidden by discipline

These produce valid-looking but wrong parties. The cross-process harness
catches the first class; only the cross-platform verification run against the
reference platform catches the second — which is why the discipline forbids
them outright instead of relying on being caught:

- **HashMap iteration inside the state** — the random state of a hash table is
  seeded per process: two processes diverge under identical inputs, while the
  in-process twin runs agree. The cross-process harness exists for exactly
  this class.
- **Floating point, wall-clock time, thread counts, global entropy** —
  platform- and schedule-dependent; same-platform runs agree by accident, the
  cross-platform run does not.

## What does *not* break a replay

- **Delivery order and redeliveries** — the engine canonically orders commands
  by `(target step, player, idempotency identifier)`; identical redeliveries
  collapse; conflicting ones are errors, never a silent "first wins".
- **A new RNG stream name** — stream locality: introducing a name never shifts
  the sequences of existing names.
- **Events** — they are the output of a step and never its input; changing
  what the rules record changes nothing in the state hash.
- **Continuity across a snapshot** — the snapshot restores the state
  byte-identically, and the engine re-derives every phase stream per step, so
  a party continued from a snapshot matches the run that never detoured.

## The bug report

A reproducible report is the record, the starting state, and the versions —
nothing else. On the maintainer's side: restore the state, verify the record,
and read the first divergent checkpoint with both hashes.
