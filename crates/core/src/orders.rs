//! Command envelopes and their canonical ordering.
//!
//! A multi-player simulation receives commands from many sources in
//! arbitrary order; the deterministic result requires a canonical
//! order that never depends on the arrival order. The envelope carries
//! the machine input of a command — the player, the target step, the
//! idempotency identifier, and the payload; [`canonical_order`]
//! produces the one true order and resolves the delivery conflicts:
//!
//! - commands for steps other than the current one are rejected (the
//!   session owns the queue of future steps);
//! - identical redeliveries — the same player, step, idempotency
//!   identifier, and payload — collapse into one command (redelivery
//!   is safe);
//! - the same player and idempotency identifier with a conflicting
//!   payload is an error, never a silent "first wins" or "last wins".
//!
//! Identifier discipline: `0` is reserved as "unset" and rejected.

use std::fmt;

/// The identifier of a player (a human seat or an AI) submitting
/// commands. `0` is reserved as "unset".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u64);

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p:{}", self.0)
    }
}

impl PlayerId {
    /// The reserved "unset" value; never assigned to a player.
    pub const RESERVED: u64 = 0;

    /// Whether this identifier holds the reserved unset value.
    pub fn is_reserved(self) -> bool {
        self.0 == Self::RESERVED
    }
}

/// The idempotency identifier of one command delivery: the same value
/// redelivered with the same payload is a duplicate, not a second
/// command. `0` is reserved as "unset".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyId(pub u64);

impl fmt::Display for IdempotencyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "i:{}", self.0)
    }
}

impl IdempotencyId {
    /// The reserved "unset" value; never assigned to a delivery.
    pub const RESERVED: u64 = 0;

    /// Whether this identifier holds the reserved unset value.
    pub fn is_reserved(self) -> bool {
        self.0 == Self::RESERVED
    }
}

/// A command wrapped in its delivery metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandEnvelope<C> {
    /// The player that submitted the command.
    pub player: PlayerId,
    /// The step the command targets.
    pub target_step: u64,
    /// The idempotency identifier of the delivery.
    pub idempotency: IdempotencyId,
    /// The command payload.
    pub payload: C,
}

/// Why the commands could not be canonically ordered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandOrderError {
    /// A required identifier holds the reserved `0` value.
    ReservedIdentifier {
        /// The offending field.
        field: &'static str,
        /// The position of the offending envelope.
        index: usize,
    },
    /// An envelope targets a step other than the one being executed.
    WrongTargetStep {
        /// The position of the offending envelope.
        index: usize,
        /// The step being executed.
        expected: u64,
        /// The envelope's target step.
        actual: u64,
    },
    /// The same player and idempotency identifier delivered two
    /// different payloads.
    ConflictingIdempotency {
        /// The conflicting player.
        player: PlayerId,
        /// The conflicting idempotency identifier.
        idempotency: IdempotencyId,
        /// The positions of the conflicting envelopes.
        envelopes: [usize; 2],
    },
}

impl fmt::Display for CommandOrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandOrderError::ReservedIdentifier { field, index } => {
                write!(f, "envelope {index}: reserved identifier 0 in {field}")
            }
            CommandOrderError::WrongTargetStep {
                index,
                expected,
                actual,
            } => write!(
                f,
                "envelope {index} targets step {actual}, but step {expected} is being executed"
            ),
            CommandOrderError::ConflictingIdempotency {
                player,
                idempotency,
                envelopes,
            } => write!(
                f,
                "player {player} reused idempotency identifier {idempotency} \
                 for two different payloads (envelopes {} and {})",
                envelopes[0], envelopes[1]
            ),
        }
    }
}

impl std::error::Error for CommandOrderError {}

