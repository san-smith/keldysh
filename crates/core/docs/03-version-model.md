# The version model

Every artifact whose change can invalidate past results carries a version, and
compatibility is strict: matching versions reproduce byte-for-byte; mismatched
versions are an explicit rejection listing what did not match. Migrations are
not promised before 1.0.

## The rules version

[`RulesVersion`] is the phase fingerprint — FNV-1a 64 over the declared phase
names, in order — plus the semantic revision. The engine recomputes the
fingerprint at every step and rejects a version that does not match the
declaration: the phase sequence cannot change without the version changing.

Build versions with [`RulesVersion::declaring`]; reassemble them from their
serialized parts — as replay records and snapshots carry them — with
[`RulesVersion::from_parts`]:

```rust
use keldysh_core::{Phase, RulesVersion};

let phases = [Phase { name: "validate" }, Phase { name: "apply" }];
let declared = RulesVersion::declaring(1, &phases);

let restored = RulesVersion::from_parts(declared.phase_fingerprint(), declared.revision());
assert_eq!(declared, restored);
```

The bump rule: **any change that can affect past results bumps the version.**
Changing the phase sequence changes the fingerprint (declare again); changing
the semantics of an existing phase bumps the revision. A semantic change
without a bump still runs — and the first replay verification diverges; the
version contract is what turns that divergence from a mystery into a diagnosis.

## The taxonomy

| Version | Carried by | Bumped when |
| --- | --- | --- |
| rules | replay records, snapshots | the phase sequence or the semantics change |
| content set | the game's save format | the compiled content set changes |
| scenario schema | the game's scenarios | the scenario envelope changes |
| save format | the game's save format | the save container changes |
| protocol | the network layer, once it exists | the wire format changes |

A replay record carries the rules version, the party seed, the starting state
hash, the command journal by step, and the checkpoint hashes. A snapshot
carries the format version, the rules version, the canonical state hash, and
the state bytes. The game's save format — built on snapshots — adds the
session level: the party seed and the game's own versions.

## The encoding strategies

The byte-level disciplines are pinned by tags, and changing a tag is a
compatibility break by definition:

- `rng-streams-v1` ([`RNG_STRATEGY`]) — how a party seed and a stream name
  derive a ChaCha8 generator;
- the canonical state encoding — the little-endian, length-prefixed discipline
  of the [`put_u64`] helpers and [`CanonicalReader`]; the canonical hash is
  FNV-1a 64 over it;
- the snapshot format v1 — the header and the payload layout of
  `keldysh-replay`'s snapshot module.

Changing any of these changes every hash and every saved byte: it is a
deliberate regeneration — golden tests pin the values — never a silent edit.
