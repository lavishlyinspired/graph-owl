//! Deterministic FQN matching — Epic 17 Slice A.
//!
//! Most duplication across write paths is case or quoting, not a genuinely
//! different identity, and that half must not depend on a similarity
//! threshold (decision 4). This module is the whole of that half: a pure
//! normal form, and a short-circuit that proves scoring is never reached
//! for a deterministic match.
//!
//! **The canonical form is a `Vec` of segments, never a rejoined string.**
//! A quoted segment can legitimately contain a literal dot (`"my.db"`), so
//! rejoining normalized segments with `.` would make `service."sales.orders"`
//! (two segments) collide with `service.sales.orders` (three) — the exact
//! kind of false-positive merge decision 3 exists to prevent. Comparing the
//! segment vectors directly keeps segment *count* part of the identity.

/// One FQN's segments after quoting and escaping are resolved, lowercased.
/// Not a display form — only ever compared for equality.
pub type NormalizedFqn = Vec<String>;

/// The normal form used for deterministic matching: segments split on an
/// *unquoted* `.`, a literal dot inside a segment decoded from `%2E`/`%2e`
/// (the two escape conventions Slice A's acceptance criteria name), and
/// every segment lowercased.
#[must_use]
pub fn normalize_fqn(raw: &str) -> NormalizedFqn {
    tokenize(raw)
        .into_iter()
        .map(|segment| segment.to_lowercase())
        .collect()
}

fn tokenize(raw: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    // Toggled by an unescaped `"`, tracking whether `.` is a separator or
    // literal content — the same convention quoted SQL identifiers use.
    let mut quoted = false;
    for c in raw.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => {
                segments.push(decode_escaped_dot(&current));
                current.clear();
            }
            _ => current.push(c),
        }
    }
    segments.push(decode_escaped_dot(&current));
    segments
}

/// `%2E`/`%2e` is the percent-encoding for `.` (RFC 3986) — the second
/// convention Slice A's acceptance criteria name, alongside quoting, for a
/// source system that cannot emit quoted segments at all.
fn decode_escaped_dot(segment: &str) -> String {
    segment.replace("%2E", ".").replace("%2e", ".")
}

/// Two FQNs address the same entity once quoting, escaping and case are
/// normalized away — the deterministic half of resolution, checked before
/// any scoring is attempted.
#[must_use]
pub fn is_deterministic_match(a: &str, b: &str) -> bool {
    normalize_fqn(a) == normalize_fqn(b)
}

/// What a pair of drafts resolved to, before confidence bands (Slice D)
/// interpret a `Scored` outcome.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PairOutcome {
    /// Matched deterministically — `score` was never called.
    DeterministicMatch,
    /// No deterministic match; this is what scoring produced.
    Scored(f64),
}

/// Resolve one pair, short-circuiting before `score` is invoked if the two
/// FQNs already match deterministically.
///
/// **The short-circuit is a correctness requirement, not an optimisation**
/// (Slice A's RED case): a bug in the scorer must never be able to affect an
/// exact match, which is only true if the scorer is structurally unreachable
/// for that case rather than merely overridden by a later check.
pub fn resolve_pair(a_fqn: &str, b_fqn: &str, score: impl FnOnce() -> f64) -> PairOutcome {
    if is_deterministic_match(a_fqn, b_fqn) {
        return PairOutcome::DeterministicMatch;
    }
    PairOutcome::Scored(score())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn case_differences_normalize_identically() {
        assert_eq!(
            normalize_fqn("prod.sales.orders"),
            normalize_fqn("PROD.SALES.ORDERS")
        );
    }

    #[test]
    fn a_quoted_segment_and_its_percent_encoded_equivalent_normalize_identically() {
        assert_eq!(
            normalize_fqn(r#"service."my.db".schema.t"#),
            normalize_fqn("service.my%2Edb.schema.t")
        );
    }

    #[test]
    fn lowercase_percent_encoding_is_also_decoded() {
        assert_eq!(
            normalize_fqn(r#"service."my.db".schema.t"#),
            normalize_fqn("service.my%2edb.schema.t")
        );
    }

    // A genuinely different FQN must not match — the negative half that
    // makes the positives above mean something, and the one a checker that
    // always returned `true` would fail.
    #[test]
    fn a_genuinely_different_fqn_does_not_match() {
        assert!(!is_deterministic_match(
            "service.sales.orders",
            "service.sales.order"
        ));
    }

    // **The collision a naive rejoin-with-dots implementation would miss.**
    // `service."sales.orders"` is two segments (`service`, `sales.orders`);
    // `service.sales.orders` is three (`service`, `sales`, `orders`). They
    // must not normalize to the same thing merely because a display-string
    // join would print identically.
    #[test]
    fn a_quoted_multi_word_segment_does_not_collide_with_separate_segments() {
        assert!(!is_deterministic_match(
            r#"service."sales.orders""#,
            "service.sales.orders"
        ));
    }

    #[test]
    fn is_deterministic_match_agrees_with_normalize_fqn() {
        assert!(is_deterministic_match(
            "prod.sales.orders",
            "PROD.SALES.ORDERS"
        ));
    }

    // **The short-circuit, proved rather than assumed.** A call counter is
    // the only way to show the scorer is structurally unreachable, not
    // merely unused by coincidence of the current implementation.
    #[test]
    fn an_exact_match_never_invokes_the_scorer() {
        let calls = Cell::new(0);

        let outcome = resolve_pair("prod.sales.orders", "PROD.SALES.ORDERS", || {
            calls.set(calls.get() + 1);
            0.5
        });

        assert_eq!(outcome, PairOutcome::DeterministicMatch);
        assert_eq!(calls.get(), 0, "the scorer must not run on an exact match");
    }

    // The negative: a non-match *does* reach the scorer, or the short
    // circuit would be indistinguishable from scoring never running at all.
    #[test]
    fn a_non_match_invokes_the_scorer_exactly_once() {
        let calls = Cell::new(0);

        let outcome = resolve_pair("service.sales.orders", "service.sales.order", || {
            calls.set(calls.get() + 1);
            0.42
        });

        assert_eq!(outcome, PairOutcome::Scored(0.42));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn an_unterminated_quote_does_not_panic() {
        // Malformed input from a source system that mis-escaped something —
        // the tokenizer must produce *something* rather than hang or panic.
        let _ = normalize_fqn(r#"service."unterminated"#);
    }
}
