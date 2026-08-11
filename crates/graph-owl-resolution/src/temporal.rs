//! Entity validity-period resolution — Epic 105 P8's temporal engine (the
//! platform plan's §7: "`effectiveFrom`/`effectiveTo` on entities... and
//! time-travel queries that already exist at assertion-time (`as_of`)
//! extended to entity validity").
//!
//! Generalizes what `packs/gst/queries/amount-mismatch.sparql` already does
//! ad hoc — "which provision was in force on this invoice date," resolved
//! by comparing `effectiveFrom` strings lexicographically inside SPARQL,
//! because ISO-8601 sorts correctly as text — into a Rust primitive any pack
//! can reuse for any dated-entity concept: a statutory provision, a policy
//! version, a contract term, a price list. Same reason the span check in
//! [`crate::rule_match`] lives here rather than beside the query engine:
//! measured against the real engine, date arithmetic and comparison inside
//! a SPARQL expression evaluate to unbound. The join (find the candidate
//! periods) is what a query language is good at; resolving which one wins
//! is not, and stays Rust.

use chrono::NaiveDate;

/// One candidate validity window and the value it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePeriod<T> {
    /// The window opens on this date, inclusive.
    pub effective_from: NaiveDate,
    /// The window closes on this date, exclusive. `None` means still open —
    /// the common case in a law/policy history, where a provision is
    /// superseded by the next one's `effective_from` rather than carrying
    /// its own end date.
    pub effective_to: Option<NaiveDate>,
    /// What this period stands for — a cap percentage, a policy id, a
    /// `Sid`, whatever the caller's own domain names. Generic rather than a
    /// fixed type because a law's provision and a price list's rate are the
    /// same shape at this level and different at every level above it.
    pub value: T,
}

