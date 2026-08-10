//! Finding-rule comparison primitives — Epic 105 P5b.
//!
//! A pack's `[[findings]]` rule is a SPARQL query plus two conditions the
//! query itself cannot express: an n-gram similarity band, and a date-span
//! "exceeds N days" check. Both stay pure and I/O-free, which is why they
//! land in this crate rather than beside the SPARQL engine that binds their
//! inputs — `00e` rule 4 reserves `graph-owl-resolution` for exactly this,
//! and n-gram similarity is a second string-comparison strategy alongside
//! the jaro-winkler one [`crate::score`] already has.
//!
//! **Why the span check is not a SPARQL `FILTER`.** Measured against the
//! real engine: `xsd:date` subtraction, `date + duration`, and even
//! `date > date` all evaluate to unbound inside a query — it has no date
//! arithmetic in expressions. A day count needs calendar arithmetic, so the
//! query does the join and this module does the arithmetic.

use chrono::NaiveDate;

/// Why a rule's similarity or span condition could not be evaluated.
///
/// **Always returned, never absorbed into a default.** A misconfigured rule
/// that silently scored 0.0 or silently never fired would report a clean
/// reconciliation — the one failure mode this module exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleMatchError {
    /// A similarity strategy this module does not implement.
    UnknownStrategy(String),
    /// An n-gram size below 1.
    InvalidNGramSize(i64),
    /// A value that does not parse as an ISO-8601 date.
    UnreadableDate(String),
}

impl std::fmt::Display for RuleMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownStrategy(strategy) => write!(
                f,
                "unknown similarity strategy '{strategy}' — a rule that silently \
                 scored nothing would report a clean reconciliation"
            ),
            Self::InvalidNGramSize(n) => write!(f, "an n-gram size must be at least 1, got {n}"),
            Self::UnreadableDate(value) => write!(
                f,
                "'{value}' is not an ISO-8601 date — a date this module cannot read \
                 must not be guessed at"
            ),
        }
    }
}

impl std::error::Error for RuleMatchError {}

/// How two values are compared for a similarity band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimilarityStrategy {
    /// `1.0` if identical, `0.0` otherwise — lets a rule express "these must
    /// be identical" through the same mechanism as a fuzzy band, rather than
    /// a second one.
    Exact,
    /// Jaccard over n-grams, padded so short strings still have some. What
    /// makes a transposition visible: two identifiers with a swapped pair
    /// share almost every n-gram but differ in a few, where an exact
    /// comparison sees only "not equal".
    NGram { n: i64 },
}

/// How alike two values are, on `0.0..=1.0`.
///
/// # Errors
///
/// [`RuleMatchError::InvalidNGramSize`] for an n-gram size below 1.
#[must_use = "a similarity score that is never compared against a threshold was pointless to compute"]
pub fn similarity(
    left: &str,
    right: &str,
    strategy: &SimilarityStrategy,
) -> Result<f64, RuleMatchError> {
    match strategy {
        SimilarityStrategy::Exact => Ok(if left == right { 1.0 } else { 0.0 }),
        SimilarityStrategy::NGram { n } => ngram_similarity(left, right, *n),
    }
}

fn ngram_similarity(left: &str, right: &str, n: i64) -> Result<f64, RuleMatchError> {
    if n < 1 {
        return Err(RuleMatchError::InvalidNGramSize(n));
    }
    let n = usize::try_from(n).map_err(|_| RuleMatchError::InvalidNGramSize(n))?;

    // Short-circuited so identical values are exactly 1.0 rather than
    // 1.0-within-floating-point, which an `at_most` bound would then
    // sometimes admit and sometimes not.
    if left == right {
        return Ok(1.0);
    }

    let a = ngrams(left, n);
    let b = ngrams(right, n);
    let union = a.union(&b).count();
    if union == 0 {
        return Ok(0.0);
    }
    let intersection = a.intersection(&b).count();
    #[allow(clippy::cast_precision_loss)]
    Ok(intersection as f64 / union as f64)
}

/// The padded n-grams of a value. Padding matters at this length: a
/// 15-character identifier has 13 trigrams and a 3-character one has
/// exactly 1, so without padding the shortest values compare as
/// all-or-nothing.
fn ngrams(value: &str, n: usize) -> std::collections::HashSet<String> {
    let pad = n - 1;
    let chars: Vec<char> = std::iter::repeat_n('\u{2}', pad)
        .chain(value.chars())
        .chain(std::iter::repeat_n('\u{3}', pad))
        .collect();
    if chars.len() < n {
        return std::collections::HashSet::new();
    }
    (0..=chars.len() - n)
        .map(|i| chars[i..i + n].iter().collect())
        .collect()
}

