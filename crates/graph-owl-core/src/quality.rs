//! Quality signals — Epic 30.
//!
//! **The boundary, stated plainly**: graph-owl ingests and displays test
//! results produced elsewhere — dbt, Great Expectations, custom checks. It does
//! not run tests, author assertions, or schedule checks. Those are a product in
//! their own right with their own compute story, and building them would
//! dominate the roadmap.
//!
//! **Health is derived, never stored** (decision 1), and the two ways it can
//! lie are the two this module is shaped to refuse:
//!
//! - **Silence is not a pass.** An asset with no tests is `Unknown`, never
//!   `Healthy` (decision 5). Reporting health for something nobody checked is
//!   the most dangerous bug available here, because it asserts trust nobody
//!   earned and does it silently.
//! - **An old pass is not a pass.** A result from six weeks ago against a daily
//!   cadence is `Stale`, not `Success` (decision 4). Carrying the last known
//!   status forward is how a pipeline that stopped running keeps looking green.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// What a check concluded.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestStatus {
    Success,
    Failed,
    /// The check could not complete — a connection failed, the job was killed.
    ///
    /// **Neither a pass nor a failure**, and that is not a technicality: an
    /// aborted check says nothing whatever about the data, so it is exactly as
    /// informative as no recent check at all. Counting it as either would
    /// invent a signal out of an outage.
    Aborted,
}

impl TestStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            TestStatus::Success => "success",
            TestStatus::Failed => "failed",
            TestStatus::Aborted => "aborted",
        }
    }

    #[must_use]
    pub const fn all() -> &'static [TestStatus] {
        &[TestStatus::Success, TestStatus::Failed, TestStatus::Aborted]
    }

    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(raw: &str) -> Result<Self, String> {
        TestStatus::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }
}

/// What an asset's tests say about it.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Healthy,
    Unhealthy,
    /// Tests exist, but nothing recent enough to rely on.
    Stale,
    /// **No tests at all.** Distinct from every other state, and never
    /// `Healthy`: silence is not a pass.
    Unknown,
}

/// The whole picture, not just the verdict.
///
/// **The counts are the point.** "Unhealthy" tells a steward to look; "three of
/// forty failing, and two more stale" tells them how hard to look and where. A
/// summary that collapsed to a single word would send them to the test runner
/// to find out what a catalog already knew.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSummary {
    pub state: Health,
    pub passing: usize,
    pub failing: usize,
    /// Cases whose latest result is older than their cadence, **or aborted**.
    /// Both mean the same thing operationally: no usable recent signal.
    pub stale: usize,
    /// Named, because a count sends somebody hunting and a name sends them to
    /// the check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failing_cases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_cases: Vec<String>,
}

/// One test case's most recent result, as health is computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestResult {
    pub case_name: String,
    /// `None` when the case has been registered but never run — which is a
    /// *stale* case rather than an absent one, because somebody declared the
    /// check and it has produced nothing.
    pub status: Option<TestStatus>,
    pub observed_at: Option<DateTime<Utc>>,
    /// How often this check is expected to run. `None` means no cadence was
    /// declared, so age cannot make it stale — only its status counts.
    pub cadence: Option<Duration>,
}

impl LatestResult {
    /// Whether this case has a usable recent signal.
    ///
    /// A result is stale when it is older than its declared cadence, when the
    /// case has never run, or when the last run aborted. All three mean the
    /// same thing to a reader: nothing recent enough to rely on.
    #[must_use]
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        match (self.status, self.observed_at) {
            (None, _) | (_, None) => true,
            (Some(TestStatus::Aborted), _) => true,
            (Some(_), Some(observed_at)) => match self.cadence {
                None => false,
                // **Strictly greater.** A daily check that ran exactly
                // twenty-four hours ago has not yet missed its window; one a
                // second later has. Anything else makes every punctual pipeline
                // flicker.
                Some(cadence) => now - observed_at > cadence,
            },
        }
    }
}

