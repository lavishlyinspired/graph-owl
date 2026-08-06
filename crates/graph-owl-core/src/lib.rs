//! Pure domain types for the graph-owl catalog: entities, envelopes, and the
//! business rules that operate on them — no I/O, no async runtime, no other
//! `graph-owl-*` crate.
//!
//! # Embedding (Epic 37c)
//!
//! This crate is deliberately the smallest thing an embedder can depend on.
//! Every type here is plain synchronous Rust: no `async fn`, no thread
//! spawned, no lock held across a call. An embedder does not need to bring
//! an executor to use anything in this crate directly — only
//! `graph-owl-api`'s `Catalog`, which wraps a `Storage` port, needs one, and
//! only because the port itself is `async`.
//!
//! `scripts/check-embedding-boundary.py` asserts the dependency half of this
//! claim in CI: no I/O crate, no async runtime, no other workspace crate.
//! What it cannot check is *inside* this crate's own code, which is why this
//! module and every item in it carry a doc comment — `#![deny(missing_docs)]`
//! is the compiler holding the other half of the same promise.
//!
//! # Stability
//!
//! This crate is `0.y.z`: under SemVer, any `0.x.0` bump may break the public
//! API, and `0.x.y` is additive or fix-only. There is no separate
//! `#[unstable]` tier — every `pub` item is held to the same
//! `#![deny(missing_docs)]` bar, so "public but not yet promised" is not a
//! state this crate has. `1.0.0` follows Epic 37c Slice F, once the surface
//! is proven to survive a second entity family without changing. See
//! `plans/00b-architecture.md` decision 27.
#![deny(missing_docs)]

pub mod archive;
pub mod blocking;
pub mod classification;
pub mod collaboration;
pub mod contract;
pub mod contradiction;
pub mod custom_property;
pub mod domain;
pub mod drift;
pub mod entity_families;
pub mod envelope;
pub mod extraction;
pub mod extraction_run;
pub mod flake;
pub mod fqn;
pub mod glossary;
pub mod lifecycle;
pub mod lineage;
pub mod memory;
pub mod metric;
pub mod ownership;
pub mod page;
pub mod projection;
pub mod quality;
pub mod recall;
pub mod relationship_type;
pub mod resolution;
pub mod usage;
pub mod webhook;

use chrono::{DateTime, Utc};
use envelope::{ChangeDescription, EntityVersion};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// The pre-Epic-4 table entity — the walking skeleton's own type, kept
/// separate from [`Asset`] rather than merged into it (see `TableUpdate`'s
/// own history in the plans): a table created through this path never
/// projects into the graph.
pub struct Table {
    /// The stable identifier.
    pub id: Uuid,
    /// The table's own name.
    pub name: String,
    /// The full dotted path from the root.
    pub fully_qualified_name: String,
    /// A human-readable description, if one was given.
    pub description: Option<String>,
    /// When the table was first created.
    pub created_at: DateTime<Utc>,
    /// When the table was most recently changed.
    pub updated_at: DateTime<Utc>,
}

/// A partial update to a [`Table`]. Absent means "not declared".
#[derive(utoipa::ToSchema, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableUpdate {
    /// The new name, if changing it.
    pub name: Option<String>,
    /// The new description, if changing it.
    pub description: Option<String>,
}

/// A generic edge between two entities, named by kind rather than typed —
/// Epic 1's walking-skeleton relationship, predating [`Asset`]'s own
/// containment edges.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    /// The stable identifier.
    pub id: Uuid,
    /// The kind of the source entity.
    pub from_entity_type: String,
    /// The source entity.
    pub from_entity_id: Uuid,
    /// The named relationship, e.g. `feeds`.
    pub relationship_type: String,
    /// The kind of the target entity.
    pub to_entity_type: String,
    /// The target entity.
    pub to_entity_id: Uuid,
    /// When the relationship was created.
    pub created_at: DateTime<Utc>,
}

