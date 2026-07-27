use std::sync::Arc;

use chrono::Utc;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Principal, Relationship, Table, TableUpdate,
    envelope::EntityVersion,
    fqn,
    page::{Page, PageRequest},
    relationship_type::{EntityKind, RelationshipType, is_legal},
};
use graph_owl_storage::{ConflictKind, Storage, StorageError};
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

#[derive(Clone)]
pub struct Catalog {
    storage: Arc<dyn Storage>,
}

impl Catalog {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
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

        Ok(self.storage.create_relationship(relationship).await?)
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
        Ok(self.storage.delete_relationship(id).await?)
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
        Ok(self
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
            .await?)
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
        self.storage
            .update_asset(id, update, &principal.id)
            .await?
            .ok_or(CatalogError::NotFound)
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
    struct InMemoryStorage {
        assets: Mutex<Vec<Asset>>,
        versions: Mutex<Vec<AssetVersion>>,
        inserted: Mutex<Vec<Table>>,
        relationships: Mutex<Vec<Relationship>>,
    }

    #[async_trait::async_trait]
    impl Storage for InMemoryStorage {
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
