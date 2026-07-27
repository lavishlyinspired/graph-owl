pub mod page;
pub mod relationship_type;

use chrono::{DateTime, Utc};
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
