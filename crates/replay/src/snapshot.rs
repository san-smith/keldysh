//! The snapshot: the versioned machine format of a simulation state.
//!
//! The snapshot lets a party survive the loss of its state — a
//! continuation from the middle, an offline verification against the
//! record. The layout (v1, little-endian throughout):
//!
//! ```text
//! magic            5 bytes   b"KSNAP"
//! format_version   u32       1 ([`SNAPSHOT_FORMAT`])
//! fingerprint      u64       the rules' phase fingerprint
//! revision         u64       the rules' semantic revision
//! state_hash       u64       the canonical hash of the payload state
//! payload_len      u64       the length of the canonical state bytes
//! payload          bytes     the canonical state encoding
//! ```
//!
//! The header carries the compatibility metadata of the version
//! taxonomy (ADR-0002 §5.6): a foreign rules version is rejected with
//! both versions before the payload is read, and the state hash
//! detects payload corruption — drift detection, not cryptography
//! (§5.5). Migrations between versions are not promised before 1.0.
//!
//! The party seed travels outside the snapshot — the game's save
//! format (E-08+) owns the session; the snapshot is a state
//! container. RNG positions are never stored: the engine re-derives
//! each phase stream per step, and any live RNG state of the game is
//! part of `State` itself (ADR-0002 §5.3, as implemented).
//!
//! # Example
//!
//! ```
//! use std::convert::Infallible;
//!
//! use keldysh_core::{
//!     CanonicalReadError, CanonicalReader, CanonicalState, CommandEnvelope, IdempotencyId, Phase,
//!     PlayerId, RngStreams, Rules, RulesVersion, put_u64, step,
//! };
//! use keldysh_replay::snapshot::{self, SnapshotState};
//!
//! struct Counter {
//!     value: u64,
//! }
//!
//! impl CanonicalState for Counter {
//!     fn write_canonical(&self, out: &mut Vec<u8>) {
//!         put_u64(out, self.value);
//!     }
//! }
//!
//! impl SnapshotState for Counter {
//!     fn from_canonical(reader: &mut CanonicalReader<'_>) -> Result<Self, CanonicalReadError> {
//!         Ok(Self {
//!             value: reader.read_u64()?,
//!         })
//!     }
//! }
//!
//! struct CounterRules;
//!
//! static PHASES: &[Phase] = &[Phase { name: "apply" }];
//!
//! impl Rules for CounterRules {
//!     type State = Counter;
//!     type Command = u64;
//!     type Event = Infallible;
//!     type Error = Infallible;
//!
//!     fn phases(&self) -> &'static [Phase] {
//!         PHASES
//!     }
//!
//!     fn version(&self) -> RulesVersion {
//!         RulesVersion::declaring(1, PHASES)
//!     }
//!
//!     fn apply_phase(
//!         &self,
//!         _phase: &'static Phase,
//!         state: &mut Counter,
//!         commands: &[&u64],
//!         _rng: &mut rand_chacha::ChaCha8Rng,
//!         _events: &mut Vec<Infallible>,
//!     ) -> Result<(), Infallible> {
//!         for amount in commands {
//!             state.value += **amount;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let rules = CounterRules;
//! let mut state = Counter { value: 41 };
//! let envelopes = [CommandEnvelope {
//!     player: PlayerId(1),
//!     target_step: 0,
//!     idempotency: IdempotencyId(1),
//!     payload: 1,
//! }];
//! step(&mut state, 0, &envelopes, &rules, &RngStreams::new(7)).unwrap();
//!
//! let bytes = snapshot::save(&state, &rules);
//! let restored = snapshot::restore(&bytes, &rules).unwrap();
//! assert_eq!(restored.state_hash(), state.state_hash());
//! ```

use std::fmt;

use keldysh_core::{CanonicalReadError, CanonicalReader, CanonicalState, Rules, RulesVersion};

/// The format version this module reads: a snapshot from another
/// version is an explicit rejection, not a best effort.
pub const SNAPSHOT_FORMAT: u32 = 1;

const MAGIC: &[u8; 5] = b"KSNAP";

