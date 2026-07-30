//! Batch job state — Epic 16 Slice C.
//!
//! Decision 2: **batch is a job, not a request.** A 500k-row file cannot be
//! request/response, so upload returns a handle and progress is polled.
//!
//! What can go wrong here is entirely in the *verdict*: a job that processed
//! 400k rows and rejected 100k is not "failed", and calling it that discards the
//! 400k. A job whose process died is not "running", however much its last row
//! said so. Both are decided here, purely, where they can be tested without a
//! file or a database.

use std::fmt;

/// Where a job is. Reported to a poller, so every value has to mean something a
/// client can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    /// Every row landed.
    Succeeded,
    /// Some rows landed and some did not. **Distinct from failed**: a client that
    /// treated this as failure would re-push 400k rows to retry 100k, and one
    /// that treated it as success would never learn about the 100k.
    Partial,
    /// Nothing usable came of it — or it was stopped.
    Failed,
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
        })
    }
}

/// Why a job stopped before reading its file to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// The error cap was reached.
    ///
    /// **Stopping is the feature.** A 500k-row file with a wrong delimiter
    /// produces 500k errors, and a report nobody can read is a report nobody
    /// reads — the cap turns "everything is broken" into one legible sentence.
    ErrorCap { cap: usize },
    /// Somebody cancelled it.
    Cancelled,
    /// The process handling it stopped reporting.
    Abandoned,
}

/// What a job has done so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub rows_read: u64,
    pub accepted: u64,
    pub rejected: u64,
}

/// The verdict for a job that has stopped reading.
///
/// `halt` is `None` when the file was read to the end.
#[must_use]
pub fn verdict(progress: Progress, halt: Option<&Halt>) -> JobState {
    // A halt means the file was not read to the end, so the counts describe a
    // prefix rather than a result. `Partial` would imply the rest was considered
    // and rejected, which is a stronger claim than anything here supports.
    if halt.is_some() {
        return JobState::Failed;
    }
    if progress.rejected == 0 {
        // Covers the empty file too: nothing was asked for and nothing went
        // wrong, and reporting failure would make an empty export look broken.
        return JobState::Succeeded;
    }
    if progress.accepted == 0 {
        // Rejected everything. Not partial — that bucket is for jobs that mostly
        // worked, and a job with nothing to keep does not belong in it.
        return JobState::Failed;
    }
    JobState::Partial
}

/// Whether processing should stop before the next row.
#[must_use]
pub fn should_halt(progress: Progress, error_cap: usize, cancelled: bool) -> Option<Halt> {
    // Cancellation is checked first because both can be true at once, and "you
    // cancelled it" is the answer that matches what the person actually did.
    if cancelled {
        return Some(Halt::Cancelled);
    }
    // Two conditions, and both are load-bearing. `>=` because reaching the cap is
    // the trigger, not exceeding it. `rejected > 0` because the cap is about
    // *errors*: with none there is nothing to cap, and `>=` alone would halt a
    // clean job the moment somebody asked for `cap: 0` — which legitimately means
    // "stop at the first error", not "stop immediately".
    if progress.rejected > 0 && progress.rejected >= u64::try_from(error_cap).unwrap_or(u64::MAX) {
        return Some(Halt::ErrorCap { cap: error_cap });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(read: u64, accepted: u64, rejected: u64) -> Progress {
        Progress {
            rows_read: read,
            accepted,
            rejected,
        }
    }

    // The wire spelling. A client polls this and branches on it, so a rename is a
    // contract change — and `partial` in particular has to survive, since telling
    // a client "failed" for a job that landed 400k rows is the mistake this whole
    // state machine exists to prevent.
    #[test]
    fn every_state_has_a_stable_wire_spelling() {
        assert_eq!(JobState::Queued.to_string(), "queued");
        assert_eq!(JobState::Running.to_string(), "running");
        assert_eq!(JobState::Succeeded.to_string(), "succeeded");
        assert_eq!(JobState::Partial.to_string(), "partial");
        assert_eq!(JobState::Failed.to_string(), "failed");
    }

    // ---- the verdict ----

    #[test]
    fn a_file_read_to_the_end_with_no_errors_succeeded() {
        assert_eq!(verdict(progress(100, 100, 0), None), JobState::Succeeded);
    }

    // **The distinction the whole state exists for.** A client that read this as
    // failure would re-push 400k rows to retry 100k; one that read it as success
    // would never learn about the 100k.
    #[test]
    fn some_landed_and_some_did_not_is_partial_not_failed() {
        assert_eq!(verdict(progress(500, 400, 100), None), JobState::Partial);
    }

    // Nothing landed is a failure even though the file was read: a job that
    // rejected every row has produced nothing to keep, and calling it partial
    // would put it in the same bucket as one that mostly worked.
    #[test]
    fn a_file_where_nothing_landed_failed() {
        assert_eq!(verdict(progress(500, 0, 500), None), JobState::Failed);
    }

    // An empty file is not a failure. Nothing was asked for and nothing went
    // wrong — reporting failure would make an empty export look like a broken one.
    #[test]
    fn an_empty_file_succeeded() {
        assert_eq!(verdict(progress(0, 0, 0), None), JobState::Succeeded);
    }

    // A halt is always a failure *of the job*, whatever landed — the file was not
    // read to the end, so the counts describe a prefix rather than a result, and
    // "partial" would imply the rest was considered and rejected.
    #[test]
    fn any_halt_fails_the_job_however_much_landed() {
        for halt in [
            Halt::ErrorCap { cap: 1000 },
            Halt::Cancelled,
            Halt::Abandoned,
        ] {
            assert_eq!(
                verdict(progress(400_000, 399_000, 1_000), Some(&halt)),
                JobState::Failed,
                "{halt:?} should fail the job"
            );
        }
    }

    // ---- when to stop ----

    #[test]
    fn a_healthy_job_does_not_halt() {
        assert_eq!(should_halt(progress(100, 99, 1), 1000, false), None);
    }

    // "Error cap (1000) fails the job with a clear reason rather than an
    // unreadable report." A 500k-row file with the wrong delimiter produces 500k
    // errors, and nobody reads that.
    #[test]
    fn reaching_the_error_cap_halts() {
        assert_eq!(
            should_halt(progress(2000, 1000, 1000), 1000, false),
            Some(Halt::ErrorCap { cap: 1000 })
        );
    }

    // One below the cap keeps going, so the boundary is the cap rather than
    // somewhere near it.
    #[test]
    fn one_error_below_the_cap_keeps_going() {
        assert_eq!(should_halt(progress(2000, 1001, 999), 1000, false), None);
    }

    // A cap of zero means "stop at the first error", which is a legitimate thing
    // to ask for and must not be read as "no cap".
    #[test]
    fn a_zero_cap_stops_at_the_first_error() {
        assert_eq!(
            should_halt(progress(1, 0, 1), 0, false),
            Some(Halt::ErrorCap { cap: 0 })
        );
        assert_eq!(should_halt(progress(1, 1, 0), 0, false), None);
    }

    #[test]
    fn cancellation_halts() {
        assert_eq!(
            should_halt(progress(10, 10, 0), 1000, true),
            Some(Halt::Cancelled)
        );
    }

    // Cancellation wins over the cap: both are true, and "you cancelled it" is the
    // answer that matches what the person did.
    #[test]
    fn cancellation_is_reported_over_the_error_cap() {
        assert_eq!(
            should_halt(progress(2000, 1000, 1000), 1000, true),
            Some(Halt::Cancelled)
        );
    }
}
