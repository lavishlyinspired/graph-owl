//! What an agent should believe about an asset — Epic 14 Slice B.
//!
//! **The most damaging bug this file can have is reporting confidence nobody
//! earned.** An agent told an asset is `Healthy` builds on it; an agent told
//! `Unknown` says so in its answer. Every default here therefore leans toward
//! admitting ignorance, and the tests are written to catch the opposite.
//!
//! Pure, and `now` is a parameter rather than a call to the clock — an expiry
//! rule whose boundary cannot be tested is an expiry rule nobody has checked.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Where an asset is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Lifecycle {
    /// Nobody said. **Not** an assumption of production.
    Unknown,
    Draft,
    Production,
    /// Retired, and what replaced it.
    ///
    /// The successor is the actionable half: an agent told only "deprecated"
    /// reports a dead end, where one told the successor can carry on.
    Deprecated {
        successor: Option<String>,
    },
}

/// Whether somebody vouched for this asset, and whether that still holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Certification {
    /// Never certified. Distinct from a certification that lapsed — one was
    /// never vouched for, the other was and no longer is, and only the second
    /// tells an agent somebody once cared.
    None,
    Certified {
        by: String,
        #[serde(rename = "expiresAt")]
        expires_at: Option<DateTime<Utc>>,
    },
    /// Certified once, and the certification has run out.
    Expired {
        by: String,
        #[serde(rename = "expiredAt")]
        expired_at: DateTime<Utc>,
    },
}

/// What the tests say, if anything ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    /// **The default, and the only safe one.** No test result is not a pass.
    Unknown,
    Healthy,
    Unhealthy,
    /// Tests exist and last ran too long ago to speak for the asset now.
    Stale,
}

/// Something an agent should know is missing.
///
/// Named rather than implied by an absent field. An agent shown a partial
/// record assumes the rest is fine; one told "no owner, no tests" answers
/// differently, and that difference is the point of this whole summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Gap {
    NoOwner,
    NoDescription,
    NoTests,
    StaleResults,
    NoCertification,
    ExpiredCertification,
    NoLineage,
}

/// What the catalog holds about an asset, before any of it is judged.
///
/// Options throughout, because "we do not know" is the state this file exists
/// to preserve. A field that defaulted on the way in would arrive here already
/// having lost the distinction.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    pub lifecycle: Option<String>,
    pub successor: Option<String>,
    pub owner: Option<String>,
    pub description: Option<String>,
    pub certified_by: Option<String>,
    pub certification_expires_at: Option<DateTime<Utc>>,
    /// `None` means no test has ever run. `Some(false)` means one ran and
    /// failed — opposite meanings that a boolean alone cannot hold.
    pub tests_passing: Option<bool>,
    pub tests_last_run_at: Option<DateTime<Utc>>,
    pub has_lineage: bool,
}

/// What an agent is told.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustSummary {
    pub lifecycle: Lifecycle,
    pub certification: Certification,
    pub quality: Health,
    pub owner_known: bool,
    pub last_verified_at: Option<DateTime<Utc>>,
    /// Empty **only** when genuinely complete.
    pub gaps: Vec<Gap>,
}

/// How old a test result may be before it stops speaking for the asset.
///
/// Seven days: long enough that a weekly quality job keeps an asset green, and
/// short enough that a result from a fortnight ago is not presented as current.
/// Anything shorter would mark a healthy weekly-tested estate stale on the
/// seventh day, which trains readers to ignore the field.
pub const STALE_AFTER_DAYS: i64 = 7;

fn lifecycle_of(observed: &Observed) -> Lifecycle {
    match observed.lifecycle.as_deref() {
        Some("draft") => Lifecycle::Draft,
        Some("production") => Lifecycle::Production,
        Some("deprecated") => Lifecycle::Deprecated {
            successor: observed.successor.clone(),
        },
        // An unrecognised value is *not* production. A vocabulary this build
        // does not know is a reason to admit ignorance, not to guess the
        // reassuring option.
        _ => Lifecycle::Unknown,
    }
}