/// A state that can read itself back from its canonical encoding.
///
/// The write side is [`CanonicalState`]; the pair restores a state
/// from the same encoding the hash pins. The implementation must
/// consume the reader exactly — the snapshot verifies the full
/// consumption and the hash.
pub trait SnapshotState: CanonicalState + Sized {
    /// Reads the state from its canonical encoding, mirroring the
    /// state's [`CanonicalState::write_canonical`].
    fn from_canonical(reader: &mut CanonicalReader<'_>) -> Result<Self, CanonicalReadError>;
}

/// Why a snapshot could not be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// The bytes do not start with the snapshot magic.
    ForeignMagic,
    /// The format version is not the one this module reads.
    UnknownFormatVersion {
        /// The version found in the header.
        found: u32,
        /// The version this module reads.
        supported: u32,
    },
    /// The snapshot was taken under different rules; migration is not
    /// promised (ADR-0002 §5.6).
    RulesVersionMismatch {
        /// The version recorded in the header.
        recorded: RulesVersion,
        /// The version of the rules under restoration.
        actual: RulesVersion,
    },
    /// The restored state does not hash to the header's hash: the
    /// payload is corrupted.
    HashMismatch {
        /// The hash recorded in the header.
        recorded: u64,
        /// The hash of the restored state.
        actual: u64,
    },
    /// The bytes are truncated or the payload does not decode.
    Malformed {
        /// The reading error with its offset.
        source: CanonicalReadError,
    },
    /// The snapshot has bytes left after the declared payload: the
    /// file is longer than its header and payload.
    TrailingBytes {
        /// The number of unconsumed bytes.
        remaining: usize,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::ForeignMagic => write!(f, "not a Keldysh snapshot: the magic differs"),
            SnapshotError::UnknownFormatVersion { found, supported } => write!(
                f,
                "snapshot format version {found} is not supported: this reader reads {supported}"
            ),
            SnapshotError::RulesVersionMismatch { recorded, actual } => write!(
                f,
                "snapshot rules version mismatch — recorded {recorded}, restoring under {actual}"
            ),
            SnapshotError::HashMismatch { recorded, actual } => write!(
                f,
                "snapshot payload is corrupted — header hash {recorded:#018x}, \
                 restored state hashes {actual:#018x}"
            ),
            SnapshotError::Malformed { source } => write!(f, "snapshot is malformed: {source}"),
            SnapshotError::TrailingBytes { remaining } => {
                write!(f, "snapshot payload ends with {remaining} unconsumed bytes")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Saves the state under its rules: the snapshot bytes.
pub fn save<R: Rules>(state: &R::State, rules: &R) -> Vec<u8> {
    let version = rules.version();
    let payload = state.canonical_bytes();
    let mut out = Vec::with_capacity(MAGIC.len() + 4 + 8 * 4 + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SNAPSHOT_FORMAT.to_le_bytes());
    out.extend_from_slice(&version.phase_fingerprint().to_le_bytes());
    out.extend_from_slice(&version.revision().to_le_bytes());
    out.extend_from_slice(&state.state_hash().to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Restores a state from the snapshot bytes: validates the magic, the
/// format version, and the rules version before reading the payload,
/// then verifies the restored state against the header's hash.
pub fn restore<R: Rules>(bytes: &[u8], rules: &R) -> Result<R::State, SnapshotError>
where
    R::State: SnapshotState,
{
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SnapshotError::ForeignMagic);
    }
    let malformed = |source: CanonicalReadError| SnapshotError::Malformed { source };
    let mut reader = CanonicalReader::new(bytes);
    reader.read_bytes(MAGIC.len()).map_err(malformed)?;

    let found = u32::from_le_bytes(
        reader
            .read_bytes(4)
            .map_err(malformed)?
            .try_into()
            .expect("four bytes"),
    );
    if found != SNAPSHOT_FORMAT {
        return Err(SnapshotError::UnknownFormatVersion {
            found,
            supported: SNAPSHOT_FORMAT,
        });
    }

    let fingerprint = reader.read_u64().map_err(malformed)?;
    let revision = reader.read_u64().map_err(malformed)?;
    let recorded_version = RulesVersion::from_parts(fingerprint, revision);
    let actual_version = rules.version();
    if recorded_version != actual_version {
        return Err(SnapshotError::RulesVersionMismatch {
            recorded: recorded_version,
            actual: actual_version,
        });
    }

    let recorded_hash = reader.read_u64().map_err(malformed)?;
    let offset = reader.offset();
    let payload_len = reader.read_u64().map_err(malformed)?;
    let payload_len = match usize::try_from(payload_len) {
        Ok(len) => len,
        Err(_) => {
            return Err(SnapshotError::Malformed {
                source: CanonicalReadError::LengthTooLarge {
                    offset,
                    length: payload_len,
                },
            });
        }
    };
    let payload = reader.read_bytes(payload_len).map_err(malformed)?;

    // The file is exactly the header plus the payload: anything after
    // it is not a snapshot this format defines.
    if !reader.is_empty() {
        return Err(SnapshotError::TrailingBytes {
            remaining: bytes.len() - reader.offset(),
        });
    }

    let mut payload_reader = CanonicalReader::new(payload);
    let state = R::State::from_canonical(&mut payload_reader).map_err(malformed)?;
    if !payload_reader.is_empty() {
        return Err(SnapshotError::TrailingBytes {
            remaining: payload.len() - payload_reader.offset(),
        });
    }

    let actual_hash = state.state_hash();
    if actual_hash != recorded_hash {
        return Err(SnapshotError::HashMismatch {
            recorded: recorded_hash,
            actual: actual_hash,
        });
    }
    Ok(state)
}
