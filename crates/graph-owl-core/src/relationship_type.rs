//! The closed relationship vocabulary and the legality table over it.
//!
//! A free-form `relationship_type: String` lets `Table contains Database` into
//! the graph, where it is not a validation problem but a *modelling* one: every
//! later traversal, inference and lineage query has to cope with an edge that
//! should never have existed. Rejecting it at the boundary is the only cheap
//! place to catch it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Entity kinds an edge can join. Grows with Epic 2 and Epic 34; the point is
/// that it is closed, so `(from, type, to)` is checkable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    /// A table.
    Table,
}

impl EntityKind {
    /// The wire and storage form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Table => "table",
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The stored direction of every relationship this system understands.
///
/// **Append-only, serialized by name** — `01-api-conventions.md` decision 9.
/// These values are persisted, so a variant is never removed or renamed, and
/// the wire form is the string, never the discriminant.
///
/// One stored direction per pair; the inverse is exposed on read rather than
/// stored, because storing both doubles every edge and creates a consistency
/// burden (`00c-domain-model.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RelationshipType {
    /// Hierarchy: a service contains a database, a table contains a column.
    Contains,
    /// Consumption of an asset by something that is not data flow.
    Uses,
    /// Non-data dependency.
    DependsOn,
    /// Directional data flow. The lineage edge.
    Feeds,
    /// Transformation provenance. Separated from `Feeds` deliberately —
    /// lineage explainability needs *flow* and *provenance* apart.
    DerivedFrom,
    /// Resolved duplicate identity (Epic 17). Reversible.
    SameAs,
    /// Undirected association with no stronger meaning available.
    RelatedTo,
}

impl RelationshipType {
    /// Every variant, for exhaustive tests and for the `OpenAPI` enum.
    pub const ALL: [RelationshipType; 7] = [
        RelationshipType::Contains,
        RelationshipType::Uses,
        RelationshipType::DependsOn,
        RelationshipType::Feeds,
        RelationshipType::DerivedFrom,
        RelationshipType::SameAs,
        RelationshipType::RelatedTo,
    ];

    /// The wire and storage form. Stable forever — see the type docs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RelationshipType::Contains => "contains",
            RelationshipType::Uses => "uses",
            RelationshipType::DependsOn => "dependsOn",
            RelationshipType::Feeds => "feeds",
            RelationshipType::DerivedFrom => "derivedFrom",
            RelationshipType::SameAs => "sameAs",
            RelationshipType::RelatedTo => "relatedTo",
        }
    }

    /// What the inverse reads as. Exposed on read, never stored.
    #[must_use]
    pub fn inverse_name(self) -> &'static str {
        match self {
            RelationshipType::Contains => "belongsTo",
            RelationshipType::Uses => "usedBy",
            RelationshipType::DependsOn => "dependencyOf",
            RelationshipType::Feeds => "fedBy",
            RelationshipType::DerivedFrom => "derives",
            // Symmetric: its own inverse.
            RelationshipType::SameAs => "sameAs",
            RelationshipType::RelatedTo => "relatedTo",
        }
    }

    /// # Errors
    ///
    /// Returns [`UnknownRelationshipType`] for any string outside the vocabulary.
    pub fn parse(value: &str) -> Result<Self, UnknownRelationshipType> {
        RelationshipType::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
            .ok_or_else(|| UnknownRelationshipType {
                got: value.to_string(),
            })
    }
}

impl fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The parsed vocabulary rejected a string outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRelationshipType {
    /// The rejected value, so the caller can name it.
    pub got: String,
}

