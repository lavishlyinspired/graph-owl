//! Epic 29: what feeds what.
//!
//! The two highest-stakes questions in data engineering — *what breaks if I
//! change this* and *where did this number come from* — are both walks over
//! these edges.

use serde::{Deserialize, Serialize};

use crate::AssetKind;
use crate::relationship_type::RelationshipType;

/// Who asserted an edge.
///
/// **Part of an edge's identity, not a property of it.** The same pair may be
/// asserted by a person and by a connector, and those are two facts that must
/// coexist: automation is often wrong about lineage a human knows, and a human
/// is often out of date about lineage automation observes. Collapsing them
/// makes one silently overwrite the other, and which one wins depends on run
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineageSource {
    /// A person said so.
    Manual,
    /// A connector observed it.
    Connector,
    /// Imported from an `OpenLineage` run event — Epic 9 Slice D.
    OpenLineage,
}

impl LineageSource {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LineageSource::Manual => "manual",
            LineageSource::Connector => "connector",
            LineageSource::OpenLineage => "openlineage",
        }
    }

    /// # Errors
    /// The unrecognised value, so the caller can name it.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "manual" => Ok(LineageSource::Manual),
            "connector" => Ok(LineageSource::Connector),
            "openlineage" => Ok(LineageSource::OpenLineage),
            other => Err(other.to_string()),
        }
    }

    /// Every source.
    pub const ALL: [LineageSource; 3] = [
        LineageSource::Manual,
        LineageSource::Connector,
        LineageSource::OpenLineage,
    ];
}

/// What an edge carries beyond its endpoints.
///
/// The `query` is the whole reason lineage is believable: an edge with the SQL
/// that produced it can be checked, and one without is an assertion a reader
/// has to take on trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageDetails {
    /// Who asserted it.
    pub source: LineageSource,
    /// The SQL that produced the edge, if known — what makes the edge
    /// checkable rather than merely asserted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// A human-readable note, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The pipeline that moved the data, if the edge came from one — Epic 34
    /// Slice C. Lineage's missing middle: without this, "table A feeds table
    /// B" says *that* data moved but not *how* — the job, its schedule, its
    /// run history. `query` already carries *how* for a single SQL
    /// transformation; this carries it for a multi-step job that `query`
    /// cannot express as one string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<uuid::Uuid>,
    /// The `OpenLineage` run event's `run.runId` this edge was imported from
    /// — Epic 9 Slice D. What makes re-importing the same event a no-op
    /// rather than a duplicate edge: present only when `source` is
    /// [`LineageSource::OpenLineage`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openlineage_event_id: Option<String>,
}

/// One asserted edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageEdge {
    /// The stable identifier.
    pub id: uuid::Uuid,
    /// The upstream asset.
    pub from_asset_id: uuid::Uuid,
    /// The downstream asset.
    pub to_asset_id: uuid::Uuid,
    /// How the two relate.
    pub relationship: RelationshipType,
    /// Who asserted it, and the evidence.
    #[serde(flatten)]
    pub details: LineageDetails,
    /// When the edge was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Who or what created it.
    pub created_by: String,
}

/// Which asset kinds may carry which lineage relationship.
///
/// A table is what data flows between, and a column is what it flows *through* —
/// column-level lineage is the same edge one level down. Nothing else: a schema
/// does not feed a schema, and allowing it would let a coarse assertion stand in
/// for the specific one somebody actually needed, which is worse than having
/// none because it looks like an answer.
///
/// A table cannot feed a column, or the reverse. Mixing levels makes "what is
/// downstream of this table" return a set whose members are not comparable, and
/// every consumer then has to re-derive the level it wanted.
const LEGAL_LINEAGE: &[(AssetKind, RelationshipType, AssetKind)] = &[
    (AssetKind::Table, RelationshipType::Feeds, AssetKind::Table),
    (
        AssetKind::Table,
        RelationshipType::DerivedFrom,
        AssetKind::Table,
    ),
    (
        AssetKind::Column,
        RelationshipType::Feeds,
        AssetKind::Column,
    ),
    (
        AssetKind::Column,
        RelationshipType::DerivedFrom,
        AssetKind::Column,
    ),
    // Epic 34 Slice A: a dashboard is built on a table, one direction only —
    // nothing flows back from a dashboard into the table it reads.
    (
        AssetKind::Table,
        RelationshipType::Feeds,
        AssetKind::Dashboard,
    ),
    // Epic 34 Slice B: a topic can be either end — a table changed by CDC
    // publishes to a topic, and a topic consumed into a warehouse feeds a
    // table. Both are real, common flows and neither implies the other.
    (AssetKind::Table, RelationshipType::Feeds, AssetKind::Topic),
    (AssetKind::Topic, RelationshipType::Feeds, AssetKind::Table),
    // Epic 34 Slice D: a model's features are derived from table columns —
    // one direction, matching the dashboard case.
    (
        AssetKind::Table,
        RelationshipType::Feeds,
        AssetKind::MlModel,
    ),
    // Epic 34 Slice E: an external-table pattern — a container is queried
    // in place as if it were a table (e.g. an object-store-backed external
    // table). One direction: the container is the source of record.
    (
        AssetKind::Container,
        RelationshipType::Feeds,
        AssetKind::Table,
    ),
];

