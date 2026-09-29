//! This crate owns its share of the Keldysh contract — the determinism
//! harness. The invariants, the version model, and what breaks a replay are
//! documented in the [`keldysh_core`] crate documentation.
//!
//! The determinism test harness of the Keldysh framework.
//!
//! Every determinism guarantee needs a test: same inputs, two runs,
//! identical results. In-process tests cannot see what stands behind
//! the process — the `RandomState` of a `HashMap` is seeded from the
//! OS per process, so a map iterated inside the state produces
//! different orders in different processes under identical inputs,
//! while the in-process twin runs agree. The harness makes the
//! cross-process check a routine assertion (ADR-0002 §5.1, D1).
//!
//! The party under test is a closure returning the checkpoint hashes
//! — the state hash at the checkpoints the party chooses (the
//! "checkpoint dates" of the game, E-08). The comparison reports the
//! first divergent checkpoint with both hashes.
//!
//! - [`two_runs`] executes the party twice in this process: the fast
//!   unit-level check.
//! - [`two_processes`] executes the party here and in a fresh
//!   process: the current test binary re-invokes itself in worker
//!   mode (the environment marker `KELDYSH_TESTKIT_WORKER`, the
//!   libtest filter `--exact`, the hashes over stdout). This is the
//!   check in-process tests cannot substitute; it runs inside
//!   `cargo test` test binaries.
//!
//! # Example
//!
//! ```
//! use keldysh_testkit::two_runs;
//!
//! // A deterministic party: the same checkpoints every run.
//! let party = || vec![0x111, 0x222, 0x333];
//! assert!(two_runs(party).is_ok());
//! ```
//!
//! The two-process pattern (`no_run` here — it spawns a process):
//!
//! ```no_run
//! use keldysh_testkit::two_processes;
//!
//! fn compute_checkpoints() -> Vec<u64> {
//!     Vec::new()
//! }
//!
//! #[test]
//! fn the_party_is_deterministic_across_processes() {
//!     two_processes(
//!         "the_party_is_deterministic_across_processes",
//!         compute_checkpoints,
//!     )
//!     .unwrap();
//! }
//! ```

use std::fmt;
use std::process::{Command, Stdio};

/// Why the two runs did not produce identical checkpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The first checkpoint where the hashes differed.
    Checkpoint {
        /// The index of the divergent checkpoint.
        checkpoint: usize,
        /// The hash of the reference run.
        expected: u64,
        /// The hash of the other run.
        actual: u64,
    },
    /// The runs produced different numbers of checkpoints.
    Length {
        /// The number of checkpoints of the reference run.
        expected: usize,
        /// The number of checkpoints of the other run.
        actual: usize,
    },
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Divergence::Checkpoint {
                checkpoint,
                expected,
                actual,
            } => write!(
                f,
                "first divergence at checkpoint {checkpoint}: expected \
                 {expected:#018x}, got {actual:#018x}"
            ),
            Divergence::Length { expected, actual } => write!(
                f,
                "the runs produced different checkpoint counts: expected \
                 {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for Divergence {}

/// Runs `party` twice in this process and compares the checkpoint
/// hashes. The fast check; it does not see what stands behind the
/// process — use [`two_processes`] for the cross-process guarantee.
pub fn two_runs(party: impl Fn() -> Vec<u64>) -> Result<(), Divergence> {
    compare(party(), party())
}

/// Runs `party` in this process and in a fresh process, then compares
/// the checkpoint hashes.
///
/// The fresh process is the current test binary re-invoked with the
/// libtest filter `["--exact", tag, "--nocapture"]` and the worker
/// marker `KELDYSH_TESTKIT_WORKER` in its environment: the tagged test
/// runs alone, the harness executes the party, prints the hashes, and
/// exits. The closure itself never crosses the process boundary — the
/// child runs the same compiled code. Infrastructure failures (the
/// binary cannot spawn, the worker crashed or printed nothing) panic
/// with the worker's diagnostics; only a genuine determinism
/// divergence is a [`Divergence`].
///
/// The `tag` must be the exact name of the enclosing test (it doubles
/// as the libtest filter) and must not contain spaces.
pub fn two_processes(tag: &'static str, party: impl Fn() -> Vec<u64>) -> Result<(), Divergence> {
    if std::env::var(WORKER_ENV).is_ok_and(|worker| worker == tag) {
        report_and_exit(tag, party);
    }

    let exe = std::env::current_exe().expect("the current test binary path");
    let output = Command::new(exe)
        .args([tag, "--exact", "--nocapture"])
        .env(WORKER_ENV, tag)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("the worker process spawns");
    let child = parse_worker_output(&output, tag);
    compare(party(), child)
}

/// Compares the two checkpoint sequences.
fn compare(reference: Vec<u64>, other: Vec<u64>) -> Result<(), Divergence> {
    if reference.len() != other.len() {
        return Err(Divergence::Length {
            expected: reference.len(),
            actual: other.len(),
        });
    }
    for (checkpoint, (expected, actual)) in reference.iter().zip(other.iter()).enumerate() {
        if expected != actual {
            return Err(Divergence::Checkpoint {
                checkpoint,
                expected: *expected,
                actual: *actual,
            });
        }
    }
    Ok(())
}

/// The environment marker of the worker mode.
const WORKER_ENV: &str = "KELDYSH_TESTKIT_WORKER";
/// The prefix of a worker hash line on stdout.
const LINE_PREFIX: &str = "keldysh-testkit:";

/// The worker side: prints the party's hashes and leaves the process.
fn report_and_exit(tag: &str, party: impl FnOnce() -> Vec<u64>) -> ! {
    for hash in party() {
        println!("{LINE_PREFIX} {tag} {hash:#018x}");
    }
    std::process::exit(0);
}

/// Parses the worker's stdout into the checkpoint hashes; a crashed
/// or silent worker panics with its diagnostics.
fn parse_worker_output(output: &std::process::Output, tag: &str) -> Vec<u64> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hashes = Vec::new();
    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix(LINE_PREFIX) else {
            continue;
        };
        let Some((worker_tag, hash)) = rest.trim().split_once(' ') else {
            continue;
        };
        if worker_tag != tag {
            continue;
        }
        let hash = u64::from_str_radix(hash.trim_start_matches("0x"), 16).unwrap_or_else(|error| {
            panic!("the worker printed a malformed hash {hash:?}: {error}")
        });
        hashes.push(hash);
    }

    if !output.status.success() || hashes.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "the worker process failed (status {}): no checkpoint hashes for {tag:?} \
             came back;\nworker stderr tail:\n{tail}",
            output.status
        );
    }
    hashes
}
