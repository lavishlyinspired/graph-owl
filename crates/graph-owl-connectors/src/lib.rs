//! Source connectors: the `Connector` trait, the run machinery, and the
//! Postgres reference implementation.
//!
//! **Status**: Epic 15, first vertical slice. Connectors beyond this one are
//! Python, out of process, pushing through the ingestion API — see
//! `plans/00j-language-boundaries.md`. What stays here is the governance part:
//! the trait, run scoping, and the ordering guarantee.

pub mod batch;
pub mod ingest;
pub mod job;
pub mod rows;
pub mod streaming;
pub mod streaming_pulsar;
pub mod webhook_mapping;
pub mod webhook_signature;

use async_trait::async_trait;
use graph_owl_core::AssetKind;
use serde::{Deserialize, Serialize};

pub mod postgres;

/// One record yielded by a source, before it becomes a catalog asset.
///
/// Carries the *path* rather than a parent id, because a source knows where a
/// thing sits in its own hierarchy and knows nothing about catalog ids. The
/// sink resolves the path to a parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub kind: AssetKind,
    /// Root-to-leaf, e.g. `["warehouse", "sales", "public", "orders"]`.
    pub path: Vec<String>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

impl SourceRecord {
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }

    /// A fingerprint of what the **source** said, and nothing else.
    ///
    /// Decision 3 makes a re-run *converge*; it does not make it *cheap* — the
    /// catalog still receives, validates and diffs every record before deciding
    /// nothing changed. This turns the second run into read-compare-skip for the
    /// unchanged majority.
    ///
    /// **Only source-owned fields.** A description a person edited in the
    /// console is catalog-owned: including it would make every human edit look
    /// like a source change on the next run, and the connector would helpfully
    /// overwrite it. Kind, path, and the source's own description and
    /// properties are what the source is entitled to assert.
    ///
    /// Framed with lengths rather than concatenated, because
    /// `["ab", "c"]` and `["a", "bc"]` are different paths and a naive join
    /// gives them the same bytes — a collision between two real assets, which
    /// is the one failure mode a fingerprint must not have.
    #[must_use]
    pub fn source_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.kind.as_str().as_bytes());
        hasher.update(b"\x1f");
        hasher.update(
            u64::try_from(self.path.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for segment in &self.path {
            hasher.update(
                u64::try_from(segment.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            hasher.update(segment.as_bytes());
        }
        // `None` and `Some("")` are different statements — "the source has
        // nothing to say" versus "the source says it is empty" — and a
        // fingerprint that conflated them would skip a write that clears a
        // description.
        hash_optional(&mut hasher, self.description.as_deref().map(str::as_bytes));
        hash_optional(
            &mut hasher,
            self.properties
                .as_ref()
                .map(|p| p.to_string())
                .as_deref()
                .map(str::as_bytes),
        );
        hasher.finalize().into()
    }
}

fn hash_optional(hasher: &mut sha2::Sha256, value: Option<&[u8]>) {
    use sha2::Digest as _;
    match value {
        None => hasher.update([0u8]),
        Some(bytes) => {
            hasher.update([1u8]);
            hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
            hasher.update(bytes);
        }
    }
}

/// What a run should do with one record, decided before any write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ingest {
    /// The FQN is unknown.
    Create,
    /// Known, and the source says something different.
    Patch,
    /// Known, and the source says exactly what it said last time.
    Skip,
}

/// What the catalog already holds for an FQN.
///
/// Three states, not two. `Option<[u8; 32]>` cannot express them: "no such
/// asset" and "an asset with no fingerprint" are different situations with
/// different correct answers, and collapsing them makes one of the two wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existing {
    /// No asset with this FQN.
    Absent,
    /// The asset exists but carries no fingerprint — catalogued before
    /// fingerprinting, or by a connector that does not compute one.
    Unfingerprinted,
    Fingerprinted([u8; 32]),
}