/// A rule's `[findings.similarity]` band: how to compare, and the inclusive
/// range a score must fall in to pass.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarityBand {
    pub strategy: SimilarityStrategy,
    /// Inclusive lower bound.
    pub at_least: f64,
    /// Inclusive upper bound. **The load-bearing half** — without it, every
    /// *correctly* matched pair scores 1.0 and is reported as a suspected
    /// typo, which is the finding that makes a reviewer stop trusting the
    /// queue.
    pub at_most: f64,
}

/// # Errors
///
/// [`RuleMatchError::InvalidNGramSize`] for an n-gram size below 1.
pub fn passes_similarity(
    left: &str,
    right: &str,
    band: &SimilarityBand,
) -> Result<bool, RuleMatchError> {
    let score = similarity(left, right, &band.strategy)?;
    Ok(band.at_least <= score && score <= band.at_most)
}

/// What to do when a span's second event is unbound — the query's `OPTIONAL`
/// simply did not match anything for this row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenMissing {
    /// The condition does not fire. The safe default: a rule that treats
    /// every absent second event as a match would flag data that may
    /// simply not have happened yet.
    Ignore,
    /// The condition fires unconditionally — the absence itself is what the
    /// rule is looking for.
    Finding,
    /// Measure from the first event to `as_of` (or `today` if none is
    /// configured) instead of to a second event. **Usually the reading a
    /// span condition wants**: an invoice issued yesterday and not yet paid
    /// is not overdue, and treating it as a finding fills the queue with
    /// accusations about data that is simply not due yet.
    Elapsed,
}

/// A rule's `[findings.span]` condition.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanCondition {
    pub exceeds_days: i64,
    pub when_missing: WhenMissing,
    /// The date an `Elapsed` measurement is taken from. `None` means "use
    /// the caller's `today`" — kept as an explicit choice on the condition
    /// rather than read from the clock inside this function, so the same
    /// input always produces the same answer.
    pub as_of: Option<NaiveDate>,
}

fn parse_date(value: &str) -> Result<NaiveDate, RuleMatchError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| RuleMatchError::UnreadableDate(value.to_string()))
}