/// A node in the asset hierarchy: service → database → schema → table → column.
///
/// One struct for all five levels rather than five near-identical structs. What
/// differs between a schema and a table is the *kind* and what may contain it,
/// not the fields — and five structs would mean five repositories, five
/// handlers and five UI pages for one concept (`01-api-conventions.md`
/// decision 10, `39-ui-foundation.md` decision 4).
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// The stable identifier.
    pub id: Uuid,
    /// Where this asset sits in the hierarchy.
    pub kind: AssetKind,
    /// The asset's own name — the last segment of its FQN.
    pub name: String,
    /// Derived from the parent chain, never client-set.
    pub fully_qualified_name: String,
    /// The containing asset, or `None` for a root kind.
    pub parent_id: Option<Uuid>,
    /// A human-readable description, if one was given.
    pub description: Option<String>,
    /// Free-form, kind-specific: a column's data type, a service's engine.
    /// Kept open because every warehouse reports something slightly different
    /// and normalising it prematurely loses information the catalog is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
    /// Organization-defined fields — Epic 22.
    ///
    /// **A different field from `properties`, and the separation is
    /// load-bearing.** `properties` is what the *source system* reported and a
    /// connector run replaces it wholesale; `extension` is what the
    /// *organization* added. Had custom properties gone into `properties`, the
    /// next connector run would have silently wiped every hand-curated
    /// `costCenter`.
    ///
    /// `Option` rather than a bare map so that PATCH can tell "leave the bag
    /// alone" from "the bag is now empty" — the same distinction every other
    /// optional field on this envelope draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<serde_json::Map<String, serde_json::Value>>,
    /// Where this asset is in its life — Epic 26.
    ///
    /// **Orthogonal to `deleted`.** A tombstone says the source no longer
    /// reports it; a lifecycle state says what the organization intends. An
    /// asset can be Active and tombstoned (a connector lost sight of it) or
    /// Retired and present (deliberately turned off), and collapsing them would
    /// make both unanswerable.
    #[serde(default = "default_lifecycle")]
    pub lifecycle: crate::lifecycle::LifecycleState,
    /// Why it is going away and what to use instead. Present only while
    /// `lifecycle` is `Deprecated` — the two are written together by the one
    /// method that moves the state, so they cannot disagree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<crate::lifecycle::Deprecation>,
    /// Users and teams accountable for this asset — Epic 11 Slice C.
    ///
    /// **Always serialized, empty when unowned**, never omitted. An unowned asset
    /// is a real and reportable state, and the choice is not cosmetic: `classify`
    /// treats a *removed* field as a breaking change, so a field that disappeared
    /// when the last owner was dropped would make a governance event read as a
    /// schema break.
    #[serde(default)]
    pub owners: Vec<crate::ownership::EntityReference>,
    // ---- envelope (Epic 3) ----
    /// The envelope's version, bumped on every change.
    pub version: EntityVersion,
    /// Who or what made the most recent change.
    pub updated_by: String,
    /// What changed to produce this version. `None` on the initial version:
    /// there was nothing before it to diff against, and an empty diff would
    /// read as "nothing changed" rather than "this is where it began".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    /// Whether the asset is tombstoned — a source no longer reports it.
    pub deleted: bool,
    /// When the asset was tombstoned, if it has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// When the asset was first created.
    pub created_at: DateTime<Utc>,
    /// When the asset was most recently changed.
    pub updated_at: DateTime<Utc>,
}

/// A past state of an asset, with what produced it.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetVersion {
    /// The envelope's version this snapshot was taken at.
    pub version: EntityVersion,
    /// The asset as it stood at this version.
    pub snapshot: Asset,
    /// What changed to produce this version, if this was not the initial one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    /// Who or what made the change.
    pub updated_by: String,
    /// When the change was made.
    pub updated_at: DateTime<Utc>,
}

/// A partial update. Absent means "not declared"; explicit `null` means clear.
/// The distinction is what stops a connector's null description from blanking
/// what a human wrote (`15-connectors.md` decision 3).
#[derive(utoipa::ToSchema, Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUpdate {
    /// The new description. Absent (`None`) leaves it alone; present with an
    /// inner `None` (explicit JSON `null`) clears it.
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// Organization-defined fields — Epic 22 Slice B.
    ///
    /// **Merged per key, not replaced wholesale.** A patch naming
    /// `costCenter` must not silently clear `retentionDays`, and a client that
    /// had to send the whole bag to change one field would be racing every
    /// other client that did the same. A key present with an explicit `null`
    /// clears *that* key; a key absent is untouched — Epic 3's PATCH semantics
    /// applied one level down, where the fields actually are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<serde_json::Map<String, serde_json::Value>>,
}

impl AssetUpdate {
    /// The bag an asset holds after this patch is applied to `before`.
    ///
    /// `None` when the patch does not mention `extension` at all, which is
    /// distinct from a patch that clears every key: the first leaves the column
    /// alone, the second writes an empty bag.
    #[must_use]
    pub fn merged_extension(
        &self,
        before: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        let patch = self.extension.as_ref()?;
        let mut merged = before.cloned().unwrap_or_default();
        for (key, value) in patch {
            if value.is_null() {
                merged.remove(key);
            } else {
                merged.insert(key.clone(), value.clone());
            }
        }
        Some(merged)
    }
}