/// An asset's health, from its test cases' latest results.
///
/// **Precedence, and every step of it is a decision:**
///
/// 1. **No cases at all → `Unknown`.** Decision 5. Never `Healthy`.
/// 2. **Any fresh failure → `Unhealthy`.** A live failure outranks staleness:
///    something is known to be wrong, which is more actionable than something
///    being unmeasured.
/// 3. **Any stale case → `Stale`.** Reported *distinctly* rather than averaged
///    into the passes — a table with nine fresh passes and one check that
///    stopped running six weeks ago is not simply healthy, and the whole point
///    of decision 4 is that nobody finds that out by accident.
/// 4. **Otherwise `Healthy`**, which now means every declared check ran
///    recently and passed.
#[must_use]
pub fn health_of(cases: &[LatestResult], now: DateTime<Utc>) -> HealthSummary {
    if cases.is_empty() {
        return HealthSummary {
            state: Health::Unknown,
            passing: 0,
            failing: 0,
            stale: 0,
            failing_cases: Vec::new(),
            stale_cases: Vec::new(),
        };
    }

    let mut passing = 0;
    let mut failing_cases = Vec::new();
    let mut stale_cases = Vec::new();

    for case in cases {
        if case.is_stale(now) {
            stale_cases.push(case.case_name.clone());
        } else if case.status == Some(TestStatus::Failed) {
            failing_cases.push(case.case_name.clone());
        } else {
            passing += 1;
        }
    }

    let state = if !failing_cases.is_empty() {
        Health::Unhealthy
    } else if !stale_cases.is_empty() {
        Health::Stale
    } else {
        Health::Healthy
    };

    HealthSummary {
        state,
        passing,
        failing: failing_cases.len(),
        stale: stale_cases.len(),
        failing_cases,
        stale_cases,
    }
}

/// The worst health among a set — for rolling upstream health up a lineage
/// walk.
///
/// **`Unknown` is not the worst.** An upstream nobody tests is less alarming
/// than one known to be failing, and ordering it below `Unhealthy` is what
/// stops an untested corner of the estate drowning out a real incident.
#[must_use]
pub fn worst(states: &[Health]) -> Health {
    states
        .iter()
        .copied()
        .max_by_key(|state| match state {
            Health::Unhealthy => 3,
            Health::Stale => 2,
            Health::Unknown => 1,
            Health::Healthy => 0,
        })
        .unwrap_or(Health::Unknown)
}

