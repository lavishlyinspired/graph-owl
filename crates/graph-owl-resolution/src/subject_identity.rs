//! Domain-pack subject identity resolution — Plan 109 Slice 1.
//!
//! **Platform-generic, per the plan's own binding decision.** This module
//! has no idea what a GSTIN or an invoice is — a pack supplies an
//! [`IdentityPolicy`] naming which [`Strategy`] computes its own
//! business-identity key and, optionally, which computes a collision guard.
//! GST is the first caller, not a reason to special-case anything here.
//!
//! **The three-way decision, per the user's own exact wording**: same
//! business key + compatible collision guard resolves automatically;
//! same business key + incompatible guard is `Ambiguous`; a candidate whose
//! business key does not match at all — found only through a pack's own
//! blocking (an n-gram near-miss, say) — is `Ambiguous` too, never an
//! auto-attach. [`decide`] is the single function that turns those three
//! cases into one answer, so no caller can accidentally skip the guard on
//! the exact-match path — the false-merge gap the second review pass on
//! Plan 109 found in this plan's first draft.

use graph_owl_core::blocking_strategy::{Record, Strategy};

/// A pack's own identity policy: which strategy computes the hard
/// business-identity key, and which (if any) computes the collision guard
/// checked once that key matches exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityPolicy {
    /// Two records whose keys under this strategy are equal (and present)
    /// share a business identity.
    pub business_key: Strategy,
    /// Checked only once `business_key` already matched exactly. `None`
    /// means the policy has no guard, so an exact business-key match always
    /// attaches — the right default for a pack whose identity is already
    /// unambiguous without one.
    pub collision_guard: Option<Strategy>,
}

/// What [`decide`] concluded about one candidate against one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectDecision {
    /// The business-identity keys matched exactly, and the collision guard
    /// (if the policy has one) agreed. The caller writes an attachment.
    Attach,
    /// Either the business-identity keys did not match exactly, or they did
    /// and the collision guard's fields conflict. **The caller writes
    /// nothing** — this is the load-bearing property Plan 109's second
    /// review pass added: a hard-key match is not itself proof of identity.
    Ambiguous,
}

