//! Drift, made HTTP-queryable — Epic 20 x Epic 42 Slice D.
//!
//! Epic 20's CLI already computes drift by comparing declaration files to
//! live state (`graph_owl_cli::drift::detect`), distinguishing two cases a
//! plain diff cannot: someone edited live state outside the declarations
//! (`LiveEdited`), versus the declarations moved and nothing has applied
//! them yet (`Unapplied`). That computation needs the declaration files,
//! which only exist wherever the CLI ran — so this module is not a second
//! implementation of drift detection, it is the record a pushed report
//! becomes once it exists, for a review queue that has no file access of
//! its own.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which side moved — mirrors `graph_owl_cli::drift::DriftKind` at the wire,
/// since a CLI push and a console read describe the same fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DriftKind {
    /// Live state no longer matches what was last applied — someone changed
    /// it outside the declarations.
    LiveEdited,
    /// The declaration changed and nothing has been applied since.
    Unapplied,
}

/// Where a drift item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DriftStatus {
    /// Reported, not yet decided.
    Pending,
    /// The declared value was written to live state.
    Applied,
    /// Reviewed and deliberately left as-is.
    Ignored,
}

/// One field on one asset, declared and live disagreeing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriftItem {
    /// The drift record's own id — distinct from `asset_id`, since one asset
    /// can have several drifted fields, each its own row.
    pub id: Uuid,
    /// The asset this drift was reported against.
    pub asset_id: Uuid,
    /// Denormalized at read time so a reviewer never has to cross-reference
    /// an id to know what they are looking at.
    pub fully_qualified_name: String,
    /// Which field disagrees between declared and live state.
    pub field: String,
    /// Which side moved.
    pub kind: DriftKind,
    /// The value currently in live state, if any.
    pub live_value: Option<String>,
    /// The value the declaration file names, if any.
    pub declared_value: Option<String>,
    /// Where this item stands in the review queue.
    pub status: DriftStatus,
    /// When the report that produced this item was pushed.
    pub reported_at: DateTime<Utc>,
    /// When `status` last changed away from `pending`, if it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,
    /// Who decided it, if it has been decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// Why, required on `ignored` — see the module doc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One item of a pushed report, before the server has resolved an asset id
/// or assigned a stable identifier — what a CLI (or anything else that has
/// computed drift) sends.
#[derive(Debug, Clone, PartialEq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriftReportItem {
    /// Which asset this item is about — resolved to `asset_id` server-side.
    pub fully_qualified_name: String,
    /// Which field disagrees between declared and live state.
    pub field: String,
    /// Which side moved.
    pub kind: DriftKind,
    /// The value currently in live state, if any.
    pub live_value: Option<String>,
    /// The value the declaration file names, if any.
    pub declared_value: Option<String>,
}

/// Which fields `apply` knows how to write back to live state.
///
/// **Deliberately not every field a declaration can name.** The CLI's own
/// `declared_field_changes` (Epic 20) only ever diffs `description` today,
/// so this is not an artificial restriction — it is the honest current
/// reach of both sides. Extending it means extending the CLI's diff first,
/// or `apply` would claim to fix drift it cannot actually explain.
#[must_use]
pub fn is_applicable_field(field: &str) -> bool {
    field == "description"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_is_applicable() {
        assert!(is_applicable_field("description"));
    }

    #[test]
    fn an_unrecognized_field_is_not_applicable() {
        assert!(!is_applicable_field("owner"));
        assert!(!is_applicable_field(""));
    }
}