/// Parse an ISO 8601 duration into a cadence.
///
/// **A deliberate subset: days, hours, minutes and seconds only.** `P1Y` and
/// `P1M` are refused because a year and a month are not fixed lengths of time —
/// "did this check run within its cadence" has to be answerable by subtracting
/// two instants, and a cadence that depends on which month it is cannot be. An
/// organization that means thirty days can say `P30D`.
///
/// # Errors
///
/// A sentence naming what is wrong, ready to be a `400` detail.
pub fn parse_cadence(raw: &str) -> Result<Duration, String> {
    let rest = raw
        .strip_prefix('P')
        .ok_or_else(|| format!("`{raw}` is not an ISO 8601 duration: it must start with `P`"))?;
    if rest.is_empty() {
        return Err(format!("`{raw}` states no duration"));
    }

    let (date_part, time_part) = match rest.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (rest, None),
    };

    let mut total = Duration::zero();
    let mut number = String::new();

    for character in date_part.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        let value: i64 = number
            .parse()
            .map_err(|_| format!("`{raw}` has a unit with no number before it"))?;
        number.clear();
        match character {
            'D' => total = total + Duration::days(value),
            'W' => total = total + Duration::weeks(value),
            'Y' | 'M' => {
                return Err(format!(
                    "`{raw}` uses `{character}`, which is not a fixed length of time — \
                     a cadence has to be answerable by subtracting two instants. \
                     Use days instead, e.g. `P30D`"
                ));
            }
            other => return Err(format!("`{raw}` has an unrecognised unit `{other}`")),
        }
    }
    if !number.is_empty() {
        return Err(format!("`{raw}` has a number with no unit after it"));
    }

    if let Some(time_part) = time_part {
        if time_part.is_empty() {
            return Err(format!("`{raw}` has a `T` with no time after it"));
        }
        for character in time_part.chars() {
            if character.is_ascii_digit() {
                number.push(character);
                continue;
            }
            let value: i64 = number
                .parse()
                .map_err(|_| format!("`{raw}` has a unit with no number before it"))?;
            number.clear();
            match character {
                'H' => total = total + Duration::hours(value),
                'M' => total = total + Duration::minutes(value),
                'S' => total = total + Duration::seconds(value),
                other => return Err(format!("`{raw}` has an unrecognised unit `{other}`")),
            }
        }
        if !number.is_empty() {
            return Err(format!("`{raw}` has a number with no unit after it"));
        }
    }

    if total.is_zero() {
        return Err(format!("`{raw}` is a zero-length cadence"));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name: &str, status: TestStatus, hours_ago: i64, cadence_hours: i64) -> LatestResult {
        LatestResult {
            case_name: name.to_string(),
            status: Some(status),
            observed_at: Some(Utc::now() - Duration::hours(hours_ago)),
            cadence: Some(Duration::hours(cadence_hours)),
        }
    }

    // ---- the truth table ----

    #[test]
    fn every_case_fresh_and_passing_is_healthy() {
        let cases = vec![
            case("not_null", TestStatus::Success, 1, 24),
            case("row_count", TestStatus::Success, 2, 24),
        ];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Healthy);
        assert_eq!(summary.passing, 2);
        assert_eq!(summary.failing, 0);
        assert_eq!(summary.stale, 0);
    }

    /// **The most dangerous possible bug in this module.** An asset nobody
    /// tests reported as healthy asserts trust nobody earned, and does it
    /// silently — which is why decision 5 is a decision rather than a default.
    #[test]
    fn an_asset_with_no_tests_is_unknown_and_never_healthy() {
        let summary = health_of(&[], Utc::now());

        assert_eq!(summary.state, Health::Unknown);
        assert_ne!(summary.state, Health::Healthy, "silence is not a pass");
        assert_eq!(summary.passing, 0);
    }

    #[test]
    fn a_failing_case_makes_the_asset_unhealthy_and_names_it() {
        let cases = vec![
            case("not_null", TestStatus::Success, 1, 24),
            case("row_count", TestStatus::Failed, 1, 24),
        ];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Unhealthy);
        assert_eq!(summary.failing, 1);
        assert_eq!(summary.passing, 1);
        assert_eq!(summary.failing_cases, vec!["row_count"]);
    }

    /// **An old pass is not a pass** (decision 4). Carrying the last known
    /// status forward is how a pipeline that stopped running keeps looking
    /// green for months.
    #[test]
    fn a_result_older_than_its_cadence_is_stale_not_its_last_status() {
        let cases = vec![case("not_null", TestStatus::Success, 100, 24)];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Stale);
        assert_eq!(
            summary.passing, 0,
            "a six-week-old success is not a current pass"
        );
        assert_eq!(summary.stale_cases, vec!["not_null"]);
    }

    /// **The mixed case, which averaging would hide.** Nine fresh passes and
    /// one check that stopped running is not simply healthy, and the whole
    /// point of decision 4 is that nobody finds that out by accident.
    #[test]
    fn fresh_passes_beside_a_stale_case_report_stale_rather_than_healthy() {
        let cases = vec![
            case("not_null", TestStatus::Success, 1, 24),
            case("freshness", TestStatus::Success, 100, 24),
        ];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Stale);
        assert_eq!(summary.passing, 1, "the fresh one still counts as passing");
        assert_eq!(summary.stale, 1, "and the stale one is reported distinctly");
    }

    /// **A live failure outranks staleness.** Something known to be wrong is
    /// more actionable than something unmeasured.
    #[test]
    fn a_fresh_failure_outranks_a_stale_case() {
        let cases = vec![
            case("row_count", TestStatus::Failed, 1, 24),
            case("freshness", TestStatus::Success, 100, 24),
        ];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Unhealthy);
        assert_eq!(summary.failing, 1);
        assert_eq!(summary.stale, 1, "and the stale one is still reported");
    }

    /// A *stale failure* is stale, not failing — the check has not run recently
    /// enough for its verdict to mean anything, whichever verdict it was.
    #[test]
    fn a_failure_older_than_its_cadence_is_stale_rather_than_failing() {
        let cases = vec![case("row_count", TestStatus::Failed, 100, 24)];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Stale);
        assert_eq!(summary.failing, 0, "a six-week-old failure is not current");
    }

    /// **An aborted check says nothing about the data.** Counting it as either
    /// a pass or a failure would invent a signal out of an outage.
    #[test]
    fn an_aborted_result_is_neither_a_pass_nor_a_failure() {
        let cases = vec![case("not_null", TestStatus::Aborted, 1, 24)];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Stale);
        assert_eq!(summary.passing, 0);
        assert_eq!(summary.failing, 0);
    }

    /// A registered case that has never run is stale, not absent — somebody
    /// declared the check and it has produced nothing, which is worth saying.
    #[test]
    fn a_case_that_has_never_run_is_stale() {
        let cases = vec![LatestResult {
            case_name: "not_null".to_string(),
            status: None,
            observed_at: None,
            cadence: Some(Duration::hours(24)),
        }];

        let summary = health_of(&cases, Utc::now());

        assert_eq!(summary.state, Health::Stale);
        assert_eq!(summary.stale, 1);
    }

    /// With no declared cadence, age cannot make a result stale — only its
    /// status counts. Otherwise every check without a schedule would decay to
    /// stale and the state would mean nothing.
    #[test]
    fn a_case_with_no_cadence_never_goes_stale_from_age() {
        let cases = vec![LatestResult {
            case_name: "manual_review".to_string(),
            status: Some(TestStatus::Success),
            observed_at: Some(Utc::now() - Duration::days(400)),
            cadence: None,
        }];

        assert_eq!(health_of(&cases, Utc::now()).state, Health::Healthy);
    }

    // ---- the staleness boundary ----

    /// **Exactly at the cadence is still fresh; one second past is not.** A
    /// daily check that ran twenty-four hours ago has not missed its window,
    /// and anything else makes every punctual pipeline flicker.
    #[test]
    fn the_staleness_boundary_is_strictly_past_the_cadence() {
        let now = Utc::now();
        let exactly = LatestResult {
            case_name: "daily".to_string(),
            status: Some(TestStatus::Success),
            observed_at: Some(now - Duration::hours(24)),
            cadence: Some(Duration::hours(24)),
        };
        let one_second_later = LatestResult {
            observed_at: Some(now - Duration::hours(24) - Duration::seconds(1)),
            ..exactly.clone()
        };

        assert!(!exactly.is_stale(now), "on time is not late");
        assert!(one_second_later.is_stale(now), "one second late is late");
    }

    // ---- rolling up ----

    /// **`Unknown` is not the worst.** An upstream nobody tests is less
    /// alarming than one known to be failing, and ordering it below `Unhealthy`
    /// stops an untested corner of the estate drowning out a real incident.
    #[test]
    fn the_worst_upstream_health_ranks_unhealthy_above_unknown() {
        assert_eq!(
            worst(&[Health::Healthy, Health::Unknown, Health::Unhealthy]),
            Health::Unhealthy
        );
        assert_eq!(worst(&[Health::Healthy, Health::Unknown]), Health::Unknown);
        assert_eq!(worst(&[Health::Healthy, Health::Stale]), Health::Stale);
        assert_eq!(worst(&[Health::Stale, Health::Unknown]), Health::Stale);
    }

    #[test]
    fn nothing_upstream_is_unknown_rather_than_healthy() {
        assert_eq!(worst(&[]), Health::Unknown);
    }

    // ---- cadence parsing ----

    #[test]
    fn common_cadences_parse() {
        assert_eq!(parse_cadence("P1D"), Ok(Duration::days(1)));
        assert_eq!(parse_cadence("PT1H"), Ok(Duration::hours(1)));
        assert_eq!(parse_cadence("PT30M"), Ok(Duration::minutes(30)));
        assert_eq!(parse_cadence("P1W"), Ok(Duration::weeks(1)));
        assert_eq!(
            parse_cadence("P1DT12H"),
            Ok(Duration::days(1) + Duration::hours(12))
        );
    }

    /// **`M` means months before the `T` and minutes after it**, which is the
    /// single nastiest thing about ISO 8601 durations — and the reason the
    /// parser splits on `T` rather than scanning once.
    #[test]
    fn the_same_letter_means_different_things_either_side_of_the_t() {
        assert_eq!(parse_cadence("PT5M"), Ok(Duration::minutes(5)));
        assert!(
            parse_cadence("P5M").is_err(),
            "before the T, `M` is months, and months are refused"
        );
    }

    /// **Years and months are refused, deliberately.** "Did this run within its
    /// cadence" has to be answerable by subtracting two instants, and a cadence
    /// that depends on which month it is cannot be.
    #[test]
    fn years_and_months_are_refused_with_a_usable_alternative() {
        let error = parse_cadence("P1Y").expect_err("years are not a fixed length");
        assert!(error.contains("fixed length"), "{error}");
        assert!(error.contains("P30D"), "and it says what to use: {error}");
    }

    #[test]
    fn malformed_durations_are_refused() {
        for bad in ["1D", "P", "PD", "P1", "PT", "P1X", ""] {
            assert!(parse_cadence(bad).is_err(), "`{bad}` should be refused");
        }
    }

    /// A zero cadence would make every result instantly stale, which is a
    /// configuration nobody means.
    #[test]
    fn a_zero_cadence_is_refused() {
        assert!(parse_cadence("PT0S").is_err());
    }

    // ---- wire shapes ----

    #[test]
    fn statuses_round_trip_through_their_wire_names() {
        for status in TestStatus::all() {
            assert_eq!(TestStatus::parse(status.as_str()), Ok(*status));
        }
        assert!(TestStatus::parse("flaky").is_err());
        // Pinned, because these are the values the database's `CHECK` accepts.
        assert_eq!(TestStatus::Success.as_str(), "success");
        assert_eq!(TestStatus::Aborted.as_str(), "aborted");
    }

    #[test]
    fn a_summary_is_camel_case_on_the_wire() {
        let summary = HealthSummary {
            state: Health::Unhealthy,
            passing: 1,
            failing: 2,
            stale: 3,
            failing_cases: vec!["row_count".to_string()],
            stale_cases: Vec::new(),
        };

        let json = serde_json::to_value(&summary).expect("serialize");

        assert_eq!(json["state"], "unhealthy");
        assert!(json.get("failingCases").is_some(), "{json}");
        assert!(json.get("failing_cases").is_none(), "{json}");
        assert!(
            json.get("staleCases").is_none(),
            "an empty list is omitted rather than sent as []: {json}"
        );

        let parsed: HealthSummary = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, summary);
    }
}