/// Which candidate period is in force at `at`, if any.
///
/// **The latest-starting period that has already started and has not yet
/// ended wins.** Two periods should never both be in force for the same
/// concept at the same instant in a well-formed history; if the caller's
/// data has overlapping windows anyway, this resolves by latest start
/// rather than refusing to answer — the same posture
/// [`crate::rule_match::passes_span`] takes toward a query that
/// over-fetches: resolution, not refusal, is this module's job.
#[must_use]
pub fn in_force_at<T>(
    periods: &[EffectivePeriod<T>],
    at: NaiveDate,
) -> Option<&EffectivePeriod<T>> {
    periods
        .iter()
        .filter(|p| p.effective_from <= at && p.effective_to.is_none_or(|end| at < end))
        .max_by_key(|p| p.effective_from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("valid fixture date")
    }

    /// `packs/gst/law/rule-36-4.ttl`'s own real shape: four open-ended
    /// provisions, each superseded by the next one's start date, no
    /// `effective_to` on any of them.
    fn rule_36_4_caps() -> Vec<EffectivePeriod<&'static str>> {
        vec![
            EffectivePeriod {
                effective_from: date("2019-10-09"),
                effective_to: None,
                value: "20",
            },
            EffectivePeriod {
                effective_from: date("2020-01-01"),
                effective_to: None,
                value: "10",
            },
            EffectivePeriod {
                effective_from: date("2021-01-01"),
                effective_to: None,
                value: "5",
            },
            EffectivePeriod {
                effective_from: date("2022-01-01"),
                effective_to: None,
                value: "0",
            },
        ]
    }

    #[test]
    fn a_2020_invoice_resolves_to_the_10_percent_cap() {
        let caps = rule_36_4_caps();
        let resolved = in_force_at(&caps, date("2020-07-12"));
        assert_eq!(resolved.map(|p| p.value), Some("10"), "{resolved:?}");
    }

    #[test]
    fn a_2026_invoice_resolves_to_the_nil_cap_from_2022() {
        let caps = rule_36_4_caps();
        let resolved = in_force_at(&caps, date("2026-08-01"));
        assert_eq!(resolved.map(|p| p.value), Some("0"), "{resolved:?}");
    }

    #[test]
    fn a_date_before_any_period_started_resolves_to_nothing() {
        let caps = rule_36_4_caps();
        let resolved = in_force_at(&caps, date("2019-01-01"));
        assert_eq!(resolved, None, "{resolved:?}");
    }

    #[test]
    fn the_start_date_itself_is_already_in_force_inclusive() {
        let caps = rule_36_4_caps();
        let resolved = in_force_at(&caps, date("2021-01-01"));
        assert_eq!(resolved.map(|p| p.value), Some("5"), "{resolved:?}");
    }

    #[test]
    fn an_explicit_end_date_excludes_that_day_itself_exclusive() {
        // A closed window, unlike rule-36-4's own open-ended provisions —
        // proves the primitive handles both styles, not just the one
        // fixture GST happens to use.
        let periods = vec![
            EffectivePeriod {
                effective_from: date("2024-01-01"),
                effective_to: Some(date("2024-07-01")),
                value: "spring",
            },
            EffectivePeriod {
                effective_from: date("2024-07-01"),
                effective_to: None,
                value: "summer",
            },
        ];
        assert_eq!(
            in_force_at(&periods, date("2024-06-30")).map(|p| p.value),
            Some("spring")
        );
        assert_eq!(
            in_force_at(&periods, date("2024-07-01")).map(|p| p.value),
            Some("summer"),
            "the end date is exclusive — the day the next window opens \
             belongs to the next window, not both"
        );
    }

    #[test]
    fn a_date_past_a_closed_window_with_nothing_after_resolves_to_nothing() {
        let periods = vec![EffectivePeriod {
            effective_from: date("2024-01-01"),
            effective_to: Some(date("2024-07-01")),
            value: "spring",
        }];
        assert_eq!(in_force_at(&periods, date("2024-08-01")), None);
    }

    /// Isolates the end-date boundary from `an_explicit_end_date_excludes_
    /// that_day_itself_exclusive`'s own tie-breaking: that test has a
    /// second period starting exactly where the first ends, so an
    /// off-by-one on the end comparison is invisible — the later-starting
    /// period wins the tie-break either way. With nothing superseding it,
    /// only a truly exclusive end date resolves to `None` here.
    #[test]
    fn querying_exactly_a_closed_window_s_end_date_with_nothing_after_resolves_to_nothing() {
        let periods = vec![EffectivePeriod {
            effective_from: date("2024-01-01"),
            effective_to: Some(date("2024-07-01")),
            value: "spring",
        }];
        assert_eq!(in_force_at(&periods, date("2024-07-01")), None);
    }

    #[test]
    fn no_candidate_periods_resolves_to_nothing_not_a_panic() {
        let periods: Vec<EffectivePeriod<&str>> = vec![];
        assert_eq!(in_force_at(&periods, date("2024-01-01")), None);
    }

    /// The generalization proof — a domain with no relationship to GST at
    /// all, the same discipline `plans/105-domain-neutrality.md`'s
    /// hospitality proof-pack already applied to blocking and matching.
    /// Freight rate cards, not tax law: same shape, unrelated domain.
    #[test]
    fn resolves_correctly_for_a_domain_with_no_relationship_to_gst() {
        let freight_rates = vec![
            EffectivePeriod {
                effective_from: date("2023-01-01"),
                effective_to: Some(date("2024-01-01")),
                value: 42.50_f64,
            },
            EffectivePeriod {
                effective_from: date("2024-01-01"),
                effective_to: None,
                value: 47.00_f64,
            },
        ];
        assert_eq!(
            in_force_at(&freight_rates, date("2023-06-15")).map(|p| p.value),
            Some(42.50)
        );
        assert_eq!(
            in_force_at(&freight_rates, date("2025-01-01")).map(|p| p.value),
            Some(47.00)
        );
    }
}
