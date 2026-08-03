//! Usage and popularity — Epic 28.
//!
//! **Why an agent needs this**: recommending a technically-matching but
//! abandoned table is worse than returning nothing, because it looks like an
//! answer. A table twelve teams query daily is a different proposition from one
//! last read eight months ago, and no amount of metadata distinguishes them.
//!
//! Everything here is pure. Storing a pre-computed popularity would go stale
//! silently (decision 1) — the same reasoning as Epic 26's certification status
//! and Epic 30's health — so the summary is derived from rollups on read, and
//! this module is the derivation.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// What was done to an asset.
#[derive(
    utoipa::ToSchema,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum UsageOperation {
    Read,
    Write,
    Delete,
    /// A tool inspected the schema without reading rows. Counted separately
    /// because it is *not* evidence anybody depends on the data — a BI tool
    /// refreshing its catalogue touches every table it can see.
    SchemaRead,
}

impl UsageOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            UsageOperation::Read => "read",
            UsageOperation::Write => "write",
            UsageOperation::Delete => "delete",
            UsageOperation::SchemaRead => "schemaRead",
        }
    }

    #[must_use]
    pub const fn all() -> &'static [UsageOperation] {
        &[
            UsageOperation::Read,
            UsageOperation::Write,
            UsageOperation::Delete,
            UsageOperation::SchemaRead,
        ]
    }

    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(raw: &str) -> Result<Self, String> {
        UsageOperation::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }

    /// Whether this operation is evidence that somebody *depends* on the data.
    ///
    /// A schema read is not: a BI tool refreshing its catalogue touches every
    /// table it can see, and counting that as use would make the whole estate
    /// look busy and the signal worthless.
    #[must_use]
    pub fn is_consumption(self) -> bool {
        matches!(self, UsageOperation::Read | UsageOperation::Write)
    }
}

/// Who used it.
///
/// **Unresolved identifiers are kept, not discarded** (decision 3). A warehouse
/// username that maps to no `User` is still a distinct consumer, and dropping it
/// would under-count exactly the external usage nobody has onboarded yet.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Consumer {
    /// Resolved to a `User` in this catalog.
    Principal { id: String },
    /// A warehouse identity nothing here matches — **yet**. Resolution is
    /// retroactive, so creating the matching user later reclassifies the
    /// history rather than starting a second count.
    Opaque { identifier: String },
}

impl Consumer {
    /// The stable key a rollup is grouped by.
    ///
    /// **Prefixed, so a principal called `alice` and an opaque `alice` do not
    /// collide.** They are different consumers until resolution says otherwise,
    /// and merging them silently would inflate one and erase the other.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Consumer::Principal { id } => format!("principal:{id}"),
            Consumer::Opaque { identifier } => format!("opaque:{identifier}"),
        }
    }

    /// The bare identifier, whichever kind this is.
    #[must_use]
    pub fn identifier(&self) -> &str {
        match self {
            Consumer::Principal { id } => id,
            Consumer::Opaque { identifier } => identifier,
        }
    }
}

/// Which way usage is moving.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trend {
    Rising,
    Stable,
    Declining,
    /// No access in [`DORMANT_AFTER_DAYS`]. **The signal that most changes a
    /// recommendation**, and therefore the one that must never be guessed.
    Dormant,
    /// Nothing has ever been ingested for this asset. **Not `Dormant`** —
    /// absence of data is not absence of use, and claiming an asset is unused
    /// when nothing was ever measured is a false negative that would get it
    /// wrongly retired.
    Unknown,
}

/// How long without an access before an asset is dormant.
///
/// A quarter: long enough that seasonal and quarterly workloads — a report run
/// at each period close — do not read as abandoned, which is the false positive
/// that would matter most. Shorter windows make every quarterly job look dead
/// between runs.
pub const DORMANT_AFTER_DAYS: i64 = 90;