/// Decides one candidate against one target under a pack's own identity
/// policy.
///
/// The caller is assumed to have already found `candidate` worth comparing
/// at all — via the pack's own blocking configuration, typically — this
/// function only adjudicates *which* of the three outcomes applies; it does
/// not search for candidates.
#[must_use]
pub fn decide(policy: &IdentityPolicy, target: &Record, candidate: &Record) -> SubjectDecision {
    let exact_business_key_match = matches!(
        (
            policy.business_key.key(target),
            policy.business_key.key(candidate),
        ),
        (Some(t), Some(c)) if t == c
    );
    if !exact_business_key_match {
        return SubjectDecision::Ambiguous;
    }

    let Some(guard) = &policy.collision_guard else {
        return SubjectDecision::Attach;
    };
    let guard_compatible = matches!(
        (guard.key(target), guard.key(candidate)),
        (Some(t), Some(c)) if t == c
    );
    if guard_compatible {
        SubjectDecision::Attach
    } else {
        SubjectDecision::Ambiguous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: &[(&str, &str)]) -> Record {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// The GST identity policy this slice was built for: supplier GSTIN +
    /// normalized invoice number + document type form the business key;
    /// invoice date, bucketed by fiscal year (India's April start), is the
    /// collision guard.
    fn gst_invoice_policy() -> IdentityPolicy {
        IdentityPolicy {
            business_key: Strategy::Exact {
                fields: vec![
                    "supplierGstin".to_string(),
                    "invoiceKey".to_string(),
                    "documentType".to_string(),
                ],
            },
            collision_guard: Some(Strategy::FiscalYear {
                fields: vec!["invoiceDate".to_string()],
                starts_in: 4,
            }),
        }
    }

    fn invoice(gstin: &str, key: &str, date: &str) -> Record {
        record(&[
            ("supplierGstin", gstin),
            ("invoiceKey", key),
            ("documentType", "Invoice"),
            ("invoiceDate", date),
        ])
    }

    // ── same business key, compatible guard → Attach ────────────────────

    #[test]
    fn an_exact_business_key_match_with_a_compatible_date_attaches() {
        let target = invoice("29ABCDE1234F1Z5", "INV001", "2026-08-03");
        let candidate = invoice("29ABCDE1234F1Z5", "INV001", "2026-08-05");

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Attach
        );
    }

    // ── same business key, incompatible guard → Ambiguous ───────────────
    // The user's own example, verbatim.

    #[test]
    fn an_exact_business_key_match_a_year_apart_is_ambiguous_not_attached() {
        let target = invoice("29ABCDE1234F1Z5", "INV001", "2025-01-10");
        let candidate = invoice("29ABCDE1234F1Z5", "INV001", "2026-01-10");

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Ambiguous,
            "a hard-key match with an incompatible date must not silently merge"
        );
    }

    /// The boundary itself: one day before the fiscal year starts is a
    /// genuinely different fiscal year, so this must be `Ambiguous` too —
    /// not just "far apart" dates.
    #[test]
    fn an_exact_business_key_match_either_side_of_the_fiscal_boundary_is_ambiguous() {
        let target = invoice("29ABCDE1234F1Z5", "INV001", "2025-03-31");
        let candidate = invoice("29ABCDE1234F1Z5", "INV001", "2025-04-01");

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Ambiguous
        );
    }

    // ── no business key match at all → Ambiguous, never Attach ──────────
    // The pack's own planted transposition/PAN-mismatch shapes: a
    // different GSTIN entirely, however the candidate was found.

    #[test]
    fn a_transposed_gstin_never_attaches_however_close_the_dates() {
        // `…1MZ` against `…1ZM` — the pack's own planted transposition.
        let target = invoice("27AABCU9603R1ZM", "INV1004", "2026-07-21");
        let candidate = invoice("27AABCU9603R1MZ", "INV1004", "2026-07-21");

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Ambiguous,
            "fuzzy similarity alone must never auto-attach, regardless of score"
        );
    }

    #[test]
    fn a_pan_level_registration_difference_is_ambiguous_not_attached() {
        let target = invoice("29AABCU9603R1ZK", "INV1015", "2026-07-31");
        let candidate = invoice("27AABCU9603R1ZM", "INV1015", "2026-07-31");

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Ambiguous
        );
    }

    // ── a policy with no collision guard ─────────────────────────────────

    #[test]
    fn with_no_collision_guard_an_exact_business_key_match_always_attaches() {
        let policy = IdentityPolicy {
            business_key: Strategy::Exact {
                fields: vec!["supplierGstin".to_string(), "invoiceKey".to_string()],
            },
            collision_guard: None,
        };
        let target = invoice("29ABCDE1234F1Z5", "INV001", "2020-01-01");
        let candidate = invoice("29ABCDE1234F1Z5", "INV001", "2030-12-31");

        assert_eq!(
            decide(&policy, &target, &candidate),
            SubjectDecision::Attach
        );
    }

    // ── a record missing a business-key field ────────────────────────────
    // `Strategy::key` already returns `None` for a missing field (all-or-
    // nothing, per `blocking_strategy`'s own contract) — this asserts that
    // `decide` treats two `None`s as *not* a match, not as a vacuous one.

    #[test]
    fn two_records_both_missing_the_business_key_field_are_not_a_match() {
        let target = record(&[("invoiceKey", "INV001")]); // no supplierGstin
        let candidate = record(&[("invoiceKey", "INV001")]); // no supplierGstin

        assert_eq!(
            decide(&gst_invoice_policy(), &target, &candidate),
            SubjectDecision::Ambiguous,
            "two absent keys must not be treated as an equal, matching key"
        );
    }
}
