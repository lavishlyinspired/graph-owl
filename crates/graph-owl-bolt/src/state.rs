//! The connection state machine's pure decision core — Epic 7d Slice C.
//!
//! **Split from the I/O that drives it**, the same separation
//! `graph-owl-constraint`'s validator or `graph-owl-authz`'s
//! `AccessPredicate` keep: [`admit`] answers "is this message legal right
//! now", exhaustively and without a socket in sight, and `crate::server`
//! is the only thing that acts on the answer.
//!
//! **`FAILED` ignoring everything but `RESET` is the property this module
//! exists to get right.** A server that keeps processing messages after a
//! failure lets a client's pipelined batch run half-executed with no way to
//! know which half — `07d-engine-bolt.md`'s own words for why this gets its
//! own slice rather than falling out of ordinary error handling.

use crate::messages::MessageKind;

/// Where a connection is in its lifecycle. `Negotiation` exists only until
/// the first `HELLO` succeeds — every connection starts there, and no
/// message ever returns a session to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Negotiation,
    Authed,
    Streaming,
    Failed,
}

/// A connection's state, in full: the [`Phase`] plus whether an explicit
/// transaction is open. Kept as a separate flag rather than extra `Phase`
/// variants, because "streaming" and "inside an explicit transaction" are
/// independent facts — a `RUN` issued after `BEGIN` still passes through
/// `Streaming` while pulling its results, and must return to `Authed` with
/// `in_transaction` still `true`, not reset to autocommit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub phase: Phase,
    pub in_transaction: bool,
}

impl Session {
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: Phase::Negotiation,
            in_transaction: false,
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// What to do with a message, decided from [`Session`] and [`MessageKind`]
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Legal here — hand it to the message-specific handler.
    Proceed,
    /// `FAILED` swallows it: respond `IGNORED`, no state change, no attempt
    /// to interpret the message's fields at all.
    Ignore,
    /// Not legal here, and not something `FAILED` would swallow either — a
    /// protocol violation. The caller responds `FAILURE` and closes the
    /// connection; there is no recovering a connection that sent this.
    Violation,
}