/// Assets that predate Epic 26 are `Active`, not `Draft`.
///
/// Retroactively marking a whole existing estate `draft` would make the state
/// meaningless on the day it shipped — everything already here got here from a
/// connector or a deliberate write.
fn default_lifecycle() -> crate::lifecycle::LifecycleState {
    crate::lifecycle::LifecycleState::Active
}

fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Where an asset sits in the hierarchy: service → database → schema →
/// table → column.
pub enum AssetKind {
    /// The root of a hierarchy — a database engine, a message broker.
    Service,
    /// A database within a service.
    Database,
    /// A schema within a database.
    Schema,
    /// A table within a schema.
    Table,
    /// A column within a table.
    Column,
    /// The root of a dashboarding tool — Epic 34 Slice A.
    DashboardService,
    /// A dashboard within a dashboarding tool.
    Dashboard,
    /// A chart within a dashboard.
    Chart,
    /// The root of a message broker — Epic 34 Slice B.
    MessagingService,
    /// A topic within a message broker.
    Topic,
    /// One field of a topic's message schema — the column machinery, under
    /// a topic instead of a table.
    TopicField,
    /// The root of an orchestration tool — Epic 34 Slice C.
    PipelineService,
    /// A pipeline within an orchestration tool.
    Pipeline,
    /// A task within a pipeline; tasks form a DAG via `downstreamTasks` in
    /// `properties`, not via containment.
    Task,
    /// The root of a model registry — Epic 34 Slice D.
    MlModelService,
    /// A model within a model registry.
    MlModel,
    /// A feature of a model, sourced from table columns by FQN.
    Feature,
    /// The root of an object store — Epic 34 Slice E.
    StorageService,
    /// A container within an object store; containers nest via `contains`,
    /// and a [`AssetKind::Column`] may live directly under one too (reusing
    /// the column machinery for structured formats like Parquet or Avro).
    /// Both are a second valid parent `parent_kind` cannot express — see
    /// decision 28 in `00b-architecture.md` for the resulting gap in the
    /// projection predicate's naming.
    Container,
}

impl AssetKind {
    /// Every kind, root to leaf.
    ///
    /// **One flat list across every family**, not five-per-chain: nothing in
    /// this project's containment, envelope, search, or authz machinery reads
    /// this list as a single hierarchy — `parent_kind` is what defines each
    /// family's chain, and `ALL` is only ever used to enumerate or validate
    /// against, so the database hierarchy and the dashboard hierarchy share
    /// it without implying either contains the other.
    pub const ALL: [AssetKind; 19] = [
        AssetKind::Service,
        AssetKind::Database,
        AssetKind::Schema,
        AssetKind::Table,
        AssetKind::Column,
        AssetKind::DashboardService,
        AssetKind::Dashboard,
        AssetKind::Chart,
        AssetKind::MessagingService,
        AssetKind::Topic,
        AssetKind::TopicField,
        AssetKind::PipelineService,
        AssetKind::Pipeline,
        AssetKind::Task,
        AssetKind::MlModelService,
        AssetKind::MlModel,
        AssetKind::Feature,
        AssetKind::StorageService,
        AssetKind::Container,
    ];

    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Service => "service",
            AssetKind::Database => "database",
            AssetKind::Schema => "schema",
            AssetKind::Table => "table",
            AssetKind::Column => "column",
            AssetKind::DashboardService => "dashboardService",
            AssetKind::Dashboard => "dashboard",
            AssetKind::Chart => "chart",
            AssetKind::MessagingService => "messagingService",
            AssetKind::Topic => "topic",
            AssetKind::TopicField => "topicField",
            AssetKind::PipelineService => "pipelineService",
            AssetKind::Pipeline => "pipeline",
            AssetKind::Task => "task",
            AssetKind::MlModelService => "mlModelService",
            AssetKind::MlModel => "mlModel",
            AssetKind::Feature => "feature",
            AssetKind::StorageService => "storageService",
            AssetKind::Container => "container",
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
            AssetKind::DashboardService => None,
            AssetKind::Dashboard => Some(AssetKind::DashboardService),
            AssetKind::Chart => Some(AssetKind::Dashboard),
            AssetKind::MessagingService => None,
            AssetKind::Topic => Some(AssetKind::MessagingService),
            AssetKind::TopicField => Some(AssetKind::Topic),
            AssetKind::PipelineService => None,
            AssetKind::Pipeline => Some(AssetKind::PipelineService),
            AssetKind::Task => Some(AssetKind::Pipeline),
            AssetKind::MlModelService => None,
            AssetKind::MlModel => Some(AssetKind::MlModelService),
            AssetKind::Feature => Some(AssetKind::MlModel),
            AssetKind::StorageService => None,
            // The *declared* parent — used by `depth()`, the flake
            // projection's predicate naming, and `graph-owl-cli`'s
            // declarative validator, none of which can express "one of
            // several kinds". A container's *actual* parent may also be
            // another container (nesting, nesting again) — see
            // `Catalog::upsert_asset`'s Container special-case, and the
            // decision recorded in `00b-architecture.md` for the resulting
            // gap: a nested container's projected containment predicate
            // reads `parentStorageService` even when its real parent is a
            // container. Self-referential (`Some(Container)`) was rejected
            // for this — `depth()` walks `parent_kind()` to termination, and
            // a self-referential value would not terminate.
            AssetKind::Container => Some(AssetKind::StorageService),
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
    /// The string that did not match any kind.
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