/// Decision 7's three outcomes.
///
/// An asset with **no stored fingerprint is patched, not skipped**. We cannot
/// prove it changed, and that is exactly the point: skipping on absent evidence
/// would freeze every pre-fingerprinting asset at whatever it said then, and
/// the freeze would be invisible — the run would report success and change
/// nothing, for as long as nobody noticed.
#[must_use]
pub fn decide_ingest(existing: Existing, incoming: [u8; 32]) -> Ingest {
    match existing {
        Existing::Absent => Ingest::Create,
        Existing::Unfingerprinted => Ingest::Patch,
        Existing::Fingerprinted(stored) if stored == incoming => Ingest::Skip,
        Existing::Fingerprinted(_) => Ingest::Patch,
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunScope {
    /// Schemas to include. Empty means all non-system schemas.
    pub include_schemas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub created: usize,
    pub failed: usize,
}

#[derive(Debug)]
pub enum ConnectorError {
    Connection(String),
    Introspection(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::Connection(message) => write!(f, "connection failed: {message}"),
            ConnectorError::Introspection(message) => write!(f, "introspection failed: {message}"),
        }
    }
}

#[async_trait]
pub trait Connector: Send + Sync {
    /// Stable type name, used in configuration and run history.
    fn type_name(&self) -> &'static str;

    /// What this connector needs configured, as JSON Schema.
    ///
    /// **The connector declares its own shape**, so the admin console renders a
    /// form without knowing what a Postgres connection needs — which is the
    /// whole reason the console has one form renderer instead of one screen per
    /// connector. A hundred connectors with hand-written forms is a hundred
    /// places for a field to go missing.
    ///
    /// A property is marked secret with `"writeOnly": true`, which is JSON
    /// Schema's own vocabulary for it. The console renders those as password
    /// inputs and never populates them, because there is nothing to populate
    /// from — `ConnectorConfig` has no field for a credential.
    fn config_schema(&self) -> serde_json::Value;

    /// Fails fast with a typed error rather than surfacing at first fetch.
    async fn test_connection(&self) -> Result<(), ConnectorError>;

    /// Yields records **parents before children**.
    ///
    /// The ordering is the connector's contract, not the sink's problem: a sink
    /// that had to buffer and topologically sort would need the whole source in
    /// memory before writing anything.
    async fn fetch(&self, scope: &RunScope) -> Result<Vec<SourceRecord>, ConnectorError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_name_is_the_last_path_segment() {
        let record = SourceRecord {
            kind: AssetKind::Table,
            path: vec![
                "warehouse".into(),
                "sales".into(),
                "public".into(),
                "orders".into(),
            ],
            description: None,
            properties: None,
        };
        assert_eq!(record.name(), "orders");
    }

    #[test]
    fn an_empty_path_yields_an_empty_name_rather_than_panicking() {
        let record = SourceRecord {
            kind: AssetKind::Service,
            path: Vec::new(),
            description: None,
            properties: None,
        };
        assert_eq!(record.name(), "");
    }
}

/// What a run would remove, and whether that is allowed to happen.
///
/// Deletion detection is the one part of a connector run that destroys
/// information, and the failure it guards against is mundane: a connection
/// string pointed at the wrong database, or credentials that quietly lost
/// access to a schema. Both produce a run that sees almost nothing and would
/// tombstone almost everything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionPlan {
    /// Assets under this run's scope that the source no longer reports.
    pub absent: usize,
    /// How many the run examined in total.
    pub total: usize,
    /// `Some` when the run refuses to proceed, naming why.
    pub refused: Option<String>,
}

impl DeletionPlan {
    /// The fraction of the scope a run may remove before it stops and asks.
    ///
    /// Not an attempt to tell a large legitimate change from a
    /// misconfiguration — nothing can, from the outside. It is the point where
    /// the cost of a human reviewing the run is plainly lower than the cost of
    /// being wrong: a fifth of an estate tombstoned is painful to restore and
    /// alarming to discover, while a fifth being legitimately dropped in one
    /// run is rare enough to be worth a look.
    ///
    /// An operator who means it passes an explicit threshold and proceeds.
    pub const DEFAULT_THRESHOLD: f64 = 0.20;

    /// Decide whether a run may delete what it found missing.
    #[must_use]
    pub fn decide(absent: usize, total: usize, threshold: f64) -> Self {
        // Nothing missing is always safe, including on an empty scope — and
        // 0/0 must not become NaN and slip past the comparison.
        if absent == 0 {
            return Self {
                absent,
                total,
                refused: None,
            };
        }

        // A run that reports nothing at all against a non-empty catalog is the
        // signature of a misconfiguration, not of an emptied database — so it
        // is refused even when the *fraction* would have passed.
        //
        // `threshold > 1.0` is the deliberate override, and it has to be
        // checked first or the guard would refuse the very case it documents
        // as the way through.
        if absent == total && threshold <= 1.0 {
            return Self {
                absent,
                total,
                refused: Some(format!(
                    "the source reported none of the {total} assets in this scope. \
                     That is what a wrong connection string looks like; pass an \
                     explicit threshold above 1.0 to delete them anyway"
                )),
            };
        }

        #[allow(clippy::cast_precision_loss)]
        let fraction = absent as f64 / total as f64;
        if fraction > threshold {
            return Self {
                absent,
                total,
                refused: Some(format!(
                    "{absent} of {total} assets ({:.0}%) are absent from the source, \
                     above the {:.0}% threshold for this run",
                    fraction * 100.0,
                    threshold * 100.0
                )),
            };
        }

        Self {
            absent,
            total,
            refused: None,
        }
    }