fn certification_of(observed: &Observed, now: DateTime<Utc>) -> Certification {
    let Some(by) = observed.certified_by.clone() else {
        return Certification::None;
    };
    match observed.certification_expires_at {
        // **Expiry is evaluated, not merely carried.** A summary that reports
        // the date and lets the reader compare is a summary an agent will get
        // wrong, and it will get it wrong in the confident direction.
        Some(expires_at) if expires_at <= now => Certification::Expired {
            by,
            expired_at: expires_at,
        },
        expires_at => Certification::Certified { by, expires_at },
    }
}

fn health_of(observed: &Observed, now: DateTime<Utc>) -> Health {
    // No result is not a pass. This is the single most damaging default in the
    // file to get wrong, because it manufactures confidence out of silence.
    let Some(passing) = observed.tests_passing else {
        return Health::Unknown;
    };
    if !passing {
        // A failure is a failure however old it is. Ageing it into `Stale`
        // would quietly downgrade the one state an agent must not build on.
        return Health::Unhealthy;
    }
    match observed.tests_last_run_at {
        // A pass with no timestamp cannot be shown to be current, and a pass
        // that cannot be dated is a pass that might be from last year.
        None => Health::Stale,
        Some(ran) if (now - ran).num_days() >= STALE_AFTER_DAYS => Health::Stale,
        Some(_) => Health::Healthy,
    }
}

/// Everything an agent should know is missing.
///
/// Order is fixed so two summaries of the same asset compare equal and a diff
/// between runs means something.
fn gaps_of(observed: &Observed, certification: &Certification, quality: Health) -> Vec<Gap> {
    let mut gaps = Vec::new();
    if observed.owner.is_none() {
        gaps.push(Gap::NoOwner);
    }
    // A present-but-empty description is not a description. Storing `""` is how
    // a required field gets satisfied without being answered.
    if observed
        .description
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        gaps.push(Gap::NoDescription);
    }
    match quality {
        Health::Unknown => gaps.push(Gap::NoTests),
        Health::Stale => gaps.push(Gap::StaleResults),
        _ => {}
    }
    match certification {
        Certification::None => gaps.push(Gap::NoCertification),
        Certification::Expired { .. } => gaps.push(Gap::ExpiredCertification),
        Certification::Certified { .. } => {}
    }
    if !observed.has_lineage {
        gaps.push(Gap::NoLineage);
    }
    gaps
}