    /// Epic 34 Slice A: a second, independent chain. Independent, not merged
    /// into the database hierarchy's root — a dashboard is not contained by a
    /// database engine, and forcing one root would make "list every root"
    /// return two unrelated things under one name.
    #[test]
    fn the_dashboard_family_is_its_own_chain_rooted_independently() {
        assert_eq!(AssetKind::DashboardService.parent_kind(), None);
        assert_eq!(
            AssetKind::Dashboard.parent_kind(),
            Some(AssetKind::DashboardService)
        );
        assert_eq!(AssetKind::Chart.parent_kind(), Some(AssetKind::Dashboard));
        assert_eq!(AssetKind::Chart.depth(), 2);
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
    /// The stable identifier — a user id, a service account name.
    pub id: String,
    /// A human-readable name.
    pub name: String,
    /// What kind of caller this is.
    pub kind: PrincipalKind,
    /// Roles carry policies. Resolved once when the principal is built, so
    /// every downstream check reads the same set — a per-check lookup would
    /// let permissions change mid-request.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Whether this principal bypasses policy entirely.
    #[serde(default)]
    pub is_admin: bool,
}

/// What kind of caller a [`Principal`] is.
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

    // ---- Epic 22 Slice B: the PATCH merge ----

    /// **The criterion the whole merge exists for.** A patch naming one custom
    /// property must not clear the others — a client forced to send the whole
    /// bag to change one field is racing every other client doing the same,
    /// and the loser's value disappears with nothing failing.
    #[test]
    fn a_patch_naming_one_property_leaves_the_others_alone() {
        let before: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({
                "costCenter": "CC-1", "retentionDays": 30
            }))
            .expect("a bag");
        let update = AssetUpdate {
            description: None,
            extension: serde_json::from_value(serde_json::json!({ "costCenter": "CC-2" })).ok(),
        };

        let merged = update.merged_extension(Some(&before)).expect("a merge");

        assert_eq!(merged["costCenter"], serde_json::json!("CC-2"));
        assert_eq!(
            merged["retentionDays"],
            serde_json::json!(30),
            "an unmentioned property must survive the patch"
        );
    }

    /// An explicit null clears **that** key and nothing else — Epic 3's
    /// absent-versus-null distinction, one level down.
    #[test]
    fn an_explicit_null_clears_only_the_key_it_names() {
        let before: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({
                "costCenter": "CC-1", "retentionDays": 30
            }))
            .expect("a bag");
        let update = AssetUpdate {
            description: None,
            extension: serde_json::from_value(serde_json::json!({ "costCenter": null })).ok(),
        };

        let merged = update.merged_extension(Some(&before)).expect("a merge");

        assert!(!merged.contains_key("costCenter"));
        assert_eq!(merged["retentionDays"], serde_json::json!(30));
    }

    /// **Absent is not the same as empty.** A patch that says nothing about
    /// `extension` must leave the column alone; one that clears every key
    /// writes an empty bag. Collapsing the two would make every description
    /// edit wipe the organization's fields.
    #[test]
    fn a_patch_that_does_not_mention_extension_changes_nothing() {
        let before: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({ "costCenter": "CC-1" })).expect("a bag");
        let update = AssetUpdate {
            description: Some(Some("new".to_string())),
            extension: None,
        };

        assert!(
            update.merged_extension(Some(&before)).is_none(),
            "an absent `extension` is a decision not to touch it"
        );
    }

    /// A first value on an asset that had no bag at all still lands.
    #[test]
    fn a_patch_onto_an_absent_bag_creates_it() {
        let update = AssetUpdate {
            description: None,
            extension: serde_json::from_value(serde_json::json!({ "costCenter": "CC-1" })).ok(),
        };

        let merged = update.merged_extension(None).expect("a merge");

        assert_eq!(merged["costCenter"], serde_json::json!("CC-1"));
    }
}
