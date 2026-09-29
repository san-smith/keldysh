#![doc = include_str!("../docs/01-overview.md")]
#![doc = include_str!("../docs/02-invariants.md")]
#![doc = include_str!("../docs/03-version-model.md")]
#![doc = include_str!("../docs/04-what-breaks-a-replay.md")]

mod fnv;
mod journal;
mod orders;
mod rng;
mod state;
mod step;

pub use fnv::fnv1a64;
pub use journal::{EventJournal, JournalError, StepEvents};
pub use orders::{CommandEnvelope, CommandOrderError, IdempotencyId, PlayerId, canonical_order};
pub use rng::{RNG_STRATEGY, RngStreams};
pub use state::{
    CanonicalReadError, CanonicalReader, CanonicalState, put_i64, put_len_prefixed, put_seq_len,
    put_u64,
};
pub use step::{Phase, Rules, RulesVersion, StepError, StepOutcome, step};