/// Whether this triple is a lineage edge the catalog will accept.
#[must_use]
pub fn is_legal_lineage(from: AssetKind, relationship: RelationshipType, to: AssetKind) -> bool {
    LEGAL_LINEAGE.contains(&(from, relationship, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod what_may_feed_what {
        use super::*;

        #[test]
        fn a_table_feeds_a_table() {
            assert!(is_legal_lineage(
                AssetKind::Table,
                RelationshipType::Feeds,
                AssetKind::Table
            ));
        }

        #[test]
        fn a_column_feeds_a_column() {
            assert!(is_legal_lineage(
                AssetKind::Column,
                RelationshipType::Feeds,
                AssetKind::Column
            ));
        }

        /// Mixing levels makes "what is downstream of this table" return a set
        /// whose members are not comparable, and every consumer then has to
        /// re-derive the level it wanted.
        #[test]
        fn a_table_does_not_feed_a_column_or_the_reverse() {
            assert!(!is_legal_lineage(
                AssetKind::Table,
                RelationshipType::Feeds,
                AssetKind::Column
            ));
            assert!(!is_legal_lineage(
                AssetKind::Column,
                RelationshipType::Feeds,
                AssetKind::Table
            ));
        }

        /// Epic 34 Slice A: a dashboard is built on a table, one direction
        /// only — a dashboard does not feed the table it reads.
        #[test]
        fn a_table_feeds_a_dashboard_but_not_the_reverse() {
            assert!(is_legal_lineage(
                AssetKind::Table,
                RelationshipType::Feeds,
                AssetKind::Dashboard
            ));
            assert!(!is_legal_lineage(
                AssetKind::Dashboard,
                RelationshipType::Feeds,
                AssetKind::Table
            ));
        }

        /// A coarse assertion standing in for the specific one somebody needed
        /// is worse than none, because it looks like an answer.
        #[test]
        fn containers_do_not_feed_each_other() {
            for kind in [AssetKind::Service, AssetKind::Database, AssetKind::Schema] {
                assert!(
                    !is_legal_lineage(kind, RelationshipType::Feeds, kind),
                    "{kind} must not carry lineage"
                );
            }
        }

        /// And the negative that stops the table being satisfied by "allow
        /// nothing": a relationship that is not lineage is refused even between
        /// kinds that do carry lineage.
        #[test]
        fn a_non_lineage_relationship_is_not_lineage() {
            for relationship in [
                RelationshipType::Contains,
                RelationshipType::SameAs,
                RelationshipType::RelatedTo,
                RelationshipType::Uses,
            ] {
                assert!(
                    !is_legal_lineage(AssetKind::Table, relationship, AssetKind::Table),
                    "{relationship} is not a lineage edge"
                );
            }
        }
    }

    mod who_asserted_it {
        use super::*;

        #[test]
        fn every_source_round_trips_through_its_wire_form() {
            for source in LineageSource::ALL {
                assert_eq!(LineageSource::parse(source.as_str()), Ok(source));
            }
        }

        #[test]
        fn an_unknown_source_is_named_rather_than_defaulted() {
            assert_eq!(LineageSource::parse("guessed"), Err("guessed".to_string()));
        }

        /// Defaulting an unrecognised source to `Manual` would attribute a
        /// machine's guess to a person, which is exactly backwards for the one
        /// field a reader uses to decide how much to trust the edge.
        #[test]
        fn sources_have_distinct_wire_forms() {
            let mut forms: Vec<&str> = LineageSource::ALL.iter().map(|s| s.as_str()).collect();
            forms.sort_unstable();
            let before = forms.len();
            forms.dedup();
            assert_eq!(forms.len(), before);
        }
    }
}
