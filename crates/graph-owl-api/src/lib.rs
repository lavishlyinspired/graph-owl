use std::sync::Arc;

use chrono::{DateTime, Utc};
use graph_owl_authz::{AccessPredicate, MetadataOperation, Policy, Subject, compile};
use graph_owl_core::projection;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Principal, Relationship, Table, TableUpdate,
    envelope::EntityVersion,
    fqn,
    page::{Page, PageRequest},
    relationship_type::{EntityKind, RelationshipType, is_legal},
};
use graph_owl_engine::TripleStore;
use graph_owl_storage::{ConflictKind, Storage, StorageError, StoredUser};
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, Subgraph, TraversalEngine};
use serde::Deserialize;
use uuid::Uuid;

pub mod validation;
use validation::{
    FieldError, FieldErrorCode, FieldPath, ValidateBody, optional_string, require_non_empty_string,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTable {
    pub name: String,
    pub fully_qualified_name: String,
    pub description: Option<String>,
}

impl ValidateBody for CreateTable {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("fullyQualifiedName"),
            &mut errors,
        );
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertAsset {
    pub kind: AssetKind,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

impl ValidateBody for UpsertAsset {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("kind"), &mut errors);
        if let Some(kind) = value.get("kind").and_then(serde_json::Value::as_str) {
            if AssetKind::parse(kind).is_err() {
                errors.push(FieldError::new(
                    "kind",
                    FieldErrorCode::Type,
                    format!(
                        "`{kind}` is not an asset kind; expected one of: {}",
                        AssetKind::ALL
                            .iter()
                            .map(|k| k.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
        require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRelationship {
    pub to_table_id: Uuid,
    pub relationship_type: String,
}

/// PATCH semantics: every field is optional, so absence is never an error.
/// But a field the client *did* send must still be usable — `name: ""` is a
/// request to blank a required value, not a no-op.
impl ValidateBody for TableUpdate {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value.get("name").is_some_and(|v| !v.is_null()) {
            require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        }
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

/// PATCH: absence is never an error. But a description the client *did* send
/// must be usable — a blank string is a request to clear a field, and explicit
/// null is how that is expressed.
impl ValidateBody for AssetUpdate {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|d| d.trim().is_empty())
        {
            errors.push(FieldError::new(
                "description",
                FieldErrorCode::Empty,
                "`description` must not be blank; send null to clear it",
            ));
        }
        errors
    }
}

impl ValidateBody for CreateRelationship {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("toTableId"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("relationshipType"),
            &mut errors,
        );
        errors
    }
}

/// One error taxonomy for the whole facade.
///
/// Replaces a per-operation error enum. Handlers now *map* a domain failure to
/// a status code rather than each deciding what a failure means, which is what
/// keeps a fifth endpoint from inventing a sixth notion of "not found".
#[derive(Debug)]
pub enum CatalogError {
    /// The addressed entity does not exist.
    NotFound,
    /// A uniqueness constraint rejected the write.
    Conflict {
        detail: String,
        existing_id: Option<Uuid>,
        kind: ConflictKind,
    },
    /// A field-level failure that got past boundary validation, or one that
    /// only the domain can detect.
    Validation(Vec<FieldError>),
    /// The `(from, type, to)` triple is not in the legality table. Distinct
    /// from `Validation` because the *shape* is fine and the *meaning* is not —
    /// a client fixes it by choosing a different relationship, not a different
    /// value.
    IllegalRelationship {
        from: EntityKind,
        relationship: RelationshipType,
        to: EntityKind,
    },
    Storage(StorageError),
}

impl From<StorageError> for CatalogError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Conflict {
                detail,
                existing_id,
                kind,
            } => CatalogError::Conflict {
                detail,
                existing_id,
                kind,
            },
            StorageError::Unexpected(message) => {
                CatalogError::Storage(StorageError::Unexpected(message))
            }
        }
    }
}

fn subject_of(principal: &Principal) -> Subject {
    Subject {
        id: principal.id.clone(),
        roles: principal.roles.clone(),
        is_admin: principal.is_admin,
    }
}

#[derive(Clone)]
pub struct Catalog {
    storage: Arc<dyn Storage>,
    /// The graph view of what `storage` holds. Optional because the catalog is
    /// fully functional without it — that is decision 6 made structural rather
    /// than promised: if the projection were required, a graph outage would be
    /// a catalog outage.
    graph: Option<Arc<dyn TripleStore>>,
    /// The same backend seen through its traversal capability. Two fields
    /// rather than one combined trait, because storing flakes and walking them
    /// are genuinely separate contracts — a backend could reasonably implement
    /// one and not the other.
    traversal: Option<Arc<dyn TraversalEngine>>,
}

impl Catalog {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            graph: None,
            traversal: None,
        }
    }

    /// The catalog, projecting into a graph as it writes.
    #[must_use]
    pub fn with_graph(mut self, graph: Arc<dyn TripleStore>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// The traversal capability of the same backend.
    #[must_use]
    pub fn with_traversal(mut self, traversal: Arc<dyn TraversalEngine>) -> Self {
        self.traversal = Some(traversal);
        self
    }

    /// The neighbourhood around an asset, as a graph.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist or the caller may not see it.
    /// `Storage` if no traversal engine is configured — answering a graph
    /// question with an empty graph would read as "nothing is connected",
    /// which is a wrong answer rather than a missing feature.
    pub async fn asset_subgraph(
        &self,
        principal: &Principal,
        id: Uuid,
        direction: Direction,
        bounds: Bounds,
        as_of: Option<DateTime<Utc>>,
    ) -> Result<Subgraph, CatalogError> {
        // Visibility first, and against relational state — decision 7. The
        // projection lags by design, so a permission revoked in that window
        // would still be honoured by a check that read from the graph.
        self.get_asset_for(principal, id).await?;

        let traversal = self.traversal.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no traversal engine configured".to_string(),
            ))
        })?;

        let as_of_t = match (as_of, &self.graph) {
            (Some(at), Some(graph)) => graph
                .time_at(at)
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?,
            _ => None,
        };

        traversal
            .subgraph(
                &[graph_owl_core::flake::Sid::new(
                    graph_owl_core::flake::namespace::DSC,
                    id.to_string(),
                )],
                direction,
                bounds,
                &EdgeFilter {
                    relationship_types: None,
                    as_of: as_of_t,
                },
            )
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))
    }

    /// Project a relationship into the graph, or withdraw it.
    ///
    /// Same failure isolation as [`project`]: an edge that fails to reach the
    /// graph leaves the relational row intact and the graph view stale.
    ///
    /// [`project`]: Self::project
    async fn project_relationship(&self, relationship: &Relationship, asserting: bool) {
        let Some(graph) = &self.graph else {
            return;
        };

        let outcome = async {
            let t = graph.next_time().await?;
            let flakes = projection::relationship_to_flakes(relationship, t);
            if asserting {
                graph.assert_flakes(&flakes).await
            } else {
                // Every flake of the edge, withdrawn together. Retracting only
                // the endpoints would leave an orphan node still carrying
                // `rdf:type dsc:Relationship` — an edge to nowhere, which a
                // traversal would count and then fail to follow.
                graph.retract_flakes(&flakes).await
            }
        }
        .await;

        if let Err(error) = outcome {
            eprintln!(
                "graph projection failed for relationship {} ({error}). The edge \
                 is intact; the graph view is stale until reconciliation.",
                relationship.id
            );
        }
    }

    /// The asset as it stood at a past instant.
    ///
    /// Reconstructed from the graph rather than read from a snapshot table:
    /// history recoverable *by construction* is the whole claim of the flake
    /// model, and a parallel snapshot table is exactly the thing that can
    /// drift from the facts it claims to summarise.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset did not exist at that instant — including when
    /// the graph is younger than the question. `Unexpected` if no graph is
    /// configured, because silently answering a time-travel question from
    /// current state would be a wrong answer rather than a missing feature.
    pub async fn get_asset_as_of(
        &self,
        id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<Asset, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let t = graph
            .time_at(at)
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?
            // Nothing had happened yet. Distinct from "the entity did not
            // exist", but indistinguishable to a caller asking about one id —
            // and both are honestly a 404 for that id at that instant.
            .ok_or(CatalogError::NotFound)?;

        let flakes = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(graph_owl_core::flake::Sid::new(
                    graph_owl_core::flake::namespace::DSC,
                    id.to_string(),
                )),
                as_of: Some(t),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        projection::asset_from_flakes(id, &flakes).ok_or(CatalogError::NotFound)
    }

    /// Project an asset's new state into the graph, after the relational write
    /// has already succeeded.
    ///
    /// **Never propagates a failure.** Decision 6: relational is the source of
    /// truth, and failing an entity write because its graph projection failed
    /// would make the graph a single point of failure for the catalog. The
    /// entity exists; the graph view catches up.
    ///
    /// `before` is read here rather than passed in, because the diff belongs
    /// to the projection and not to the write path — a caller that had to
    /// supply it would be doing the projection's bookkeeping for it, and would
    /// eventually forget to.
    async fn project(&self, before: Option<Asset>, after: &Asset) {
        let Some(graph) = &self.graph else {
            return;
        };

        let outcome = async {
            let t = graph.next_time().await?;
            let flakes = match &before {
                Some(before) => projection::asset_update_flakes(before, after, t),
                None => projection::asset_to_flakes(after, t),
            };
            // Retractions and assertions go through their own verbs; the flag
            // is not carried on the struct.
            let (retractions, assertions): (Vec<_>, Vec<_>) =
                flakes.into_iter().partition(|f| !f.op);
            graph.retract_flakes(&retractions).await?;
            graph.assert_flakes(&assertions).await
        }
        .await;

        if let Err(error) = outcome {
            // Logged, not returned. A silent failure here would be a drift bug
            // nobody could diagnose; a returned one would be decision 6
            // violated. Epic 4 Slice G turns this into a queued reconciliation.
            eprintln!(
                "graph projection failed for asset {} ({}): {error}. The entity \
                 is intact; the graph view is stale until reconciliation.",
                after.id, after.fully_qualified_name
            );
        }
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails, e.g. a duplicate `fully_qualified_name`.
    pub async fn create_table(
        &self,
        principal: &Principal,
        request: CreateTable,
    ) -> Result<Table, CatalogError> {
        // Epic 3 puts this on the envelope as `updated_by`. Until then the
        // principal is threaded and observable, so Epic 12 changes an extractor
        // rather than forty signatures.
        let _ = principal;
        let now = Utc::now();
        let table = Table {
            id: Uuid::new_v4(),
            name: request.name,
            fully_qualified_name: request.fully_qualified_name,
            description: request.description,
            created_at: now,
            updated_at: now,
        };
        Ok(self.storage.insert_table(table).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_table(&self, id: Uuid) -> Result<Option<Table>, CatalogError> {
        Ok(self.storage.get_table(id).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, CatalogError> {
        Ok(self.storage.list_tables(page).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn update_table(
        &self,
        principal: &Principal,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, CatalogError> {
        let _ = principal;
        Ok(self.storage.update_table(id, update).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_table(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<bool, CatalogError> {
        let _ = principal;
        Ok(self.storage.delete_table(id).await?)
    }

    /// # Errors
    ///
    /// Returns `CatalogError::Validation` if `relationshipType` is not in the
    /// vocabulary, `CatalogError::IllegalRelationship` if the triple is not in the
    /// legality table, `CatalogError::NotFound` if either table doesn't exist, or
    /// `CatalogError::Conflict` if storage rejects it (e.g. a duplicate
    /// relationship).
    pub async fn create_relationship(
        &self,
        principal: &Principal,
        from_table_id: Uuid,
        request: CreateRelationship,
    ) -> Result<Relationship, CatalogError> {
        let _ = principal;
        // Vocabulary and legality are checked *before* existence, deliberately:
        // an illegal triple between two nonexistent tables is a triple problem,
        // and reporting 404 would send the client hunting for the wrong bug.
        let relationship_type =
            RelationshipType::parse(&request.relationship_type).map_err(|unknown| {
                CatalogError::Validation(vec![FieldError::new(
                    "relationshipType",
                    FieldErrorCode::Type,
                    format!(
                        "`{}` is not a relationship type; expected one of: {}",
                        unknown.got,
                        RelationshipType::ALL
                            .iter()
                            .map(|r| r.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )])
            })?;

        let (from, to) = (EntityKind::Table, EntityKind::Table);
        if !is_legal(from, relationship_type, to) {
            return Err(CatalogError::IllegalRelationship {
                from,
                relationship: relationship_type,
                to,
            });
        }

        if self.storage.get_table(from_table_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        if self.storage.get_table(request.to_table_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }

        let relationship = Relationship {
            id: Uuid::new_v4(),
            from_entity_type: from.as_str().to_string(),
            from_entity_id: from_table_id,
            relationship_type: relationship_type.as_str().to_string(),
            to_entity_type: to.as_str().to_string(),
            to_entity_id: request.to_table_id,
            created_at: Utc::now(),
        };

        let created = self.storage.create_relationship(relationship).await?;
        self.project_relationship(&created, true).await;
        Ok(created)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails. Returns `Ok(None)` if the table
    /// itself doesn't exist.
    pub async fn list_relationships_for_table(
        &self,
        table_id: Uuid,
    ) -> Result<Option<Vec<Relationship>>, CatalogError> {
        if self.storage.get_table(table_id).await?.is_none() {
            return Ok(None);
        }

        let relationships = self
            .storage
            .list_relationships_for_entity("table", table_id)
            .await?;
        Ok(Some(relationships))
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_relationship(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<bool, CatalogError> {
        let _ = principal;
        // Read before deleting: a retraction has to name the exact facts it
        // withdraws, and after the row is gone there is nothing left to name
        // them from.
        let existing = self.storage.get_relationship(id).await.unwrap_or(None);
        let deleted = self.storage.delete_relationship(id).await?;
        if deleted {
            if let Some(relationship) = existing {
                self.project_relationship(&relationship, false).await;
            }
        }
        Ok(deleted)
    }

    // ---- asset hierarchy (Epic 2) ----

    /// Creates or converges an asset, deriving its FQN from the parent chain.
    ///
    /// # Errors
    ///
    /// `Validation` if the FQN cannot be derived or the parent is the wrong
    /// kind; `NotFound` if the parent does not exist.
    pub async fn upsert_asset(
        &self,
        principal: &Principal,
        request: UpsertAsset,
    ) -> Result<Asset, CatalogError> {
        let _ = principal;

        // Containment is checked against the *actual* parent, not a claim in
        // the request: a column under a schema is a hierarchy corruption every
        // later traversal has to cope with.
        let parent = match request.parent_id {
            Some(parent_id) => {
                let parent = self
                    .storage
                    .get_asset(parent_id)
                    .await?
                    .ok_or(CatalogError::NotFound)?;
                if request.kind.parent_kind() != Some(parent.kind) {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "parentId",
                        FieldErrorCode::Type,
                        format!(
                            "a `{}` is contained by a `{}`, not a `{}`",
                            request.kind,
                            request
                                .kind
                                .parent_kind()
                                .map_or_else(|| "nothing".to_string(), |k| k.to_string()),
                            parent.kind
                        ),
                    )]));
                }
                Some(parent)
            }
            None => {
                if request.kind.parent_kind().is_some() {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "parentId",
                        FieldErrorCode::Required,
                        format!("a `{}` requires a parent", request.kind),
                    )]));
                }
                None
            }
        };

        let fully_qualified_name = match &parent {
            Some(parent) => fqn::child_of(&parent.fully_qualified_name, &request.name),
            None => fqn::derive(&[&request.name]),
        }
        .map_err(|error| {
            CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Type,
                error.to_string(),
            )])
        })?;

        let now = Utc::now();
        // Read before the write so the projection can diff against it. A
        // create has no prior state and projects its whole self; an upsert
        // over an existing FQN is an update and must retract what it replaces.
        let before = self
            .storage
            .get_asset_by_fqn(&fully_qualified_name)
            .await
            .unwrap_or(None);

        let written = self
            .storage
            .upsert_asset(Asset {
                id: Uuid::new_v4(),
                kind: request.kind,
                name: request.name,
                fully_qualified_name,
                parent_id: request.parent_id,
                description: request.description,
                properties: request.properties,
                version: EntityVersion::initial(),
                updated_by: principal.id.clone(),
                // No diff on the initial version: there was nothing before it,
                // and an empty diff would read as "nothing changed" rather than
                // "this is where it began".
                change_description: None,
                deleted: false,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            })
            .await?;

        self.project(before, &written).await;
        Ok(written)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, CatalogError> {
        Ok(self.storage.get_asset(id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, CatalogError> {
        Ok(self.storage.get_asset_by_fqn(fqn).await?)
    }

    /// Cheapest possible round trip to storage, for readiness.
    ///
    /// # Errors
    ///
    /// Returns an error if storage is unreachable.
    pub async fn ping(&self) -> Result<(), CatalogError> {
        self.storage.ping().await.map_err(Into::into)
    }

    /// Resolves a principal's policies once per request.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn policies_for(&self, principal: &Principal) -> Result<Vec<Policy>, CatalogError> {
        if principal.is_admin {
            return Ok(Vec::new());
        }
        Ok(self.storage.policies_for_roles(&principal.roles).await?)
    }

    async fn predicate_for(
        &self,
        principal: &Principal,
        operation: MetadataOperation,
    ) -> Result<AccessPredicate, CatalogError> {
        let policies = self.policies_for(principal).await?;
        Ok(compile(&subject_of(principal), operation, &policies))
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_assets_for(
        &self,
        principal: &Principal,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .list_assets_visible(kind, page, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn search_assets_for(
        &self,
        principal: &Principal,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .search_assets_visible(query, kind, page, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_children_for(
        &self,
        principal: &Principal,
        parent_id: Option<Uuid>,
    ) -> Result<Vec<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .list_children_visible(parent_id, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn count_assets_by_kind_for(
        &self,
        principal: &Principal,
    ) -> Result<Vec<(AssetKind, i64)>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .count_assets_by_kind_visible(&predicate)
            .await?)
    }

    /// Reads one asset, or `NotFound` if policy hides it.
    ///
    /// **Hidden reads as missing, deliberately.** A `403` on a specific id
    /// confirms that id exists, which is exactly what the policy was meant to
    /// conceal.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist or is not visible.
    pub async fn get_asset_for(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Asset, CatalogError> {
        let asset = self
            .storage
            .get_asset(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        if predicate.admits(&asset.fully_qualified_name) {
            Ok(asset)
        } else {
            Err(CatalogError::NotFound)
        }
    }

    /// Auto-provisions a user on first sight, so ownership works without a
    /// directory sync (`12-13-security.md` decision 7).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn resolve_principal(
        &self,
        id: &str,
        display_name: &str,
    ) -> Result<Principal, CatalogError> {
        let user = match self.storage.find_user(id).await? {
            Some(user) => user,
            None => {
                let user = StoredUser {
                    id: id.to_string(),
                    display_name: display_name.to_string(),
                    email: None,
                    is_admin: false,
                    is_bot: false,
                    roles: Vec::new(),
                };
                self.storage.upsert_user(&user).await?;
                user
            }
        };
        Ok(Principal {
            id: user.id,
            name: user.display_name,
            kind: if user.is_bot {
                graph_owl_core::PrincipalKind::Service
            } else {
                graph_owl_core::PrincipalKind::User
            },
            roles: user.roles,
            is_admin: user.is_admin,
        })
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        Ok(self.storage.list_assets(kind, page).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, CatalogError> {
        Ok(self.storage.list_children(parent_id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, CatalogError> {
        Ok(self.storage.ancestors_of(id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        Ok(self.storage.search_assets(query, kind, page).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, CatalogError> {
        Ok(self.storage.count_assets_by_kind().await?)
    }

    /// Writes one connector record, resolving its path to a parent id.
    ///
    /// # Errors
    ///
    /// `NotFound` if the record's parent has not been written yet — which is a
    /// connector contract violation, since `Connector::fetch` promises parents
    /// before children.
    pub async fn ingest_record(
        &self,
        principal: &Principal,
        kind: AssetKind,
        path: &[String],
        description: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Result<Asset, CatalogError> {
        let parent_id = if path.len() > 1 {
            let parent_fqn = fqn::derive(
                &path[..path.len() - 1]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| {
                CatalogError::Validation(vec![FieldError::new(
                    "path",
                    FieldErrorCode::Type,
                    error.to_string(),
                )])
            })?;
            Some(
                self.storage
                    .get_asset_by_fqn(&parent_fqn)
                    .await?
                    .ok_or(CatalogError::NotFound)?
                    .id,
            )
        } else {
            None
        };

        let name = path.last().cloned().unwrap_or_default();
        self.upsert_asset(
            principal,
            UpsertAsset {
                kind,
                name,
                parent_id,
                description,
                properties,
            },
        )
        .await
    }

    // ---- envelope (Epic 3) ----

    /// Applies a partial update, advancing the version by the size of the change.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    pub async fn update_asset(
        &self,
        principal: &Principal,
        id: Uuid,
        update: &AssetUpdate,
    ) -> Result<Asset, CatalogError> {
        let before = self.storage.get_asset(id).await.unwrap_or(None);
        let updated = self
            .storage
            .update_asset(id, update, &principal.id)
            .await?
            .ok_or(CatalogError::NotFound)?;

        self.project(before, &updated).await;
        Ok(updated)
    }

    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    pub async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, CatalogError> {
        if self.storage.get_asset(id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.asset_versions(id).await?)
    }

    /// Tombstones the asset and its subtree, returning how many were affected.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    pub async fn soft_delete_asset(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<u64, CatalogError> {
        if self.storage.get_asset(id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.soft_delete_asset(id, &principal.id).await?)
    }

    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    pub async fn restore_asset(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<u64, CatalogError> {
        if self.storage.get_asset(id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.restore_asset(id, &principal.id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::page::Cursor;

    /// An asset and everything beneath it. Used by the fake's cascade, which
    /// must match Postgres's recursive CTE or a cascade bug passes here.
    fn descendants(assets: &[Asset], root: Uuid) -> Vec<Uuid> {
        let mut found = vec![root];
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for child in assets.iter().filter(|a| a.parent_id == Some(parent)) {
                if !found.contains(&child.id) {
                    found.push(child.id);
                    frontier.push(child.id);
                }
            }
        }
        found
    }
    use graph_owl_storage::Storage;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub(super) struct InMemoryStorage {
        assets: Mutex<Vec<Asset>>,
        versions: Mutex<Vec<AssetVersion>>,
        users: Mutex<Vec<StoredUser>>,
        policies: Mutex<Vec<Policy>>,
        inserted: Mutex<Vec<Table>>,
        relationships: Mutex<Vec<Relationship>>,
    }

    #[async_trait::async_trait]
    impl Storage for InMemoryStorage {
        async fn ping(&self) -> Result<(), StorageError> {
            Ok(())
        }

        // The fake honours the same identity rule as Postgres: the FQN is the
        // identity, so a re-upsert converges instead of duplicating.
        async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError> {
            let mut assets = self.assets.lock().unwrap();
            if let Some(existing) = assets
                .iter_mut()
                .find(|a| a.fully_qualified_name == asset.fully_qualified_name)
            {
                existing.name = asset.name;
                existing.parent_id = asset.parent_id;
                existing.description = asset.description.or(existing.description.clone());
                existing.properties = asset.properties.or(existing.properties.clone());
                existing.updated_at = asset.updated_at;
                return Ok(existing.clone());
            }
            assets.push(asset.clone());
            Ok(asset)
        }

        async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.id == id)
                .cloned())
        }

        async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.fully_qualified_name == fqn)
                .cloned())
        }

        async fn list_assets(
            &self,
            kind: Option<AssetKind>,
            page: &PageRequest,
        ) -> Result<Page<Asset>, StorageError> {
            let mut assets: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| kind.is_none_or(|k| a.kind == k))
                .cloned()
                .collect();
            assets.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                assets.retain(|a| {
                    (a.fully_qualified_name.as_str(), a.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            assets.truncate(page.limit + 1);
            Ok(Page::from_overfetch(assets, page.limit, |a| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError> {
            let mut children: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.parent_id == parent_id)
                .cloned()
                .collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(children)
        }

        async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError> {
            let assets = self.assets.lock().unwrap().clone();
            let mut chain = Vec::new();
            let mut current = assets.iter().find(|a| a.id == id).cloned();
            while let Some(asset) = current {
                current = asset
                    .parent_id
                    .and_then(|pid| assets.iter().find(|a| a.id == pid).cloned());
                chain.push(asset);
            }
            chain.reverse();
            Ok(chain)
        }

        async fn search_assets(
            &self,
            query: &str,
            kind: Option<AssetKind>,
            page: &PageRequest,
        ) -> Result<Page<Asset>, StorageError> {
            let needle = query.to_lowercase();
            let mut assets: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| {
                    (a.name.to_lowercase().contains(&needle)
                        || a.fully_qualified_name.to_lowercase().contains(&needle))
                        && kind.is_none_or(|k| a.kind == k)
                })
                .cloned()
                .collect();
            assets.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
            assets.truncate(page.limit + 1);
            Ok(Page::from_overfetch(assets, page.limit, |a| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError> {
            let assets = self.assets.lock().unwrap();
            Ok(AssetKind::ALL
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        i64::try_from(assets.iter().filter(|a| a.kind == kind).count())
                            .unwrap_or(i64::MAX),
                    )
                })
                .filter(|(_, n)| *n > 0)
                .collect())
        }

        // The fake honours the same envelope contract as Postgres, including
        // the no-op rule: a fake that always bumped would let a version-inflation
        // bug pass here and fail only against a real database.
        async fn update_asset(
            &self,
            id: Uuid,
            update: &AssetUpdate,
            updated_by: &str,
        ) -> Result<Option<Asset>, StorageError> {
            use graph_owl_core::envelope::{ChangeDescription, ChangeKind, classify};
            let mut assets = self.assets.lock().unwrap();
            let Some(existing) = assets.iter_mut().find(|a| a.id == id) else {
                return Ok(None);
            };
            let before = existing.clone();
            let mut after = before.clone();
            if let Some(description) = &update.description {
                after.description = description.clone();
            }
            let diff = ChangeDescription::between(
                &serde_json::to_value(&before).unwrap_or_default(),
                &serde_json::to_value(&after).unwrap_or_default(),
            );
            let kind = classify(&diff);
            if matches!(kind, ChangeKind::None) {
                return Ok(Some(before));
            }
            after.version = before.version.bump(kind);
            after.updated_by = updated_by.to_string();
            after.change_description = Some(diff.clone());
            after.updated_at = Utc::now();
            *existing = after.clone();
            self.versions.lock().unwrap().push(AssetVersion {
                version: after.version,
                snapshot: after.clone(),
                change_description: Some(diff),
                updated_by: updated_by.to_string(),
                updated_at: after.updated_at,
            });
            Ok(Some(after))
        }

        async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError> {
            let mut versions: Vec<AssetVersion> = self
                .versions
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.snapshot.id == id)
                .cloned()
                .collect();
            versions.sort_by(|a, b| b.version.cmp(&a.version));
            Ok(versions)
        }

        async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError> {
            let mut assets = self.assets.lock().unwrap();
            let subtree = descendants(&assets, id);
            let mut affected = 0;
            for asset in assets
                .iter_mut()
                .filter(|a| subtree.contains(&a.id) && !a.deleted)
            {
                asset.deleted = true;
                asset.deleted_at = Some(Utc::now());
                asset.updated_by = deleted_by.to_string();
                affected += 1;
            }
            Ok(affected)
        }

        async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError> {
            let mut assets = self.assets.lock().unwrap();
            let subtree = descendants(&assets, id);
            let mut affected = 0;
            for asset in assets
                .iter_mut()
                .filter(|a| subtree.contains(&a.id) && a.deleted)
            {
                asset.deleted = false;
                asset.deleted_at = None;
                asset.updated_by = restored_by.to_string();
                affected += 1;
            }
            Ok(affected)
        }

        // The fake applies the *same* AccessPredicate::admits used by the real
        // adapter's reference semantics, so a lowering bug shows as a
        // disagreement rather than passing here and failing in Postgres.
        async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError> {
            Ok(self
                .users
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id == id)
                .cloned())
        }

        async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError> {
            let mut users = self.users.lock().unwrap();
            if let Some(existing) = users.iter_mut().find(|u| u.id == user.id) {
                *existing = user.clone();
            } else {
                users.push(user.clone());
            }
            Ok(())
        }

        async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError> {
            let _ = roles;
            Ok(self.policies.lock().unwrap().clone())
        }

        async fn list_assets_visible(
            &self,
            kind: Option<AssetKind>,
            page: &PageRequest,
            predicate: &AccessPredicate,
        ) -> Result<Page<Asset>, StorageError> {
            let all = self.list_assets(kind, page).await?;
            let visible: Vec<Asset> = all
                .data
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect();
            Ok(Page::from_overfetch(visible, page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn search_assets_visible(
            &self,
            query: &str,
            kind: Option<AssetKind>,
            page: &PageRequest,
            predicate: &AccessPredicate,
        ) -> Result<Page<Asset>, StorageError> {
            let all = self.search_assets(query, kind, page).await?;
            let visible: Vec<Asset> = all
                .data
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect();
            Ok(Page::from_overfetch(visible, page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn list_children_visible(
            &self,
            parent_id: Option<Uuid>,
            predicate: &AccessPredicate,
        ) -> Result<Vec<Asset>, StorageError> {
            Ok(self
                .list_children(parent_id)
                .await?
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect())
        }

        async fn count_assets_by_kind_visible(
            &self,
            predicate: &AccessPredicate,
        ) -> Result<Vec<(AssetKind, i64)>, StorageError> {
            let assets = self.assets.lock().unwrap();
            Ok(AssetKind::ALL
                .into_iter()
                .map(|kind| {
                    let n = assets
                        .iter()
                        .filter(|a| a.kind == kind && predicate.admits(&a.fully_qualified_name))
                        .count();
                    (kind, i64::try_from(n).unwrap_or(i64::MAX))
                })
                .filter(|(_, n)| *n > 0)
                .collect())
        }

        async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
            self.inserted.lock().unwrap().push(table.clone());
            Ok(table)
        }

        async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
            Ok(self
                .inserted
                .lock()
                .unwrap()
                .iter()
                .find(|table| table.id == id)
                .cloned())
        }

        async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
            // The fake honours the same ordering and keyset contract as the
            // Postgres adapter. A fake that returns insertion order would let
            // a pagination bug pass here and fail only against a real database,
            // which is the whole failure mode a port is supposed to prevent.
            let mut tables = self.inserted.lock().unwrap().clone();
            tables.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                tables.retain(|t| {
                    (t.fully_qualified_name.as_str(), t.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            tables.truncate(page.limit + 1);
            Ok(Page::from_overfetch(tables, page.limit, |t| {
                Cursor::new(t.fully_qualified_name.clone(), t.id)
            }))
        }

        async fn update_table(
            &self,
            id: Uuid,
            update: TableUpdate,
        ) -> Result<Option<Table>, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let Some(table) = inserted.iter_mut().find(|table| table.id == id) else {
                return Ok(None);
            };
            if let Some(name) = update.name {
                table.name = name;
            }
            if let Some(description) = update.description {
                table.description = Some(description);
            }
            table.updated_at = Utc::now();
            Ok(Some(table.clone()))
        }

        async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let original_len = inserted.len();
            inserted.retain(|table| table.id != id);
            Ok(inserted.len() != original_len)
        }

        async fn create_relationship(
            &self,
            relationship: Relationship,
        ) -> Result<Relationship, StorageError> {
            self.relationships
                .lock()
                .unwrap()
                .push(relationship.clone());
            Ok(relationship)
        }

        async fn list_relationships_for_entity(
            &self,
            entity_type: &str,
            entity_id: Uuid,
        ) -> Result<Vec<Relationship>, StorageError> {
            Ok(self
                .relationships
                .lock()
                .unwrap()
                .iter()
                .filter(|relationship| {
                    (relationship.from_entity_type == entity_type
                        && relationship.from_entity_id == entity_id)
                        || (relationship.to_entity_type == entity_type
                            && relationship.to_entity_id == entity_id)
                })
                .cloned()
                .collect())
        }

        async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError> {
            let relationships = self.relationships.lock().unwrap();
            Ok(relationships.iter().find(|r| r.id == id).cloned())
        }

        async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut relationships = self.relationships.lock().unwrap();
            let original_len = relationships.len();
            relationships.retain(|relationship| relationship.id != id);
            Ok(relationships.len() != original_len)
        }
    }

    fn mock_create_table_request() -> CreateTable {
        CreateTable {
            name: "customers".to_string(),
            fully_qualified_name: "warehouse.public.customers".to_string(),
            description: None,
        }
    }

    #[tokio::test]
    async fn creating_a_table_assigns_matching_created_and_updated_timestamps() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let table = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_eq!(table.name, "customers");
        assert_eq!(table.fully_qualified_name, "warehouse.public.customers");
        assert_eq!(table.created_at, table.updated_at);
    }

    #[tokio::test]
    async fn creating_two_tables_assigns_different_ids() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let first = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_ne!(first.id, second.id);
    }

    #[tokio::test]
    async fn getting_a_table_by_id_returns_the_stored_table() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");

        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn getting_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let found = catalog
            .get_table(Uuid::new_v4())
            .await
            .expect("get_table should succeed");

        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn listing_tables_with_none_created_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        assert_eq!(page.data, Vec::new());
        assert_eq!(page.paging.after, None);
    }

    #[tokio::test]
    async fn listing_tables_returns_all_created_tables() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let first = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(
                &Principal::system(),
                CreateTable {
                    fully_qualified_name: "warehouse.public.orders".to_string(),
                    ..mock_create_table_request()
                },
            )
            .await
            .expect("create_table should succeed");

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        // Sorted by FQN, so the order is the contract's, not insertion order.
        let mut expected = vec![first, second];
        expected.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        assert_eq!(page.data, expected);
        assert_eq!(page.paging.after, None, "both rows fit in one page");
    }

    #[tokio::test]
    async fn updating_a_table_changes_only_the_provided_fields() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let updated = catalog
            .update_table(
                &Principal::system(),
                created.id,
                TableUpdate {
                    name: None,
                    description: Some("a new description".to_string()),
                },
            )
            .await
            .expect("update_table should succeed")
            .expect("table should exist");

        assert_eq!(updated.name, created.name);
        assert_eq!(updated.description, Some("a new description".to_string()));
        assert_eq!(updated.created_at, created.created_at);
    }

    #[tokio::test]
    async fn updating_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .update_table(&Principal::system(), Uuid::new_v4(), TableUpdate::default())
            .await
            .expect("update_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_table_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let deleted = catalog
            .delete_table(&Principal::system(), created.id)
            .await
            .expect("delete_table should succeed");

        assert!(deleted);
        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_table_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_table(&Principal::system(), Uuid::new_v4())
            .await
            .expect("delete_table should succeed");

        assert!(!deleted);
    }

    #[tokio::test]
    async fn creating_a_relationship_between_two_existing_tables_succeeds() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationship = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        assert_eq!(relationship.from_entity_type, "table");
        assert_eq!(relationship.from_entity_id, from.id);
        assert_eq!(relationship.to_entity_type, "table");
        assert_eq!(relationship.to_entity_id, to.id);
        assert_eq!(relationship.relationship_type, "derivedFrom");
    }

    #[tokio::test]
    async fn creating_a_relationship_from_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                Uuid::new_v4(),
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn creating_a_relationship_to_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: Uuid::new_v4(),
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn creating_a_relationship_with_an_empty_type_is_a_field_validation_error() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: String::new(),
                },
            )
            .await;

        assert!(
            matches!(result, Err(CatalogError::Validation(ref errors))
                if errors.iter().any(|e| e.field == "relationshipType")),
            "an empty type is now an unknown vocabulary member, reported per field"
        );
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_with_none_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let table = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationships = catalog
            .list_relationships_for_table(table.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships, Vec::new());
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_returns_relationships_from_either_side() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let customers = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let archive = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        catalog
            .create_relationship(
                &Principal::system(),
                orders.id,
                CreateRelationship {
                    to_table_id: customers.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");
        catalog
            .create_relationship(
                &Principal::system(),
                archive.id,
                CreateRelationship {
                    to_table_id: orders.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let relationships = catalog
            .list_relationships_for_table(orders.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships.len(), 2);
    }

    #[tokio::test]
    async fn listing_relationships_for_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .list_relationships_for_table(Uuid::new_v4())
            .await
            .expect("list_relationships_for_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_relationship_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let relationship = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let deleted = catalog
            .delete_relationship(&Principal::system(), relationship.id)
            .await
            .expect("delete_relationship should succeed");

        assert!(deleted);
        let remaining = catalog
            .list_relationships_for_table(from.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");
        assert_eq!(remaining, Vec::new());
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_relationship_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_relationship(&Principal::system(), Uuid::new_v4())
            .await
            .expect("delete_relationship should succeed");

        assert!(!deleted);
    }
}

#[cfg(test)]
mod projection_isolation_tests {
    use super::*;
    use async_trait::async_trait;
    use graph_owl_core::flake::{Flake, TriplePattern};
    use graph_owl_engine::EngineError;
    use std::sync::Mutex;
    use tests::InMemoryStorage;

    /// A graph that records what it was asked to do, and can be told to fail.
    struct RecordingGraph {
        fail: bool,
        asserted: Mutex<Vec<Flake>>,
        retracted: Mutex<Vec<Flake>>,
    }

    impl RecordingGraph {
        fn working() -> Arc<Self> {
            Arc::new(Self {
                fail: false,
                asserted: Mutex::new(Vec::new()),
                retracted: Mutex::new(Vec::new()),
            })
        }

        fn broken() -> Arc<Self> {
            Arc::new(Self {
                fail: true,
                asserted: Mutex::new(Vec::new()),
                retracted: Mutex::new(Vec::new()),
            })
        }

        fn refuse<T>(&self) -> Result<T, EngineError> {
            Err(EngineError::Backend("the graph is down".to_string()))
        }
    }

    #[async_trait]
    impl TripleStore for RecordingGraph {
        async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
            if self.fail {
                return self.refuse();
            }
            self.asserted
                .lock()
                .expect("lock")
                .extend_from_slice(flakes);
            Ok(())
        }

        async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
            if self.fail {
                return self.refuse();
            }
            self.retracted
                .lock()
                .expect("lock")
                .extend_from_slice(flakes);
            Ok(())
        }

        async fn query_pattern(&self, _: &TriplePattern) -> Result<Vec<Flake>, EngineError> {
            Ok(Vec::new())
        }

        async fn count(&self, _: &TriplePattern) -> Result<u64, EngineError> {
            Ok(0)
        }

        async fn next_time(&self) -> Result<i64, EngineError> {
            if self.fail {
                return self.refuse();
            }
            Ok(1)
        }

        async fn time_at(
            &self,
            _: chrono::DateTime<chrono::Utc>,
        ) -> Result<Option<i64>, EngineError> {
            Ok(Some(1))
        }
    }

    fn service(name: &str) -> UpsertAsset {
        UpsertAsset {
            kind: AssetKind::Service,
            name: name.to_string(),
            parent_id: None,
            description: None,
            properties: None,
        }
    }

    /// **Decision 6, asserted rather than promised.** Failing an entity write
    /// because its graph projection failed would make the graph a single point
    /// of failure for the catalog — the exact coupling the split exists to
    /// avoid.
    #[tokio::test]
    async fn an_entity_write_survives_a_graph_that_is_down() {
        let catalog =
            Catalog::new(Arc::new(InMemoryStorage::default())).with_graph(RecordingGraph::broken());

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("the entity must be written even though the graph refused");

        assert_eq!(created.name, "hdfc-core");
        assert_eq!(
            catalog
                .get_asset(created.id)
                .await
                .expect("readable")
                .expect("the entity must still exist")
                .name,
            "hdfc-core",
            "and must still be there afterwards"
        );
    }

    #[tokio::test]
    async fn an_update_survives_a_graph_that_is_down() {
        let catalog =
            Catalog::new(Arc::new(InMemoryStorage::default())).with_graph(RecordingGraph::broken());
        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let updated = catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("core banking".to_string())),
                },
            )
            .await
            .expect("the update must land even though the graph refused");

        assert_eq!(updated.description.as_deref(), Some("core banking"));
    }

    /// A catalog with no graph configured must behave exactly as before —
    /// this is what makes the projection genuinely optional rather than
    /// optional-until-something-touches-it.
    #[tokio::test]
    async fn a_catalog_with_no_graph_still_writes() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("no graph configured is not an error");
    }

    #[tokio::test]
    async fn creating_an_asset_projects_its_fields() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let asserted = graph.asserted.lock().expect("lock");
        assert!(!asserted.is_empty(), "a create must project something");
        assert!(
            asserted.iter().all(|f| f.s.id == created.id.to_string()),
            "every flake is about the asset just written"
        );
        assert!(
            asserted.iter().any(|f| f.p.id == "name"),
            "the name is the least a projection can carry: {asserted:?}"
        );
        assert!(
            graph.retracted.lock().expect("lock").is_empty(),
            "a create has nothing to retract"
        );
    }

    /// The update path must withdraw what it replaces. Asserting the new value
    /// without retracting the old leaves both current, and a single-valued
    /// predicate then has two answers.
    #[tokio::test]
    async fn updating_an_asset_retracts_the_value_it_replaces() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("core banking".to_string())),
                },
            )
            .await
            .expect("update");

        let retracted = graph.retracted.lock().expect("lock");
        let asserted = graph.asserted.lock().expect("lock");
        assert!(
            asserted.iter().any(|f| f.p.id == "description"),
            "the new description must be asserted"
        );
        // The version and updatedAt change on every edit, so there is always
        // something to withdraw even when the edited field was previously
        // absent.
        assert!(
            retracted.iter().any(|f| f.p.id == "version"),
            "the superseded version must be withdrawn: {retracted:?}"
        );
    }
}