/// Decide what a message means in this session's current state.
///
/// `RESET` and `GOODBYE` are legal in every phase including `FAILED` — that
/// is the entire mechanism by which a `FAILED` connection ever becomes
/// useful again, and why they are checked before the `FAILED`-swallows-all
/// rule rather than after it.
#[must_use]
pub fn admit(session: Session, kind: MessageKind) -> Outcome {
    if matches!(kind, MessageKind::Reset | MessageKind::Goodbye) {
        return Outcome::Proceed;
    }
    if session.phase == Phase::Failed {
        return Outcome::Ignore;
    }
    match (session.phase, kind) {
        (Phase::Negotiation, MessageKind::Hello) => Outcome::Proceed,
        (Phase::Negotiation, _) => Outcome::Violation,

        (Phase::Authed, MessageKind::Run | MessageKind::Begin) => Outcome::Proceed,
        (Phase::Authed, MessageKind::Commit | MessageKind::Rollback) => {
            if session.in_transaction {
                Outcome::Proceed
            } else {
                Outcome::Violation
            }
        }
        (Phase::Authed, MessageKind::Hello | MessageKind::Pull | MessageKind::Discard) => {
            Outcome::Violation
        }

        (Phase::Streaming, MessageKind::Pull | MessageKind::Discard) => Outcome::Proceed,
        (Phase::Streaming, _) => Outcome::Violation,

        (Phase::Failed, _) => unreachable!("handled above"),
        // Reset and Goodbye returned above for every phase; the compiler
        // cannot see that from here, but nothing reaches this arm.
        (_, MessageKind::Reset | MessageKind::Goodbye) => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authed() -> Session {
        Session {
            phase: Phase::Authed,
            in_transaction: false,
        }
    }

    fn streaming() -> Session {
        Session {
            phase: Phase::Streaming,
            in_transaction: false,
        }
    }

    fn failed() -> Session {
        Session {
            phase: Phase::Failed,
            in_transaction: false,
        }
    }

    #[test]
    fn hello_is_legal_only_before_authentication() {
        assert_eq!(admit(Session::new(), MessageKind::Hello), Outcome::Proceed);
        assert_eq!(admit(authed(), MessageKind::Hello), Outcome::Violation);
    }

    #[test]
    fn nothing_but_hello_is_legal_during_negotiation() {
        for kind in [
            MessageKind::Run,
            MessageKind::Pull,
            MessageKind::Discard,
            MessageKind::Begin,
            MessageKind::Commit,
            MessageKind::Rollback,
        ] {
            assert_eq!(
                admit(Session::new(), kind),
                Outcome::Violation,
                "{kind:?} must not be legal pre-HELLO"
            );
        }
    }

    #[test]
    fn run_is_legal_once_authed() {
        assert_eq!(admit(authed(), MessageKind::Run), Outcome::Proceed);
    }

    #[test]
    fn pull_and_discard_are_legal_only_while_streaming() {
        assert_eq!(admit(streaming(), MessageKind::Pull), Outcome::Proceed);
        assert_eq!(admit(streaming(), MessageKind::Discard), Outcome::Proceed);
        assert_eq!(
            admit(authed(), MessageKind::Pull),
            Outcome::Violation,
            "nothing is streaming yet"
        );
        assert_eq!(admit(authed(), MessageKind::Discard), Outcome::Violation);
    }

    #[test]
    fn begin_is_legal_when_authed() {
        assert_eq!(admit(authed(), MessageKind::Begin), Outcome::Proceed);
    }

    #[test]
    fn commit_and_rollback_require_an_open_transaction() {
        let in_tx = Session {
            phase: Phase::Authed,
            in_transaction: true,
        };
        assert_eq!(admit(in_tx, MessageKind::Commit), Outcome::Proceed);
        assert_eq!(admit(in_tx, MessageKind::Rollback), Outcome::Proceed);
        assert_eq!(
            admit(authed(), MessageKind::Commit),
            Outcome::Violation,
            "COMMIT with nothing to commit must not silently succeed"
        );
        assert_eq!(admit(authed(), MessageKind::Rollback), Outcome::Violation);
    }

    #[test]
    fn a_run_while_already_streaming_is_a_violation_not_pipelined() {
        // Documented scope reduction: one active result stream per
        // connection. A driver's own PULL/DISCARD-before-next-RUN discipline
        // means this never triggers in ordinary use.
        assert_eq!(admit(streaming(), MessageKind::Run), Outcome::Violation);
    }

    #[test]
    fn failed_ignores_run_pull_and_discard() {
        for kind in [
            MessageKind::Run,
            MessageKind::Pull,
            MessageKind::Discard,
            MessageKind::Begin,
        ] {
            assert_eq!(
                admit(failed(), kind),
                Outcome::Ignore,
                "{kind:?} must be ignored, not violated or proceeded"
            );
        }
    }

    #[test]
    fn reset_is_legal_from_every_phase() {
        for session in [Session::new(), authed(), streaming(), failed()] {
            assert_eq!(admit(session, MessageKind::Reset), Outcome::Proceed);
        }
    }

    #[test]
    fn goodbye_is_legal_from_every_phase() {
        for session in [Session::new(), authed(), streaming(), failed()] {
            assert_eq!(admit(session, MessageKind::Goodbye), Outcome::Proceed);
        }
    }

    #[test]
    fn the_pipelined_batch_after_a_failure_is_ignored_until_reset() {
        // The scenario the module doc names: RUN, an erroring RUN, then
        // PULL, all sent without waiting. Once a failure has put the session
        // into FAILED, every one of those trailing messages must be ignored
        // — this is what a request-response test can never exercise, since
        // it always waits for one answer before sending the next.
        let session = failed();
        assert_eq!(admit(session, MessageKind::Run), Outcome::Ignore);
        assert_eq!(admit(session, MessageKind::Pull), Outcome::Ignore);
        assert_eq!(admit(session, MessageKind::Discard), Outcome::Ignore);
        assert_eq!(
            admit(session, MessageKind::Reset),
            Outcome::Proceed,
            "only RESET recovers"
        );
    }
}