/// Whether a span condition fires.
///
/// `start` must already be a bound value — the caller is responsible for
/// treating an unbound *first* event as a rule-configuration error, the same
/// way an unbound subject variable is (a query edited to rename a variable
/// must not fail silently). `end` is `Some` when the query bound the second
/// event for this row, `None` when an `OPTIONAL` did not match.
///
/// # Errors
///
/// [`RuleMatchError::UnreadableDate`] if `start`, `end`, or `condition.as_of`
/// is not an ISO-8601 date. Never silently treated as absent — a malformed
/// date quietly becoming "no second event" would turn a data-quality problem
/// into a fabricated compliance finding.
pub fn passes_span(
    start: &str,
    end: Option<&str>,
    condition: &SpanCondition,
    today: NaiveDate,
) -> Result<bool, RuleMatchError> {
    let start = parse_date(start)?;

    let Some(end) = end else {
        return match condition.when_missing {
            WhenMissing::Finding => Ok(true),
            WhenMissing::Ignore => Ok(false),
            WhenMissing::Elapsed => {
                let judged_on = condition.as_of.unwrap_or(today);
                Ok((judged_on - start).num_days() > condition.exceeds_days)
            }
        };
    };

    let end = parse_date(end)?;
    // Strictly greater: "within 180 days" includes the 180th, and a rule
    // that fired on it would accuse someone on the last day they were
    // compliant.
    Ok((end - start).num_days() > condition.exceeds_days)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ngram(n: i64) -> SimilarityStrategy {
        SimilarityStrategy::NGram { n }
    }

    // ---- Display: an error that renders nothing is worse than none ----

    #[test]
    fn each_error_variant_names_what_went_wrong() {
        assert!(
            RuleMatchError::UnknownStrategy("zzz".to_string())
                .to_string()
                .contains("zzz")
        );
        assert!(
            RuleMatchError::InvalidNGramSize(0)
                .to_string()
                .contains('0')
        );
        assert!(
            RuleMatchError::UnreadableDate("nope".to_string())
                .to_string()
                .contains("nope")
        );
    }

    // ---- similarity: exact ----

    #[test]
    fn exact_strategy_scores_one_for_identical_values_and_zero_otherwise() {
        assert_eq!(similarity("a", "a", &SimilarityStrategy::Exact), Ok(1.0));
        assert_eq!(similarity("a", "b", &SimilarityStrategy::Exact), Ok(0.0));
    }

    // ---- similarity: ngram, pinned to the pack's own real fixture numbers ----

    #[test]
    fn ngram_similarity_reproduces_the_packs_own_transposition_score() {
        // packs/gst/pack.toml's own comment: "the planted transposition
        // scores 0.619 at n=3" — the real claimed/filed GSTINs from
        // packs/gst/fixtures/purchase-register.ttl and gstr2b.ttl (INV-1004),
        // differing only by the trailing "ZM"/"MZ" transposition.
        let claimed = "27AABCU9603R1ZM";
        let filed = "27AABCU9603R1MZ";
        let score = similarity(claimed, filed, &ngram(3)).unwrap();
        assert!((score - 0.619_047_619).abs() < 1e-6, "score={score}");
    }

    #[test]
    fn ngram_similarity_scores_genuinely_different_values_far_below_the_transposition() {
        let claimed = "27AABCU9603R1ZM";
        let unrelated = "29AACCG0527D1Z8";
        let transposed_score = similarity(claimed, "27AABCU9603R1MZ", &ngram(3)).unwrap();
        let unrelated_score = similarity(claimed, unrelated, &ngram(3)).unwrap();
        assert!(
            unrelated_score < transposed_score,
            "unrelated={unrelated_score} transposed={transposed_score}"
        );
    }

    #[test]
    fn ngram_similarity_short_circuits_identical_values_to_exactly_one() {
        // Not "close to 1.0" — the manifest's `at_most = 0.999` depends on
        // an identical pair landing outside the band, not just near its edge.
        assert_eq!(similarity("same", "same", &ngram(3)), Ok(1.0));
    }

    #[test]
    fn ngram_similarity_handles_strings_shorter_than_n() {
        // A 1- or 2-character value must not panic or divide by zero; it is
        // simply compared on whatever padded n-grams it has.
        let score = similarity("a", "ab", &ngram(3)).unwrap();
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn ngrams_computes_a_real_ngram_when_the_padded_length_exactly_equals_n() {
        // At n=1 there is no padding, so a 1-character value pads to exactly
        // `chars.len() == n` — the boundary the underflow guard sits on. It
        // must still produce one real n-gram, not an empty set: "a" and "ab"
        // share the "a" 1-gram, which only shows up if the guard's `<`
        // (rather than `<=` or `==`) is exactly right.
        let score = similarity("a", "ab", &ngram(1)).unwrap();
        assert!((score - 0.5).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn ngrams_returns_empty_only_when_strictly_shorter_than_n() {
        // n=1 with an empty string is the only input that ever makes the
        // padded length strictly less than n (padding is `2*(n-1)`, so for
        // n>=2 a non-empty value already meets or exceeds n). Exercising it
        // is what makes `<` distinguishable from `==`.
        let score = similarity("", "a", &ngram(1)).unwrap();
        assert!((score - 0.0).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn ngram_similarity_accepts_a_size_of_exactly_one() {
        // The boundary the `n < 1` guard sits on: 1 is the smallest valid
        // size and must not be refused alongside 0.
        assert!(similarity("a", "b", &ngram(1)).is_ok());
    }

    #[test]
    fn ngram_similarity_refuses_a_size_below_one() {
        assert_eq!(
            similarity("a", "b", &ngram(0)),
            Err(RuleMatchError::InvalidNGramSize(0))
        );
        assert_eq!(
            similarity("a", "b", &ngram(-1)),
            Err(RuleMatchError::InvalidNGramSize(-1))
        );
    }

    // ---- passes_similarity: the band ----

    #[test]
    fn passes_similarity_band_is_inclusive_at_both_ends() {
        let band = SimilarityBand {
            strategy: SimilarityStrategy::Exact,
            at_least: 1.0,
            at_most: 1.0,
        };
        assert_eq!(passes_similarity("a", "a", &band), Ok(true));
        assert_eq!(passes_similarity("a", "b", &band), Ok(false));
    }

    #[test]
    fn passes_similarity_at_most_excludes_a_perfect_match() {
        // The load-bearing half of the band: without `at_most`, every
        // correctly matched pair (score 1.0) would pass a transposition
        // rule and flag every correct invoice as a suspected typo.
        let band = SimilarityBand {
            strategy: ngram(3),
            at_least: 0.40,
            at_most: 0.999,
        };
        assert_eq!(passes_similarity("same", "same", &band), Ok(false));
    }

    #[test]
    fn passes_similarity_at_least_excludes_a_weak_match() {
        let band = SimilarityBand {
            strategy: ngram(3),
            at_least: 0.90,
            at_most: 0.999,
        };
        assert_eq!(
            passes_similarity("27AABCU9603R1ZM", "29AACCG0527D1Z8", &band),
            Ok(false)
        );
    }

    #[test]
    fn passes_similarity_propagates_an_unknown_ngram_size() {
        let band = SimilarityBand {
            strategy: ngram(0),
            at_least: 0.0,
            at_most: 1.0,
        };
        assert!(passes_similarity("a", "b", &band).is_err());
    }

    // ---- passes_span: exact two-event case ----

    #[test]
    fn passes_span_fires_only_when_strictly_exceeding_the_day_count() {
        let condition = SpanCondition {
            exceeds_days: 180,
            when_missing: WhenMissing::Ignore,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        // exactly 180 days apart: must NOT fire — the 180th day is still
        // within the window, and a rule firing on it would accuse someone
        // on the last day they were compliant.
        assert_eq!(
            passes_span("2026-01-01", Some("2026-06-30"), &condition, today),
            Ok(false)
        );
        // 181 days apart: fires.
        assert_eq!(
            passes_span("2026-01-01", Some("2026-07-01"), &condition, today),
            Ok(true)
        );
    }

    // ---- passes_span: when_missing semantics ----

    #[test]
    fn passes_span_when_missing_ignore_does_not_fire() {
        let condition = SpanCondition {
            exceeds_days: 0,
            when_missing: WhenMissing::Ignore,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(
            passes_span("2020-01-01", None, &condition, today),
            Ok(false)
        );
    }

    #[test]
    fn passes_span_when_missing_finding_always_fires() {
        let condition = SpanCondition {
            exceeds_days: 999_999,
            when_missing: WhenMissing::Finding,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(passes_span("2026-06-01", None, &condition, today), Ok(true));
    }

    #[test]
    fn passes_span_when_missing_elapsed_measures_from_as_of_when_given() {
        let condition = SpanCondition {
            exceeds_days: 180,
            when_missing: WhenMissing::Elapsed,
            as_of: Some(NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()),
        };
        // today is deliberately different from as_of, to prove as_of wins.
        let today = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        // 2026-01-01 to as_of 2026-08-01 is 212 days: fires.
        assert_eq!(passes_span("2026-01-01", None, &condition, today), Ok(true));
    }

    #[test]
    fn passes_span_when_missing_elapsed_does_not_fire_at_exactly_the_day_count() {
        // The same strictly-greater boundary as the two-event case, for the
        // elapsed reading: exactly 180 days from the anchor to `as_of` must
        // not fire.
        let condition = SpanCondition {
            exceeds_days: 180,
            when_missing: WhenMissing::Elapsed,
            as_of: Some(NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()),
        };
        let today = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        assert_eq!(
            passes_span("2026-01-01", None, &condition, today),
            Ok(false)
        );
    }

    #[test]
    fn passes_span_when_missing_elapsed_falls_back_to_today_without_as_of() {
        let condition = SpanCondition {
            exceeds_days: 180,
            when_missing: WhenMissing::Elapsed,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        assert_eq!(passes_span("2026-01-01", None, &condition, today), Ok(true));
        let recent_today = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        assert_eq!(
            passes_span("2026-01-01", None, &condition, recent_today),
            Ok(false)
        );
    }

    // ---- passes_span: malformed dates are never guessed at ----

    #[test]
    fn passes_span_refuses_an_unreadable_start_date() {
        let condition = SpanCondition {
            exceeds_days: 0,
            when_missing: WhenMissing::Ignore,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            passes_span("not-a-date", Some("2026-01-01"), &condition, today),
            Err(RuleMatchError::UnreadableDate("not-a-date".to_string()))
        );
    }

    #[test]
    fn passes_span_refuses_an_unreadable_end_date() {
        let condition = SpanCondition {
            exceeds_days: 0,
            when_missing: WhenMissing::Ignore,
            as_of: None,
        };
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            passes_span("2026-01-01", Some("not-a-date"), &condition, today),
            Err(RuleMatchError::UnreadableDate("not-a-date".to_string()))
        );
    }
}