/// Judge what the catalog holds, as of `now`.
#[must_use]
pub fn summarise(observed: &Observed, now: DateTime<Utc>) -> TrustSummary {
    let certification = certification_of(observed, now);
    let quality = health_of(observed, now);
    TrustSummary {
        lifecycle: lifecycle_of(observed),
        gaps: gaps_of(observed, &certification, quality),
        certification,
        quality,
        owner_known: observed.owner.is_some(),
        last_verified_at: observed.tests_last_run_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-30T12:00:00Z")
            .expect("a fixed instant")
            .with_timezone(&Utc)
    }

    /// An asset with nothing recorded — the state most of a real estate is in.
    fn bare() -> Observed {
        Observed::default()
    }

    /// An asset with everything a governed one should have.
    fn complete() -> Observed {
        Observed {
            lifecycle: Some("production".into()),
            successor: None,
            owner: Some("finance".into()),
            description: Some("customer orders".into()),
            certified_by: Some("data-governance".into()),
            certification_expires_at: Some(now() + Duration::days(30)),
            tests_passing: Some(true),
            tests_last_run_at: Some(now() - Duration::hours(2)),
            has_lineage: true,
        }
    }

    mod silence_is_not_confidence {
        use super::*;

        /// **The most damaging bug this file could have.** An agent told
        /// `Healthy` builds on the asset. Reporting it for something nobody
        /// ever tested manufactures confidence out of an absence.
        #[test]
        fn an_asset_nobody_tested_reports_unknown_never_healthy() {
            let summary = summarise(&bare(), now());

            assert_eq!(summary.quality, Health::Unknown);
            assert!(summary.gaps.contains(&Gap::NoTests));
        }

        /// And the negative that stops "always Unknown" passing: a recent pass
        /// is reported as healthy.
        #[test]
        fn a_recent_pass_is_reported_as_healthy() {
            let summary = summarise(&complete(), now());

            assert_eq!(summary.quality, Health::Healthy);
            assert!(!summary.gaps.contains(&Gap::NoTests));
        }

        /// `None` and `Some(false)` are opposite statements — "never ran" and
        /// "ran and failed" — and a boolean alone cannot hold both.
        #[test]
        fn a_failure_is_distinguished_from_never_having_run() {
            let failing = Observed {
                tests_passing: Some(false),
                tests_last_run_at: Some(now()),
                ..bare()
            };

            assert_eq!(summarise(&failing, now()).quality, Health::Unhealthy);
            assert_eq!(summarise(&bare(), now()).quality, Health::Unknown);
        }

        /// A failure stays a failure however old it is. Ageing it into `Stale`
        /// would quietly downgrade the one state an agent must not build on.
        #[test]
        fn an_old_failure_does_not_decay_into_merely_stale() {
            let old_failure = Observed {
                tests_passing: Some(false),
                tests_last_run_at: Some(now() - Duration::days(90)),
                ..bare()
            };

            assert_eq!(summarise(&old_failure, now()).quality, Health::Unhealthy);
        }

        /// A pass nobody dated cannot be shown to be current — it might be from
        /// last year.
        #[test]
        fn a_pass_with_no_timestamp_is_stale_rather_than_healthy() {
            let undated = Observed {
                tests_passing: Some(true),
                tests_last_run_at: None,
                ..bare()
            };

            assert_eq!(summarise(&undated, now()).quality, Health::Stale);
        }

        /// The staleness boundary, both sides. A result exactly at the limit is
        /// stale; one an hour younger is not.
        #[test]
        fn the_staleness_boundary_is_where_it_says_it_is() {
            let at = Observed {
                tests_passing: Some(true),
                tests_last_run_at: Some(now() - Duration::days(STALE_AFTER_DAYS)),
                ..bare()
            };
            let just_inside = Observed {
                tests_last_run_at: Some(
                    now() - Duration::days(STALE_AFTER_DAYS) + Duration::hours(1),
                ),
                ..at.clone()
            };

            assert_eq!(summarise(&at, now()).quality, Health::Stale);
            assert_eq!(summarise(&just_inside, now()).quality, Health::Healthy);
        }

        #[test]
        fn a_stale_result_is_reported_as_a_gap() {
            let stale = Observed {
                tests_passing: Some(true),
                tests_last_run_at: Some(now() - Duration::days(30)),
                ..complete()
            };

            let summary = summarise(&stale, now());

            assert_eq!(summary.quality, Health::Stale);
            assert!(summary.gaps.contains(&Gap::StaleResults));
            assert!(
                !summary.gaps.contains(&Gap::NoTests),
                "tests exist; they are merely old"
            );
        }
    }

    mod certification_is_evaluated_not_merely_carried {
        use super::*;

        #[test]
        fn a_live_certification_is_reported_as_certified() {
            let summary = summarise(&complete(), now());

            assert!(
                matches!(summary.certification, Certification::Certified { .. }),
                "{:?}",
                summary.certification
            );
            assert!(!summary.gaps.contains(&Gap::ExpiredCertification));
        }

        /// **A summary that reports the date and lets the reader compare is a
        /// summary an agent will get wrong** — and it will get it wrong in the
        /// confident direction.
        #[test]
        fn a_lapsed_certification_is_reported_as_expired_not_certified() {
            let lapsed = Observed {
                certification_expires_at: Some(now() - Duration::days(1)),
                ..complete()
            };

            let summary = summarise(&lapsed, now());

            assert!(
                matches!(summary.certification, Certification::Expired { .. }),
                "{:?}",
                summary.certification
            );
            assert!(summary.gaps.contains(&Gap::ExpiredCertification));
        }

        /// The boundary, at exactly the expiry instant. A certification that
        /// expires *at* an instant is not valid at that instant.
        #[test]
        fn a_certification_expiring_exactly_now_is_expired() {
            let at_boundary = Observed {
                certification_expires_at: Some(now()),
                ..complete()
            };
            let a_second_later = Observed {
                certification_expires_at: Some(now() + Duration::seconds(1)),
                ..complete()
            };

            assert!(matches!(
                summarise(&at_boundary, now()).certification,
                Certification::Expired { .. }
            ));
            assert!(matches!(
                summarise(&a_second_later, now()).certification,
                Certification::Certified { .. }
            ));
        }

        /// Never certified is not the same as lapsed: one was never vouched
        /// for, the other was and no longer is, and only the second tells an
        /// agent somebody once cared.
        #[test]
        fn never_certified_is_distinct_from_expired() {
            let never = summarise(&bare(), now());
            let lapsed = summarise(
                &Observed {
                    certification_expires_at: Some(now() - Duration::days(1)),
                    ..complete()
                },
                now(),
            );

            assert_eq!(never.certification, Certification::None);
            assert!(never.gaps.contains(&Gap::NoCertification));
            assert!(!never.gaps.contains(&Gap::ExpiredCertification));
            assert!(lapsed.gaps.contains(&Gap::ExpiredCertification));
            assert!(!lapsed.gaps.contains(&Gap::NoCertification));
        }

        /// A certification with no expiry does not expire. Treating a missing
        /// date as "expired" would retire every permanently-certified asset.
        #[test]
        fn a_certification_with_no_expiry_stays_valid() {
            let forever = Observed {
                certification_expires_at: None,
                ..complete()
            };

            assert!(matches!(
                summarise(&forever, now()).certification,
                Certification::Certified {
                    expires_at: None,
                    ..
                }
            ));
        }

        /// Who vouched survives into the summary — an agent weighing a
        /// certification wants to know whose it is.
        #[test]
        fn the_certifier_is_named() {
            let Certification::Certified { by, .. } = summarise(&complete(), now()).certification
            else {
                panic!("expected a certification")
            };

            assert_eq!(by, "data-governance");
        }
    }

    mod lifecycle {
        use super::*;

        #[test]
        fn each_known_state_is_read() {
            for (raw, expected) in [
                ("draft", Lifecycle::Draft),
                ("production", Lifecycle::Production),
            ] {
                let observed = Observed {
                    lifecycle: Some(raw.into()),
                    ..bare()
                };
                assert_eq!(summarise(&observed, now()).lifecycle, expected, "{raw}");
            }
        }

        /// **The successor is the actionable half.** An agent told only
        /// "deprecated" reports a dead end; one told what replaced it carries
        /// on.
        #[test]
        fn a_deprecated_asset_names_its_successor() {
            let deprecated = Observed {
                lifecycle: Some("deprecated".into()),
                successor: Some("warehouse.orders_v2".into()),
                ..complete()
            };

            assert_eq!(
                summarise(&deprecated, now()).lifecycle,
                Lifecycle::Deprecated {
                    successor: Some("warehouse.orders_v2".into())
                }
            );
        }

        /// And deprecation without a known successor still reports the
        /// deprecation — withholding it because the successor is unknown would
        /// hide the more important half.
        #[test]
        fn a_deprecated_asset_with_no_successor_still_reports_deprecation() {
            let deprecated = Observed {
                lifecycle: Some("deprecated".into()),
                successor: None,
                ..complete()
            };

            assert_eq!(
                summarise(&deprecated, now()).lifecycle,
                Lifecycle::Deprecated { successor: None }
            );
        }

        /// **An unrecognised value is not production.** A vocabulary this build
        /// does not know is a reason to admit ignorance, not to guess the
        /// reassuring option.
        #[test]
        fn an_unrecognised_lifecycle_is_unknown_rather_than_production() {
            let odd = Observed {
                lifecycle: Some("gamma-preview".into()),
                ..complete()
            };

            assert_eq!(summarise(&odd, now()).lifecycle, Lifecycle::Unknown);
        }

        #[test]
        fn an_absent_lifecycle_is_unknown() {
            assert_eq!(summarise(&bare(), now()).lifecycle, Lifecycle::Unknown);
        }
    }

    mod what_is_missing_is_said_aloud {
        use super::*;

        /// An agent shown a partial record assumes the rest is fine. This is
        /// the whole reason gaps are named rather than implied by absence.
        #[test]
        fn a_bare_asset_reports_every_gap() {
            let summary = summarise(&bare(), now());

            for expected in [
                Gap::NoOwner,
                Gap::NoDescription,
                Gap::NoTests,
                Gap::NoCertification,
                Gap::NoLineage,
            ] {
                assert!(
                    summary.gaps.contains(&expected),
                    "{expected:?} missing from {:?}",
                    summary.gaps
                );
            }
        }

        /// **Empty only when genuinely complete.** A gaps list that is usually
        /// empty is a field nobody checks.
        #[test]
        fn a_complete_asset_reports_no_gaps() {
            let summary = summarise(&complete(), now());

            assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
        }

        /// Each gap appears for its own reason. Removing one thing from a
        /// complete asset produces exactly one gap — a list that reacted to the
        /// wrong field would still look plausible.
        #[test]
        fn each_gap_is_raised_by_its_own_absence() {
            let cases: [(&str, Observed, Gap); 5] = [
                (
                    "owner",
                    Observed {
                        owner: None,
                        ..complete()
                    },
                    Gap::NoOwner,
                ),
                (
                    "description",
                    Observed {
                        description: None,
                        ..complete()
                    },
                    Gap::NoDescription,
                ),
                (
                    "tests",
                    Observed {
                        tests_passing: None,
                        ..complete()
                    },
                    Gap::NoTests,
                ),
                (
                    "certification",
                    Observed {
                        certified_by: None,
                        ..complete()
                    },
                    Gap::NoCertification,
                ),
                (
                    "lineage",
                    Observed {
                        has_lineage: false,
                        ..complete()
                    },
                    Gap::NoLineage,
                ),
            ];

            for (name, observed, expected) in cases {
                let gaps = summarise(&observed, now()).gaps;
                assert_eq!(gaps, vec![expected], "removing {name} produced {gaps:?}");
            }
        }

        /// A blank description is not a description. Storing `""` is how a
        /// required field gets satisfied without being answered.
        #[test]
        fn a_blank_description_counts_as_missing() {
            for blank in ["", "   ", "\n"] {
                let observed = Observed {
                    description: Some(blank.into()),
                    ..complete()
                };
                assert!(
                    summarise(&observed, now())
                        .gaps
                        .contains(&Gap::NoDescription),
                    "{blank:?} was accepted as a description"
                );
            }
        }

        /// Gaps come out in a fixed order, so two summaries of the same asset
        /// compare equal and a diff between runs means something.
        #[test]
        fn the_same_asset_summarises_identically_twice() {
            assert_eq!(summarise(&bare(), now()), summarise(&bare(), now()));
        }

        #[test]
        fn ownership_is_reported_as_a_flag_as_well_as_a_gap() {
            assert!(!summarise(&bare(), now()).owner_known);
            assert!(summarise(&complete(), now()).owner_known);
        }

        #[test]
        fn the_wire_shape_is_camel_case_and_names_the_state() {
            let json = serde_json::to_value(summarise(&bare(), now())).expect("serialises");

            assert_eq!(json["quality"], "unknown");
            assert_eq!(json["lifecycle"]["state"], "unknown");
            assert_eq!(json["certification"]["state"], "none");
            assert!(json["ownerKnown"].is_boolean(), "{json}");
            assert!(json["gaps"].is_array(), "{json}");
        }
    }
}
