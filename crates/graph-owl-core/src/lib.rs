pub mod envelope;
pub mod fqn;
pub mod page;
pub mod relationship_type;

use chrono::{DateTime, Utc};
use envelope::{ChangeDescription, EntityVersion};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub id: Uuid,
    pub name: String,
    pub fully_qualified_name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub id: Uuid,
    pub from_entity_type: String,
    pub from_entity_id: Uuid,
    pub relationship_type: String,
    pub to_entity_type: String,
    pub to_entity_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// A node in the asset hierarchy: service → database → schema → table → column.
///
/// One struct for all five levels rather than five near-identical structs. What
/// differs between a schema and a table is the *kind* and what may contain it,
/// not the fields — and five structs would mean five repositories, five
/// handlers and five UI pages for one concept (`01-api-conventions.md`
/// decision 10, `39-ui-foundation.md` decision 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: Uuid,
    pub kind: AssetKind,
    pub name: String,
    /// Derived from the parent chain, never client-set.
    pub fully_qualified_name: String,
    pub parent_id: Option<Uuid>,
    pub description: Option<String>,
    /// Free-form, kind-specific: a column's data type, a service's engine.
    /// Kept open because every warehouse reports something slightly different
    /// and normalising it prematurely loses information the catalog is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
    // ---- envelope (Epic 3) ----
    pub version: EntityVersion,
    pub updated_by: String,
    /// What changed to produce this version. `None` on the initial version:
    /// there was nothing before it to diff against, and an empty diff would
    /// read as "nothing changed" rather than "this is where it began".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A past state of an asset, with what produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetVersion {
    pub version: EntityVersion,
    pub snapshot: Asset,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
}

/// A partial update. Absent means "not declared"; explicit `null` means clear.
/// The distinction is what stops a connector's null description from blanking
/// what a human wrote (`15-connectors.md` decision 3).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUpdate {
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
}

fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    Service,
    Database,
    Schema,
    Table,
    Column,
}

impl AssetKind {
    pub const ALL: [AssetKind; 5] = [
        AssetKind::Service,
        AssetKind::Database,
        AssetKind::Schema,
        AssetKind::Table,
        AssetKind::Column,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Service => "service",
            AssetKind::Database => "database",
            AssetKind::Schema => "schema",
            AssetKind::Table => "table",
            AssetKind::Column => "column",
        }
    }

    /// # Errors
    ///
    /// Returns [`UnknownAssetKind`] for a string outside the vocabulary.
    pub fn parse(value: &str) -> Result<Self, UnknownAssetKind> {
        AssetKind::ALL
            .into_iter()
            .find(|k| k.as_str() == value)
            .ok_or_else(|| UnknownAssetKind {
                got: value.to_string(),
            })
    }

    /// What may contain this kind. `None` for a root.
    ///
    /// This is the containment rule in one place, so "a column under a schema"
    /// is rejected once rather than in each of the connector, the API and the UI.
    #[must_use]
    pub fn parent_kind(self) -> Option<AssetKind> {
        match self {
            AssetKind::Service => None,
            AssetKind::Database => Some(AssetKind::Service),
            AssetKind::Schema => Some(AssetKind::Database),
            AssetKind::Table => Some(AssetKind::Schema),
            AssetKind::Column => Some(AssetKind::Table),
        }
    }

    /// Depth from the root, 0-based. Useful to the UI and to cascade logic.
    #[must_use]
    pub fn depth(self) -> usize {
        let mut depth = 0;
        let mut current = self;
        while let Some(parent) = current.parent_kind() {
            current = parent;
            depth += 1;
        }
        depth
    }
}

/// Echoes what was received, so a client sees its own typo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAssetKind {
    pub got: String,
}

impl std::fmt::Display for UnknownAssetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not an asset kind", self.got)
    }
}

impl std::fmt::Display for AssetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod asset_kind_tests {
    use super::*;

    #[test]
    fn every_kind_round_trips() {
        for kind in AssetKind::ALL {
            assert_eq!(AssetKind::parse(kind.as_str()), Ok(kind));
        }
    }

    #[test]
    fn the_hierarchy_is_a_single_chain_from_service_to_column() {
        assert_eq!(AssetKind::Service.parent_kind(), None);
        assert_eq!(AssetKind::Column.parent_kind(), Some(AssetKind::Table));
        assert_eq!(AssetKind::Table.parent_kind(), Some(AssetKind::Schema));
        assert_eq!(AssetKind::Schema.parent_kind(), Some(AssetKind::Database));
        assert_eq!(AssetKind::Database.parent_kind(), Some(AssetKind::Service));
    }

    #[test]
    fn depth_counts_hops_to_the_root() {
        assert_eq!(AssetKind::Service.depth(), 0);
        assert_eq!(AssetKind::Column.depth(), 4);
    }

    #[test]
    fn walking_parent_kinds_always_terminates() {
        // Guards the mutant that makes parent_kind cyclic: depth() would hang.
        for kind in AssetKind::ALL {
            assert!(kind.depth() < AssetKind::ALL.len(), "{kind} loops");
        }
    }

    #[test]
    fn an_unknown_kind_is_rejected() {
        assert!(AssetKind::parse("view").is_err());
    }
}

/// Who is making a request.
///
/// Epic 12 swaps the *extractor*, not this type and not the forty handler
/// signatures that take it. Threading it now, while there are six endpoints,
/// is the whole reason `01-api-conventions.md` decision 6 exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub id: String,
    pub name: String,
    pub kind: PrincipalKind,
    /// Roles carry policies. Resolved once when the principal is built, so
    /// every downstream check reads the same set — a per-check lookup would
    /// let permissions change mid-request.
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrincipalKind {
    /// A human, authenticated by Epic 12.
    User,
    /// A machine with its own credentials — a connector, an agent.
    Service,
    /// graph-owl itself: migrations, reconciliation, scheduled jobs.
    /// Not obtainable from a request, only constructed internally.
    System,
}

impl Principal {
    /// The placeholder identity until Epic 12 lands. Named `system` so that
    /// anything it writes is visibly not attributed to a person — an
    /// unauthenticated write recorded under a plausible username would be
    /// worse than one recorded honestly as machine-made.
    #[must_use]
    pub fn system() -> Self {
        Self {
            id: "system".to_string(),
            name: "system".to_string(),
            kind: PrincipalKind::System,
            roles: Vec::new(),
            // The internal identity: migrations, reconciliation, scheduled
            // jobs. Not obtainable from a request.
            is_admin: true,
        }
    }
}

#[cfg(test)]
mod principal_tests {
    use super::*;

    #[test]
    fn the_placeholder_principal_is_honestly_a_system_identity() {
        let principal = Principal::system();
        assert_eq!(principal.kind, PrincipalKind::System);
        assert_eq!(
            principal.id, "system",
            "an unauthenticated write must not be attributed to a plausible person"
        );
    }

    #[test]
    fn principal_kind_round_trips_by_name() {
        for (kind, wire) in [
            (PrincipalKind::User, "\"user\""),
            (PrincipalKind::Service, "\"service\""),
            (PrincipalKind::System, "\"system\""),
        ] {
            assert_eq!(serde_json::to_string(&kind).expect("serializes"), wire);
        }
    }
}