/// Orders the envelopes for the step being executed: validates the
/// targets and the identifiers, sorts by the canonical key
/// `(target step, player, idempotency)`, and collapses identical
/// redeliveries. The returned references point into the input slice,
/// in the canonical order, without duplicates.
pub fn canonical_order<C: PartialEq>(
    envelopes: &[CommandEnvelope<C>],
    current_step: u64,
) -> Result<Vec<&CommandEnvelope<C>>, CommandOrderError> {
    for (index, envelope) in envelopes.iter().enumerate() {
        if envelope.player.is_reserved() {
            return Err(CommandOrderError::ReservedIdentifier {
                field: "player",
                index,
            });
        }
        if envelope.idempotency.is_reserved() {
            return Err(CommandOrderError::ReservedIdentifier {
                field: "idempotency",
                index,
            });
        }
        if envelope.target_step != current_step {
            return Err(CommandOrderError::WrongTargetStep {
                index,
                expected: current_step,
                actual: envelope.target_step,
            });
        }
    }

    let mut order: Vec<usize> = (0..envelopes.len()).collect();
    order.sort_by_key(|&index| {
        let envelope = &envelopes[index];
        (
            envelope.target_step,
            envelope.player.0,
            envelope.idempotency.0,
        )
    });

    let mut result = Vec::with_capacity(order.len());
    let mut group_start = 0usize;
    while group_start < order.len() {
        let head = order[group_start];
        let key = (envelopes[head].player.0, envelopes[head].idempotency.0);
        let mut group_end = group_start + 1;
        while group_end < order.len() {
            let next = order[group_end];
            if (envelopes[next].player.0, envelopes[next].idempotency.0) != key {
                break;
            }
            if envelopes[next].payload != envelopes[head].payload {
                return Err(CommandOrderError::ConflictingIdempotency {
                    player: envelopes[head].player,
                    idempotency: envelopes[head].idempotency,
                    envelopes: [order[group_start], next],
                });
            }
            group_end += 1;
        }
        result.push(&envelopes[head]);
        group_start = group_end;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(player: u64, idempotency: u64, payload: &str) -> CommandEnvelope<&str> {
        CommandEnvelope {
            player: PlayerId(player),
            target_step: 5,
            idempotency: IdempotencyId(idempotency),
            payload,
        }
    }

    #[test]
    fn orders_by_the_canonical_key() {
        let envelopes = [
            envelope(3, 20, "third"),
            envelope(1, 30, "second"),
            envelope(1, 10, "first"),
        ];
        let ordered = canonical_order(&envelopes, 5).unwrap();
        let payloads: Vec<&str> = ordered.iter().map(|e| e.payload).collect();
        assert_eq!(payloads, vec!["first", "second", "third"]);
    }

    #[test]
    fn duplicates_collapse_and_conflicts_are_rejected() {
        let identical = [envelope(1, 10, "same"), envelope(1, 10, "same")];
        let ordered = canonical_order(&identical, 5).unwrap();
        assert_eq!(ordered.len(), 1, "identical redelivery collapses");

        let conflicting = [envelope(1, 10, "same"), envelope(1, 10, "other")];
        assert_eq!(
            canonical_order(&conflicting, 5).unwrap_err(),
            CommandOrderError::ConflictingIdempotency {
                player: PlayerId(1),
                idempotency: IdempotencyId(10),
                envelopes: [0, 1],
            }
        );
    }

    #[test]
    fn wrong_targets_and_reserved_identifiers_are_rejected() {
        let envelopes = [envelope(1, 10, "now")];
        assert_eq!(
            canonical_order(&envelopes, 6).unwrap_err(),
            CommandOrderError::WrongTargetStep {
                index: 0,
                expected: 6,
                actual: 5,
            }
        );

        let reserved_player = [CommandEnvelope {
            player: PlayerId(0),
            target_step: 5,
            idempotency: IdempotencyId(10),
            payload: "x",
        }];
        assert_eq!(
            canonical_order(&reserved_player, 5).unwrap_err(),
            CommandOrderError::ReservedIdentifier {
                field: "player",
                index: 0
            }
        );
    }
}
