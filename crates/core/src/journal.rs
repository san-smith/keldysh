//! The event journal: the chronological record of step outcomes.
//!
//! The journal is pure output. The [`step`](crate::step) engine
//! returns each step's events in [`StepOutcome`](crate::StepOutcome)
//! and never reads the journal; the session loop moves the outcome
//! into an [`EventJournal`] with [`EventJournal::record`]. The state
//! stays the source of truth (ADR-0002 §2): the journal is a
//! projection for the UI, replay verification, and diagnostics — not
//! a rollback mechanism and not a second source of truth. Events
//! never influence the step result (ADR-0002 §5.1).
//!
//! # Chronology as an invariant
//!
//! [`EventJournal::record`] accepts only a step number strictly
//! greater than the last recorded one — one entry per step, in order.
//! A stale or repeated number is a [`JournalError::OutOfOrderStep`]
//! with the coordinates; a rejected record leaves the journal
//! unchanged. An eventless step is recorded as an empty entry, so the
//! entry sequence doubles as a step audit trail.
//!
//! # No format, no version
//!
//! The in-memory journal is a container, not a data format, and
//! carries no version. A serialized journal is versioned with the
//! format that carries it — the replay format (E-03) and the save
//! format (E-08+) — per the version taxonomy of ADR-0002 §5.6.
//!
//! # Example
//!
//! A session loop records every step's outcome; the journal answers
//! with the chronological history.
//!
//! ```
//! use keldysh_core::EventJournal;
//!
//! let mut journal = EventJournal::new();
//! journal.record(0, vec!["founded a settlement", "explored a province"])?;
//! // An eventless day is recorded as an empty entry.
//! journal.record(1, Vec::<&str>::new())?;
//! journal.record(2, vec!["trained a unit"])?;
//!
//! let steps: Vec<u64> = journal.iter().map(|entry| entry.step()).collect();
//! assert_eq!(steps, [0, 1, 2]);
//! assert_eq!(journal.total_events(), 3);
//! assert_eq!(journal.last_step(), Some(2));
//!
//! // Chronology is enforced by the container: step 1 is already gone.
//! assert_eq!(
//!     journal.record(1, vec!["late arrival"]),
//!     Err(keldysh_core::JournalError::OutOfOrderStep {
//!         last_recorded: 2,
//!         rejected: 1,
//!     }),
//! );
//! # Ok::<(), keldysh_core::JournalError>(())
//! ```

use std::fmt;

/// One recorded step: the step number with the events it produced.
///
/// The events are the step outcome moved into the journal; iteration
/// yields the entries in recording (chronological) order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepEvents<E> {
    step: u64,
    events: Vec<E>,
}

impl<E> StepEvents<E> {
    /// The number of the recorded step.
    pub fn step(&self) -> u64 {
        self.step
    }

    /// The events the step produced, in execution order.
    pub fn events(&self) -> &[E] {
        &self.events
    }
}

/// Why a record could not be appended to the journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalError {
    /// The step number is not strictly greater than the last recorded
    /// one: the journal is chronological, one entry per step.
    OutOfOrderStep {
        /// The last recorded step.
        last_recorded: u64,
        /// The rejected step number.
        rejected: u64,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::OutOfOrderStep {
                last_recorded,
                rejected,
            } => write!(
                f,
                "step {rejected} cannot be recorded after step {last_recorded}: \
                 the journal is chronological, one entry per step"
            ),
        }
    }
}

impl std::error::Error for JournalError {}

/// The append-only chronological journal of step events.
///
/// The session loop records each step's outcome after the step
/// executes; the journal never feeds back into the simulation. The
/// full contract is documented at the module level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventJournal<E> {
    entries: Vec<StepEvents<E>>,
}

impl<E> Default for EventJournal<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> EventJournal<E> {
    /// Creates the empty journal.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Records one step's outcome: the step number and the events the
    /// step produced. The number must be strictly greater than the
    /// last recorded one; a rejected record leaves the journal
    /// unchanged.
    pub fn record(&mut self, step: u64, events: Vec<E>) -> Result<(), JournalError> {
        if let Some(last) = self.entries.last()
            && step <= last.step
        {
            return Err(JournalError::OutOfOrderStep {
                last_recorded: last.step,
                rejected: step,
            });
        }
        self.entries.push(StepEvents { step, events });
        Ok(())
    }

    /// The last recorded step number, if anything was recorded.
    pub fn last_step(&self) -> Option<u64> {
        self.entries.last().map(|entry| entry.step)
    }

    /// Iterates the entries in recording (chronological) order.
    pub fn iter(&self) -> std::slice::Iter<'_, StepEvents<E>> {
        self.entries.iter()
    }

    /// The number of events across all recorded steps.
    pub fn total_events(&self) -> usize {
        self.entries.iter().map(|entry| entry.events.len()).sum()
    }

    /// Whether nothing has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_in_order_and_answers_the_history() {
        let mut journal = EventJournal::new();
        assert!(journal.is_empty());
        assert_eq!(journal.last_step(), None);

        journal.record(0, vec![1]).unwrap();
        journal.record(3, vec![2, 3]).unwrap();
        let steps: Vec<u64> = journal.iter().map(|entry| entry.step()).collect();
        assert_eq!(steps, [0, 3]);
        let events: Vec<u64> = journal
            .iter()
            .flat_map(|entry| entry.events().iter().copied())
            .collect();
        assert_eq!(events, [1, 2, 3]);
        assert_eq!(journal.total_events(), 3);
        assert_eq!(journal.last_step(), Some(3));
    }

    #[test]
    fn stale_and_repeated_steps_are_rejected_without_changes() {
        let mut journal = EventJournal::new();
        journal.record(5, vec!['a']).unwrap();

        let repeated = journal.record(5, vec!['b']).unwrap_err();
        let stale = journal.record(4, vec!['c']).unwrap_err();
        assert_eq!(
            repeated,
            JournalError::OutOfOrderStep {
                last_recorded: 5,
                rejected: 5
            }
        );
        assert_eq!(
            stale,
            JournalError::OutOfOrderStep {
                last_recorded: 5,
                rejected: 4
            }
        );
        assert_eq!(
            repeated.to_string(),
            "step 5 cannot be recorded after step 5: the journal is chronological, \
             one entry per step"
        );

        // The rejected records left the journal unchanged; the next
        // valid step is still accepted.
        assert_eq!(journal.last_step(), Some(5));
        assert_eq!(journal.total_events(), 1);
        journal.record(6, vec!['d']).unwrap();
        assert_eq!(journal.last_step(), Some(6));
    }
}
