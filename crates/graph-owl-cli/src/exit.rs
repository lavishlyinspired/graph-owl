//! Exit codes and CI gating — Epic 20 Slice G.
//!
//! **Exit codes that mean something**, per the CLI conventions: CI branches
//! on them without parsing text, so a change to a message never silently
//! changes a pipeline's behaviour.

use crate::plan::Plan;

/// No changes pending.
pub const NO_CHANGES: i32 = 0;
/// An error — invalid declarations, an unreachable catalog, a failed apply.
pub const ERROR: i32 = 1;
/// Changes are pending. **Not an error**: `plan` succeeding and finding work
/// is the normal case, and conflating it with failure would make every
/// pipeline treat a legitimate diff as a broken build.
pub const CHANGES_PENDING: i32 = 2;

/// What a CI run should fail on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailOn {
    /// Never fail on the plan's content — report and continue.
    #[default]
    Nothing,
    /// Fail if anything at all would change. For a repository that is meant
    /// to be the sole author of its scope, a pending change means someone
    /// edited live state.
    AnyChange,
    /// Fail only if the plan would **delete**. The common gate: a pull
    /// request that adds or edits is routine, one that tombstones assets
    /// needs a human to look at it.
    Deletions,
}

/// The exit code for a plan under a given gate.
#[must_use]
pub fn code_for(plan: &Plan, fail_on: FailOn) -> i32 {
    let counts = plan.counts();
    match fail_on {
        FailOn::Deletions if counts.prune > 0 => ERROR,
        FailOn::AnyChange if plan.has_changes() => ERROR,
        _ if plan.has_changes() => CHANGES_PENDING,
        _ => NO_CHANGES,
    }
}

/// Strips anything credential-shaped from text bound for a log.
///
/// Plan output is printed in CI, where it is retained and often public. The
/// tool holds a token to reach the catalog, and the failure this prevents is
/// not the tool printing its own token deliberately — it is a token reaching
/// output through an error message quoting a request. `DATABASE_URL`
/// redaction in `graph-owl-server` exists for the same reason.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("authorization")
            || lower.contains("bearer ")
            || lower.contains("token")
            || lower.contains("password")
            || lower.contains("secret")
        {
            // The whole line, not just the value: a redactor that tries to
            // find "the credential part" is a parser, and a parser that is
            // wrong once leaks.
            out.push_str("[redacted]\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