    #[must_use]
    pub fn is_allowed(&self) -> bool {
        self.refused.is_none()
    }
}

#[cfg(test)]
mod connector_error_tests {
    use super::ConnectorError;

    /// These strings reach an operator staring at a failed run, and are the
    /// only thing distinguishing "I cannot reach the database" from "I reached
    /// it and could not read its catalog" — two failures with entirely
    /// different fixes.
    #[test]
    fn each_failure_says_which_stage_failed_and_why() {
        let connection = ConnectorError::Connection("no route to host".to_string()).to_string();
        assert!(connection.contains("connection failed"), "{connection}");
        assert!(connection.contains("no route to host"), "{connection}");

        let introspection =
            ConnectorError::Introspection("permission denied for schema".to_string()).to_string();
        assert!(
            introspection.contains("introspection failed"),
            "{introspection}"
        );
        assert!(
            introspection.contains("permission denied"),
            "{introspection}"
        );
    }

    #[test]
    fn the_two_stages_do_not_render_alike() {
        assert_ne!(
            ConnectorError::Connection("x".to_string()).to_string(),
            ConnectorError::Introspection("x".to_string()).to_string()
        );
    }
}

#[cfg(test)]
mod deletion_plan_tests {
    use super::DeletionPlan;

    const T: f64 = DeletionPlan::DEFAULT_THRESHOLD;

    #[test]
    fn nothing_missing_is_always_allowed() {
        assert!(DeletionPlan::decide(0, 100, T).is_allowed());
    }

    /// 0/0 must not become NaN — a NaN comparison is false, so the run would
    /// be allowed by accident rather than by decision.
    #[test]
    fn an_empty_scope_is_allowed_and_not_a_division_by_zero() {
        let plan = DeletionPlan::decide(0, 0, T);
        assert!(plan.is_allowed());
        assert_eq!(plan.absent, 0);
    }

    #[test]
    fn ordinary_churn_is_allowed() {
        // Five of a hundred: a dropped table and its columns.
        assert!(DeletionPlan::decide(5, 100, T).is_allowed());
    }

    /// The boundary is a real decision, not a rounding artefact: exactly at the
    /// threshold proceeds, one past it refuses.
    #[test]
    fn the_threshold_is_inclusive_at_the_boundary() {
        assert!(
            DeletionPlan::decide(20, 100, T).is_allowed(),
            "exactly 20% is at the threshold, not above it"
        );
        assert!(!DeletionPlan::decide(21, 100, T).is_allowed());
    }

    /// The signature of a wrong connection string. Refused whatever the
    /// threshold says — at 100% the threshold is not the thing deciding.
    #[test]
    fn a_source_reporting_nothing_at_all_is_refused_even_above_threshold() {
        let plan = DeletionPlan::decide(100, 100, 0.99);
        assert!(!plan.is_allowed());
        assert!(
            plan.refused
                .as_deref()
                .unwrap()
                .contains("connection string"),
            "the message must name the likely cause: {:?}",
            plan.refused
        );
    }

    /// An operator who means it can proceed. The guard exists to force a
    /// decision, not to make deletion impossible.
    #[test]
    fn an_explicit_threshold_above_one_permits_a_full_sweep() {
        assert!(DeletionPlan::decide(100, 100, 1.5).is_allowed());
    }

    #[test]
    fn a_raised_threshold_permits_a_large_legitimate_drop() {
        assert!(
            !DeletionPlan::decide(40, 100, T).is_allowed(),
            "40% is refused by default"
        );
        assert!(
            DeletionPlan::decide(40, 100, 0.5).is_allowed(),
            "and permitted when the operator raises the bar deliberately"
        );
    }

    /// A refusal that does not say how much it would have deleted leaves the
    /// operator unable to judge whether to override it.
    #[test]
    fn a_refusal_names_the_numbers_behind_it() {
        let plan = DeletionPlan::decide(30, 100, T);
        let message = plan.refused.expect("must refuse");
        assert!(message.contains("30"), "{message}");
        assert!(message.contains("100"), "{message}");
        assert!(message.contains("20%"), "{message}");
    }