/// Which `(from, type, to)` triples are meaningful.
///
/// Deliberately a table rather than a rule: `Table contains Table` is wrong and
/// `Table feeds Table` is right, and no general predicate distinguishes them.
/// It grows as Epic 2 and Epic 34 add entity kinds.
const LEGAL_TRIPLES: &[(EntityKind, RelationshipType, EntityKind)] = &[
    (
        EntityKind::Table,
        RelationshipType::Feeds,
        EntityKind::Table,
    ),
    (
        EntityKind::Table,
        RelationshipType::DerivedFrom,
        EntityKind::Table,
    ),
    (
        EntityKind::Table,
        RelationshipType::DependsOn,
        EntityKind::Table,
    ),
    (EntityKind::Table, RelationshipType::Uses, EntityKind::Table),
    (
        EntityKind::Table,
        RelationshipType::SameAs,
        EntityKind::Table,
    ),
    (
        EntityKind::Table,
        RelationshipType::RelatedTo,
        EntityKind::Table,
    ),
    // `Contains` is deliberately absent for Table→Table: containment is
    // hierarchy, and a table does not contain a table. Epic 2 adds
    // Service→Database, Database→Schema, Schema→Table, Table→Column.
];

/// Whether this `(from, relationship, to)` triple is in the legality table.
#[must_use]
pub fn is_legal(from: EntityKind, relationship: RelationshipType, to: EntityKind) -> bool {
    LEGAL_TRIPLES.contains(&(from, relationship, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_parses_from_its_own_wire_form() {
        for variant in RelationshipType::ALL {
            assert_eq!(
                RelationshipType::parse(variant.as_str()),
                Ok(variant),
                "{variant} must round-trip"
            );
        }
    }

    #[test]
    fn wire_forms_are_unique() {
        let mut names: Vec<&str> = RelationshipType::ALL.iter().map(|r| r.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two variants share a wire form");
    }

    #[test]
    fn an_unknown_type_is_rejected_and_names_what_it_got() {
        let error = RelationshipType::parse("contain").expect_err("typo must be rejected");
        assert_eq!(
            error.got, "contain",
            "the error must echo the client's value so the typo is visible"
        );
    }

    #[test]
    fn parsing_is_case_sensitive() {
        // The wire form is the contract. Accepting `DERIVEDFROM` would mean two
        // spellings persist for one type and neither is canonical.
        assert!(RelationshipType::parse("DerivedFrom").is_err());
        assert!(RelationshipType::parse("derivedfrom").is_err());
        assert!(RelationshipType::parse("derivedFrom").is_ok());
    }

    #[test]
    fn serde_uses_the_same_wire_form_as_as_str() {
        for variant in RelationshipType::ALL {
            let json = serde_json::to_string(&variant).expect("serializes");
            assert_eq!(
                json,
                format!("\"{}\"", variant.as_str()),
                "serde and as_str must not disagree — they are the same contract"
            );
        }
    }

    #[test]
    fn a_table_may_feed_another_table() {
        assert!(is_legal(
            EntityKind::Table,
            RelationshipType::Feeds,
            EntityKind::Table
        ));
    }

    #[test]
    fn a_table_may_not_contain_a_table() {
        // Containment is hierarchy. This is the case that motivates the table:
        // it is a modelling error, not a typo, and no general rule catches it.
        assert!(!is_legal(
            EntityKind::Table,
            RelationshipType::Contains,
            EntityKind::Table
        ));
    }

    #[test]
    fn the_legality_table_rejects_at_least_one_triple_per_entity_kind() {
        // Guards the mutant that makes `is_legal` return true unconditionally:
        // every other test here would still pass.
        let rejected = RelationshipType::ALL
            .into_iter()
            .filter(|r| !is_legal(EntityKind::Table, *r, EntityKind::Table))
            .count();
        assert!(
            rejected > 0,
            "a legality table that permits everything is not a legality table"
        );
    }

    #[test]
    fn same_as_is_its_own_inverse() {
        assert_eq!(RelationshipType::SameAs.inverse_name(), "sameAs");
        assert_eq!(RelationshipType::RelatedTo.inverse_name(), "relatedTo");
    }

    #[test]
    fn asymmetric_types_have_a_distinct_inverse() {
        for variant in [
            RelationshipType::Contains,
            RelationshipType::Uses,
            RelationshipType::DependsOn,
            RelationshipType::Feeds,
            RelationshipType::DerivedFrom,
        ] {
            assert_ne!(
                variant.inverse_name(),
                variant.as_str(),
                "{variant} is directional and needs a distinct inverse name"
            );
        }
    }
}