/// The minimum activity before a trend is reported at all.
///
/// **Without a floor, one query last week against two this week is "Rising
/// 100%"** — a ratio computed from noise, presented with the same confidence as
/// one computed from thousands. Five is the smallest number for which a doubling
/// is more likely to be a real change than a coincidence of who happened to be
/// working.
pub const TREND_VOLUME_FLOOR: u64 = 5;

/// How much a period must differ to be called a change rather than noise.
///
/// Twenty percent: below it, week-to-week variation in a stable workload —
/// a bank holiday, one analyst on leave — would flip the label back and forth
/// and teach everyone to ignore it.
pub const TREND_CHANGE_PCT: f64 = 20.0;

/// What a consumer sees about how used something is.
///
/// **Computed on read**, never stored. See the module note.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopularitySummary {
    pub queries_last_7d: u64,
    pub queries_last_30d: u64,
    pub distinct_consumers_30d: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<DateTime<Utc>>,
    pub trend: Trend,
}

/// A day's worth of one consumer's use of one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRollup {
    pub consumer_key: String,
    pub day: NaiveDate,
    pub operation: UsageOperation,
    pub count: u64,
    pub total_rows: Option<u64>,
}

/// Which way usage is moving, from two consecutive windows.
///
/// `last_accessed` decides `Dormant` and `Unknown` **before** the ratio is
/// looked at, because both are statements about whether there is anything to
/// compare rather than about the comparison.
#[must_use]
pub fn trend_of(
    recent: u64,
    previous: u64,
    last_accessed: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Trend {
    // Nothing was ever ingested. **Not `Dormant`**: absence of data is not
    // absence of use, and an asset wrongly reported as unused is one somebody
    // retires.
    let Some(last_accessed) = last_accessed else {
        return Trend::Unknown;
    };
    if (now - last_accessed).num_days() >= DORMANT_AFTER_DAYS {
        return Trend::Dormant;
    }

    // Below the floor, any ratio is noise wearing a percentage.
    if recent + previous < TREND_VOLUME_FLOOR {
        return Trend::Stable;
    }
    if previous == 0 {
        // Something from nothing, above the floor, is a real start.
        return Trend::Rising;
    }

    #[allow(clippy::cast_precision_loss)]
    let change = ((recent as f64 - previous as f64) / previous as f64) * 100.0;
    if change >= TREND_CHANGE_PCT {
        Trend::Rising
    } else if change <= -TREND_CHANGE_PCT {
        Trend::Declining
    } else {
        Trend::Stable
    }
}

/// A popularity summary from a set of daily rollups.
///
/// **Counts only consuming operations** — a schema read is a BI tool refreshing
/// its catalogue, not evidence anybody depends on the data, and counting it
/// would make the whole estate look busy.
#[must_use]
pub fn summarise(
    rollups: &[UsageRollup],
    last_accessed: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> PopularitySummary {
    let today = now.date_naive();
    let within = |day: NaiveDate, days: i64| (today - day).num_days() < days;

    let consuming = || {
        rollups
            .iter()
            .filter(|rollup| rollup.operation.is_consumption())
    };

    let queries_last_7d: u64 = consuming()
        .filter(|r| within(r.day, 7))
        .map(|r| r.count)
        .sum();
    let queries_last_30d: u64 = consuming()
        .filter(|r| within(r.day, 30))
        .map(|r| r.count)
        .sum();
    let previous_7d: u64 = consuming()
        .filter(|r| !within(r.day, 7) && within(r.day, 14))
        .map(|r| r.count)
        .sum();

    let distinct: std::collections::BTreeSet<&str> = consuming()
        .filter(|r| within(r.day, 30))
        .map(|r| r.consumer_key.as_str())
        .collect();

    PopularitySummary {
        queries_last_7d,
        queries_last_30d,
        distinct_consumers_30d: distinct.len() as u64,
        last_accessed,
        trend: trend_of(queries_last_7d, previous_7d, last_accessed, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn rollup(days_ago: i64, consumer: &str, count: u64, operation: UsageOperation) -> UsageRollup {
        UsageRollup {
            consumer_key: consumer.to_string(),
            day: (Utc::now() - Duration::days(days_ago)).date_naive(),
            operation,
            count,
            total_rows: None,
        }
    }

    // ---- the trend floor ----

    /// **The floor test the plan names.** One query last week against two this
    /// week is not "Rising 100%" — that is a ratio computed from noise,
    /// presented with the confidence of one computed from thousands.
    #[test]
    fn tiny_counts_do_not_produce_a_trend() {
        let recent = Utc::now() - Duration::days(1);

        assert_eq!(
            trend_of(2, 1, Some(recent), Utc::now()),
            Trend::Stable,
            "three queries in a fortnight is not a rising asset"
        );
    }

    /// And above the floor the same proportional change *is* reported, or the
    /// floor would be a mute button rather than a threshold.
    #[test]
    fn the_same_proportional_change_above_the_floor_is_reported() {
        let recent = Utc::now() - Duration::days(1);

        assert_eq!(
            trend_of(20, 10, Some(recent), Utc::now()),
            Trend::Rising,
            "a doubling from ten is a real signal"
        );
    }

    #[test]
    fn a_sustained_fall_is_declining_and_a_steady_week_is_stable() {
        let recent = Utc::now() - Duration::days(1);

        assert_eq!(trend_of(10, 40, Some(recent), Utc::now()), Trend::Declining);
        assert_eq!(trend_of(21, 20, Some(recent), Utc::now()), Trend::Stable);
    }

    /// Something from nothing, above the floor, is a genuine start rather than
    /// a division by zero.
    #[test]
    fn activity_starting_from_nothing_is_rising() {
        let recent = Utc::now() - Duration::days(1);

        assert_eq!(trend_of(10, 0, Some(recent), Utc::now()), Trend::Rising);
    }

    // ---- Dormant and Unknown are different answers ----

    /// **The test that stops an asset being wrongly retired.** Nothing was ever
    /// ingested, so nothing is known — and claiming it is unused would be a
    /// false negative somebody acts on.
    #[test]
    fn an_asset_with_no_observations_is_unknown_not_dormant() {
        assert_eq!(
            trend_of(0, 0, None, Utc::now()),
            Trend::Unknown,
            "absence of data is not absence of use"
        );
    }

    /// And a genuinely untouched asset *is* dormant, or the distinction above
    /// would make the signal unreachable.
    #[test]
    fn an_asset_untouched_for_a_quarter_is_dormant() {
        let stale = Utc::now() - Duration::days(DORMANT_AFTER_DAYS + 1);

        assert_eq!(trend_of(0, 0, Some(stale), Utc::now()), Trend::Dormant);
    }

    /// The boundary, at exactly the window. Ninety days with no access is
    /// dormant; eighty-nine is not.
    #[test]
    fn the_dormancy_boundary_is_the_window_itself() {
        let now = Utc::now();

        assert_eq!(
            trend_of(0, 0, Some(now - Duration::days(DORMANT_AFTER_DAYS)), now),
            Trend::Dormant
        );
        assert_ne!(
            trend_of(
                0,
                0,
                Some(now - Duration::days(DORMANT_AFTER_DAYS - 1)),
                now
            ),
            Trend::Dormant
        );
    }

    /// **Dormancy beats the ratio.** An asset with a busy fortnight three
    /// months ago is dormant now, whatever the two windows say about each
    /// other — which they would report as `Stable`.
    #[test]
    fn dormancy_is_decided_before_the_ratio() {
        let stale = Utc::now() - Duration::days(DORMANT_AFTER_DAYS + 10);

        assert_eq!(trend_of(50, 50, Some(stale), Utc::now()), Trend::Dormant);
    }

    // ---- summarising rollups ----

    #[test]
    fn the_summary_counts_the_right_windows() {
        let rollups = vec![
            rollup(1, "principal:asha", 10, UsageOperation::Read),
            rollup(6, "principal:asha", 5, UsageOperation::Read),
            rollup(20, "opaque:etl_bot", 3, UsageOperation::Read),
            rollup(45, "principal:asha", 100, UsageOperation::Read),
        ];

        let summary = summarise(&rollups, Some(Utc::now() - Duration::days(1)), Utc::now());

        assert_eq!(summary.queries_last_7d, 15, "{summary:?}");
        assert_eq!(
            summary.queries_last_30d, 18,
            "the 45-day-old row is outside"
        );
        assert_eq!(summary.distinct_consumers_30d, 2);
    }

    /// **A schema read is not use.** A BI tool refreshing its catalogue touches
    /// every table it can see; counting that would make the whole estate look
    /// busy and the signal worthless.
    #[test]
    fn schema_reads_do_not_count_as_consumption() {
        let rollups = vec![
            rollup(1, "principal:bi_tool", 500, UsageOperation::SchemaRead),
            rollup(1, "principal:asha", 2, UsageOperation::Read),
        ];

        let summary = summarise(&rollups, Some(Utc::now()), Utc::now());

        assert_eq!(summary.queries_last_7d, 2, "{summary:?}");
        assert_eq!(
            summary.distinct_consumers_30d, 1,
            "the catalogue refresher is not a consumer"
        );
    }

    /// A write *is* use — somebody depends on this table enough to maintain it.
    #[test]
    fn writes_count_as_consumption() {
        let rollups = vec![rollup(1, "principal:etl", 7, UsageOperation::Write)];

        assert_eq!(
            summarise(&rollups, Some(Utc::now()), Utc::now()).queries_last_7d,
            7
        );
    }

    #[test]
    fn an_asset_with_no_rollups_reports_zeros_and_unknown() {
        let summary = summarise(&[], None, Utc::now());

        assert_eq!(summary.queries_last_7d, 0);
        assert_eq!(summary.queries_last_30d, 0);
        assert_eq!(summary.distinct_consumers_30d, 0);
        assert_eq!(summary.trend, Trend::Unknown);
    }

    // ---- consumer identity ----

    /// **A principal and an opaque identifier do not collide.** They are
    /// different consumers until resolution says otherwise, and merging them
    /// silently would inflate one count and erase the other.
    #[test]
    fn a_principal_and_an_opaque_identifier_with_the_same_name_are_distinct() {
        let principal = Consumer::Principal {
            id: "alice".to_string(),
        };
        let opaque = Consumer::Opaque {
            identifier: "alice".to_string(),
        };

        assert_ne!(principal.key(), opaque.key());
        assert_eq!(principal.identifier(), opaque.identifier());
    }

    // ---- wire shapes ----

    #[test]
    fn operations_round_trip_through_their_wire_names() {
        for operation in UsageOperation::all() {
            assert_eq!(UsageOperation::parse(operation.as_str()), Ok(*operation));
        }
        assert!(UsageOperation::parse("browsed").is_err());
        // Pinned, because these are the values the database's `CHECK` accepts.
        assert_eq!(UsageOperation::SchemaRead.as_str(), "schemaRead");
        assert_eq!(UsageOperation::Read.as_str(), "read");
    }

    #[test]
    fn a_consumer_is_camel_case_and_tagged_on_the_wire() {
        let json = serde_json::to_value(Consumer::Opaque {
            identifier: "etl_bot".to_string(),
        })
        .expect("serialize");

        assert_eq!(json["kind"], "opaque");
        assert!(json.get("identifier").is_some(), "{json}");
    }

    #[test]
    fn a_summary_is_camel_case_on_the_wire() {
        let json = serde_json::to_value(PopularitySummary {
            queries_last_7d: 1,
            queries_last_30d: 2,
            distinct_consumers_30d: 3,
            last_accessed: None,
            trend: Trend::Dormant,
        })
        .expect("serialize");

        assert!(json.get("queriesLast7d").is_some(), "{json}");
        assert!(json.get("distinctConsumers30d").is_some(), "{json}");
        assert!(json.get("queries_last_7d").is_none(), "{json}");
        assert_eq!(json["trend"], "dormant");
    }
}