    #[test]
    fn the_default_threshold_is_conservative_but_not_zero() {
        // Compile-time: both sides are constants, so a runtime assertion here
        // could only ever fail in a build that already knew it would.
        //
        // Above zero or the guard refuses every run; below half or it is not a
        // guard at all, since a run deleting half an estate is exactly the one
        // that should stop and ask.
        const { assert!(T > 0.0 && T < 0.5) };
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use serde_json::json;

    fn record() -> SourceRecord {
        SourceRecord {
            kind: AssetKind::Table,
            path: vec![
                "hdfc-core".into(),
                "retail".into(),
                "upi_transactions".into(),
            ],
            description: Some("Every UPI payment.".into()),
            properties: Some(json!({ "rows": 12 })),
        }
    }

    mod what_the_source_said {
        use super::*;

        #[test]
        fn the_same_record_fingerprints_the_same_way() {
            assert_eq!(record().source_hash(), record().source_hash());
        }

        #[test]
        fn a_changed_description_changes_the_fingerprint() {
            let mut changed = record();
            changed.description = Some("Something else.".into());

            assert_ne!(record().source_hash(), changed.source_hash());
        }

        #[test]
        fn changed_properties_change_the_fingerprint() {
            let mut changed = record();
            changed.properties = Some(json!({ "rows": 13 }));

            assert_ne!(record().source_hash(), changed.source_hash());
        }

        #[test]
        fn a_changed_kind_changes_the_fingerprint() {
            let mut changed = record();
            changed.kind = AssetKind::Column;

            assert_ne!(record().source_hash(), changed.source_hash());
        }

        /// **The collision a naive join produces.** `["ab", "c"]` and
        /// `["a", "bc"]` are different assets; concatenating their segments
        /// gives both the same bytes, so one would be skipped as unchanged
        /// against the other's fingerprint.
        #[test]
        fn two_paths_that_concatenate_alike_fingerprint_differently() {
            let mut left = record();
            left.path = vec!["ab".into(), "c".into()];
            let mut right = record();
            right.path = vec!["a".into(), "bc".into()];

            assert_ne!(left.source_hash(), right.source_hash());
        }

        /// "The source has nothing to say" and "the source says it is empty"
        /// are different statements, and conflating them skips the write that
        /// clears a description.
        #[test]
        fn an_absent_description_differs_from_an_empty_one() {
            let mut absent = record();
            absent.description = None;
            let mut empty = record();
            empty.description = Some(String::new());

            assert_ne!(absent.source_hash(), empty.source_hash());
        }

        #[test]
        fn absent_properties_differ_from_empty_properties() {
            let mut absent = record();
            absent.properties = None;
            let mut empty = record();
            empty.properties = Some(json!({}));

            assert_ne!(absent.source_hash(), empty.source_hash());
        }

        /// The negative that stops a constant passing everything above: two
        /// genuinely identical records must agree, and the hash must not simply
        /// be a function of one field.
        #[test]
        fn records_differing_only_in_path_still_differ() {
            let mut other = record();
            other.path = vec![
                "hdfc-core".into(),
                "retail".into(),
                "card_settlements".into(),
            ];

            assert_ne!(record().source_hash(), other.source_hash());
        }
    }

    mod deciding_before_the_write {
        use super::*;

        #[test]
        fn an_unknown_fqn_is_created() {
            assert_eq!(
                decide_ingest(Existing::Absent, record().source_hash()),
                Ingest::Create
            );
        }

        #[test]
        fn an_identical_fingerprint_is_skipped() {
            let hash = record().source_hash();

            assert_eq!(
                decide_ingest(Existing::Fingerprinted(hash), hash),
                Ingest::Skip
            );
        }

        #[test]
        fn a_different_fingerprint_is_patched() {
            let mut changed = record();
            changed.description = Some("new".into());

            assert_eq!(
                decide_ingest(
                    Existing::Fingerprinted(record().source_hash()),
                    changed.source_hash()
                ),
                Ingest::Patch
            );
        }

        /// **Absent evidence is not evidence of sameness.** Skipping an asset
        /// with no stored fingerprint would freeze every pre-fingerprinting
        /// asset at whatever it said then — and the freeze would be invisible,
        /// because the run reports success and changes nothing.
        #[test]
        fn an_asset_with_no_fingerprint_is_patched_rather_than_skipped() {
            assert_eq!(
                decide_ingest(Existing::Unfingerprinted, record().source_hash()),
                Ingest::Patch
            );
        }

        /// And the negative for the whole mechanism: skip must be reachable
        /// *only* on an exact match. A `Skip` returned for anything else makes
        /// a re-run silently stop updating the catalog.
        #[test]
        fn nothing_but_an_exact_match_is_skipped() {
            let hash = record().source_hash();
            let mut other = record();
            other.path = vec!["different".into()];

            for existing in [
                Existing::Absent,
                Existing::Unfingerprinted,
                Existing::Fingerprinted(other.source_hash()),
            ] {
                assert_ne!(decide_ingest(existing, hash), Ingest::Skip, "{existing:?}");
            }
        }
    }
}
