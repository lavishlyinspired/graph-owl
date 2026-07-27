//! Source connectors: the `Connector` trait, the run machinery, and the
//! Postgres reference implementation.
//!
//! **Status**: Epic 15, first vertical slice. Connectors beyond this one are
//! Python, out of process, pushing through the ingestion API — see
//! `plans/00j-language-boundaries.md`. What stays here is the governance part:
//! the trait, run scoping, and the ordering guarantee.

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
        assert!(T > 0.0 && T < 0.5, "got {T}");
    }
}
