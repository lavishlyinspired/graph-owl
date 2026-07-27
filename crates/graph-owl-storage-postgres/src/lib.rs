use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    envelope::{ChangeDescription, EntityVersion, classify},
    page::{Cursor, Page, PageRequest},
};
use graph_owl_storage::{ConflictKind, Storage, StorageError, StoredUser, UpdateOutcome};
use sqlx::{PgPool, Row, postgres::PgRow};
use uuid::Uuid;

mod embedded {
    refinery::embed_migrations!("migrations");
}

const UNIQUE_VIOLATION: &str = "23505";

// Takes PgRow by value so it can be passed directly as a fn pointer to
// Option::map/Iterator::map at both call sites, instead of a wrapping closure.
#[allow(clippy::needless_pass_by_value)]
fn table_from_row(row: PgRow) -> Table {
    Table {
        id: row.get("id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        description: row.get("description"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn relationship_from_row(row: PgRow) -> Relationship {
    Relationship {
        id: row.get("id"),
        from_entity_type: row.get("from_entity_type"),
        from_entity_id: row.get("from_entity_id"),
        relationship_type: row.get("relationship_type"),
        to_entity_type: row.get("to_entity_type"),
        to_entity_id: row.get("to_entity_id"),
        created_at: row.get("created_at"),
    }
}

pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    /// # Errors
    ///
    /// Returns `StorageError::Unexpected` if the connection or migrations fail.
    pub async fn connect(connection_string: &str) -> Result<Self, StorageError> {
        let pool = PgPool::connect(connection_string)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let (mut migration_client, connection) =
            tokio_postgres::connect(connection_string, tokio_postgres::NoTls)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        tokio::spawn(connection);

        embedded::migrations::runner()
            .run_async(&mut migration_client)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Self { pool })
    }
}

#[async_trait]
impl Storage for PostgresStorage {
    async fn ping(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
        let result = sqlx::query(
            "INSERT INTO tables (id, name, fully_qualified_name, description, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(table.id)
        .bind(&table.name)
        .bind(&table.fully_qualified_name)
        .bind(&table.description)
        .bind(table.created_at)
        .bind(table.updated_at)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            return Err(match &e {
                sqlx::Error::Database(db_err)
                    if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) =>
                {
                    // Second query only on the error path: name the row that was
                    // already there so the caller can act on it.
                    let existing_id =
                        sqlx::query_scalar("SELECT id FROM tables WHERE fully_qualified_name = $1")
                            .bind(&table.fully_qualified_name)
                            .fetch_optional(&self.pool)
                            .await
                            .ok()
                            .flatten();
                    StorageError::Conflict {
                        detail: table.fully_qualified_name.clone(),
                        existing_id,
                        kind: ConflictKind::Fqn,
                    }
                }
                _ => StorageError::Unexpected(e.to_string()),
            });
        }

        Ok(table)
    }

    async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, fully_qualified_name, description, created_at, updated_at
             FROM tables WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(table_from_row))
    }

    async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
        // Overfetch by one: the extra row answers "is there a next page" without
        // a second COUNT, and is dropped before the page is returned.
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);

        // Keyset, not OFFSET. The row comparison `(fqn, id) > ($1, $2)` is a
        // single index-ordered seek and is stable under concurrent insert;
        // OFFSET re-counts from the start and shifts under any earlier insert.
        let rows = match &page.after {
            Some(cursor) => {
                sqlx::query(
                    "SELECT id, name, fully_qualified_name, description, created_at, updated_at
                     FROM tables
                     WHERE (fully_qualified_name, id) > ($1, $2)
                     ORDER BY fully_qualified_name, id
                     LIMIT $3",
                )
                .bind(&cursor.sort_key)
                .bind(cursor.id)
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT id, name, fully_qualified_name, description, created_at, updated_at
                     FROM tables
                     ORDER BY fully_qualified_name, id
                     LIMIT $1",
                )
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let tables: Vec<Table> = rows.into_iter().map(table_from_row).collect();
        Ok(Page::from_overfetch(tables, page.limit, |table| {
            Cursor::new(table.fully_qualified_name.clone(), table.id)
        }))
    }

    async fn update_table(
        &self,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, StorageError> {
        let row = sqlx::query(
            "UPDATE tables
             SET name = COALESCE($2, name),
                 description = COALESCE($3, description),
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, fully_qualified_name, description, created_at, updated_at",
        )
        .bind(id)
        .bind(&update.name)
        .bind(&update.description)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(table_from_row))
    }

    async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM tables WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn create_relationship(
        &self,
        relationship: Relationship,
    ) -> Result<Relationship, StorageError> {
        sqlx::query(
            "INSERT INTO entity_relationships
                (id, from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(relationship.id)
        .bind(&relationship.from_entity_type)
        .bind(relationship.from_entity_id)
        .bind(&relationship.relationship_type)
        .bind(&relationship.to_entity_type)
        .bind(relationship.to_entity_id)
        .bind(relationship.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                StorageError::Conflict {
                    detail: format!(
                        "{}:{} -{}-> {}:{}",
                        relationship.from_entity_type,
                        relationship.from_entity_id,
                        relationship.relationship_type,
                        relationship.to_entity_type,
                        relationship.to_entity_id
                    ),
                    existing_id: None,
                    kind: ConflictKind::RelationshipTuple,
                }
            }
            _ => StorageError::Unexpected(e.to_string()),
        })?;

        Ok(relationship)
    }

    async fn list_relationships_for_entity(
        &self,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<Relationship>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id, created_at
             FROM entity_relationships
             WHERE (from_entity_type = $1 AND from_entity_id = $2)
                OR (to_entity_type = $1 AND to_entity_id = $2)",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.into_iter().map(relationship_from_row).collect())
    }

    async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError> {
        sqlx::query("SELECT * FROM entity_relationships WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(relationship_from_row))
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM entity_relationships WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    // ---- asset hierarchy ----

    async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError> {
        // ON CONFLICT on the FQN, because the FQN *is* the identity: a
        // connector re-run supplies a fresh Uuid every time, and treating that
        // as a new entity would duplicate the whole warehouse nightly.
        // COALESCE on description keeps human curation: a source reporting
        // NULL means "I have nothing to say", not "blank what a person wrote"
        // (15-connectors.md decision 3).
        let row = sqlx::query(&format!(
            "INSERT INTO assets (id, kind, name, fully_qualified_name, parent_id, description,
                 properties, version_major, version_minor, updated_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, 1, $10, $8, $9)
             ON CONFLICT (fully_qualified_name) DO UPDATE SET
                 name = EXCLUDED.name,
                 parent_id = EXCLUDED.parent_id,
                 description = COALESCE(EXCLUDED.description, assets.description),
                 properties = COALESCE(EXCLUDED.properties, assets.properties),
                 updated_by = EXCLUDED.updated_by,
                 -- A re-ingest of a live asset does not resurrect a tombstone:
                 -- deletion is a governance decision and a connector must not
                 -- silently reverse it.
                 updated_at = now()
             RETURNING {ASSET_COLUMNS}"
        ))
        .bind(asset.id)
        .bind(asset.kind.as_str())
        .bind(&asset.name)
        .bind(&asset.fully_qualified_name)
        .bind(asset.parent_id)
        .bind(&asset.description)
        .bind(&asset.properties)
        .bind(asset.created_at)
        .bind(asset.updated_at)
        .bind(&asset.updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(asset_from_row(row))
    }

    async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!("SELECT {ASSET_COLUMNS} FROM assets WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(asset_from_row))
    }

    async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets WHERE fully_qualified_name = $1"
        ))
        .bind(fqn)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(asset_from_row))
    }

    async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND ($1::text IS NULL OR kind = $1)
               AND ($2::text IS NULL OR (fully_qualified_name, id) > ($2, $3))
             ORDER BY fully_qualified_name, id
             LIMIT $4"
        );
        let query = sqlx::query(&sql)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.asset_page(query, page).await
    }

    async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted AND (($1::uuid IS NULL AND parent_id IS NULL) OR parent_id = $1)
             ORDER BY name"
        ))
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError> {
        // Recursive CTE walking parent_id upward, then reversed so callers get
        // root-first — which is the order a breadcrumb renders in.
        let rows = sqlx::query(&format!(
            "WITH RECURSIVE chain AS (
                 SELECT {ASSET_COLUMNS}, 0 AS hops FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id, a.kind, a.name, a.fully_qualified_name, a.parent_id,
                        a.description, a.properties, a.version_major, a.version_minor,
                        a.updated_by, a.change_description, a.deleted, a.deleted_at,
                        a.created_at, a.updated_at, c.hops + 1
                 FROM assets a JOIN chain c ON a.id = c.parent_id
             )
             SELECT {ASSET_COLUMNS} FROM chain ORDER BY hops DESC"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let pattern = format!("%{}%", query.to_lowercase());
        let sql = format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND (lower(name) LIKE $1 OR lower(fully_qualified_name) LIKE $1)
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR (fully_qualified_name, id) > ($3, $4))
             ORDER BY fully_qualified_name, id
             LIMIT $5"
        );
        let q = sqlx::query(&sql)
            .bind(pattern)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.asset_page(q, page).await
    }

    async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError> {
        // `fqn = prefix OR fqn LIKE prefix || '.%'` rather than a bare prefix
        // match: `hdfc-core` must not also match a service called
        // `hdfc-core-archive`, which a plain LIKE would sweep into the scope
        // and then delete.
        sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE deleted = FALSE AND (fully_qualified_name = $1
                                        OR fully_qualified_name LIKE $1 || '.%')"
        ))
        .bind(prefix)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(asset_from_row).collect())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let rows =
            sqlx::query("SELECT kind, count(*) AS n FROM assets WHERE NOT deleted GROUP BY kind")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                AssetKind::parse(row.get::<&str, _>("kind"))
                    .ok()
                    .map(|kind| (kind, row.get::<i64, _>("n")))
            })
            .collect())
    }

    // ---- envelope (Epic 3) ----

    async fn update_asset(
        &self,
        id: Uuid,
        update: &AssetUpdate,
        updated_by: &str,
        expected_version: Option<EntityVersion>,
    ) -> Result<UpdateOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let before_row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets WHERE id = $1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(before_row) = before_row else {
            return Ok(UpdateOutcome::NotFound);
        };
        let before = asset_from_row(before_row);

        // Compared under the row lock taken above, so no writer can slip
        // between the check and the write.
        if expected_version.is_some_and(|expected| before.version != expected) {
            return Ok(UpdateOutcome::VersionMismatch(before.version));
        }

        // Absent means "not declared"; explicit null means clear. Collapsing
        // them would let a connector's null description blank what a human
        // wrote (15-connectors.md decision 3).
        let mut after = before.clone();
        if let Some(description) = &update.description {
            after.description = description.clone();
        }

        let diff = ChangeDescription::between(
            &serde_json::to_value(&before).unwrap_or_default(),
            &serde_json::to_value(&after).unwrap_or_default(),
        );
        let kind = classify(&diff);
        if matches!(kind, graph_owl_core::envelope::ChangeKind::None) {
            // No version, no history row, no event. This is what makes a
            // connector re-run over an unchanged source observable.
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(UpdateOutcome::Updated(Box::new(before)));
        }

        let next = before.version.bump(kind);
        let updated_row = sqlx::query(&format!(
            "UPDATE assets SET description = $2, version_major = $3, version_minor = $4,
                 updated_by = $5, change_description = $6, updated_at = now()
             WHERE id = $1
             RETURNING {ASSET_COLUMNS}"
        ))
        .bind(id)
        .bind(&after.description)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(updated_by)
        .bind(serde_json::to_value(&diff).ok())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let updated = asset_from_row(updated_row);

        // The snapshot is the state *after* the change, so replaying history
        // never requires applying diffs forward from the beginning.
        sqlx::query(
            "INSERT INTO asset_versions
                 (asset_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&updated).unwrap_or_default())
        .bind(serde_json::to_value(&diff).ok())
        .bind(updated_by)
        .bind(updated.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(UpdateOutcome::Updated(Box::new(updated)))
    }

    async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError> {
        let rows = sqlx::query(
            "SELECT version_major, version_minor, snapshot, change_description, updated_by, updated_at
             FROM asset_versions WHERE asset_id = $1
             ORDER BY version_major DESC, version_minor DESC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(AssetVersion {
                    version: EntityVersion {
                        major: u32::try_from(row.get::<i32, _>("version_major")).ok()?,
                        minor: u32::try_from(row.get::<i32, _>("version_minor")).ok()?,
                    },
                    snapshot: serde_json::from_value(row.get("snapshot")).ok()?,
                    change_description: row
                        .get::<Option<serde_json::Value>, _>("change_description")
                        .and_then(|v| serde_json::from_value(v).ok()),
                    updated_by: row.get("updated_by"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect())
    }

    async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError> {
        // Cascades down the subtree: a live column under a tombstoned table is
        // reachable by search and addresses an asset that no longer exists.
        let result = sqlx::query(
            "WITH RECURSIVE subtree AS (
                 SELECT id FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id FROM assets a JOIN subtree s ON a.parent_id = s.id
             )
             UPDATE assets SET deleted = TRUE, deleted_at = now(), updated_by = $2, updated_at = now()
             WHERE id IN (SELECT id FROM subtree) AND NOT deleted",
        )
        .bind(id)
        .bind(deleted_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError> {
        let result = sqlx::query(
            "WITH RECURSIVE subtree AS (
                 SELECT id FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id FROM assets a JOIN subtree s ON a.parent_id = s.id
             )
             UPDATE assets SET deleted = FALSE, deleted_at = NULL, updated_by = $2, updated_at = now()
             WHERE id IN (SELECT id FROM subtree) AND deleted",
        )
        .bind(id)
        .bind(restored_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected())
    }

    // ---- identity and policy (Epics 11-13) ----

    async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError> {
        let row = sqlx::query(
            "SELECT u.id, u.display_name, u.email, u.is_admin, u.is_bot,
                    COALESCE(array_agg(r.role) FILTER (WHERE r.role IS NOT NULL), '{}') AS roles
             FROM users u LEFT JOIN user_roles r ON r.user_id = u.id
             WHERE u.id = $1
             GROUP BY u.id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|row| StoredUser {
            id: row.get("id"),
            display_name: row.get("display_name"),
            email: row.get("email"),
            is_admin: row.get("is_admin"),
            is_bot: row.get("is_bot"),
            roles: row.get("roles"),
        }))
    }

    async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO users (id, display_name, email, is_admin, is_bot)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 email = EXCLUDED.email,
                 is_admin = EXCLUDED.is_admin,
                 is_bot = EXCLUDED.is_bot",
        )
        .bind(&user.id)
        .bind(&user.display_name)
        .bind(&user.email)
        .bind(user.is_admin)
        .bind(user.is_bot)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for role in &user.roles {
            sqlx::query("INSERT INTO roles (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(role)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query(
                "INSERT INTO user_roles (user_id, role) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(&user.id)
            .bind(role)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError> {
        if roles.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT DISTINCT p.name, p.rules
             FROM policies p JOIN role_policies rp ON rp.policy = p.name
             WHERE rp.role = ANY($1)",
        )
        .bind(roles)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(Policy {
                    name: row.get("name"),
                    rules: serde_json::from_value(row.get("rules")).ok()?,
                })
            })
            .collect())
    }

    async fn list_assets_visible(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            // Nothing visible. An empty page, not an error — "you may see
            // nothing here" is a legitimate answer, and 403 would leak that
            // something exists.
            return Ok(Page::from_overfetch(Vec::new(), page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND ($1::text IS NULL OR kind = $1)
               AND ($2::text IS NULL OR (fully_qualified_name, id) > ($2, $3))
               {VISIBILITY}
             ORDER BY fully_qualified_name, id
             LIMIT $4"
        );
        let query = sqlx::query(&sql)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch)
            .bind(&allow)
            .bind(&deny);
        self.asset_page(query, page).await
    }

    async fn search_assets_visible(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Page::from_overfetch(Vec::new(), page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let pattern = format!("%{}%", query.to_lowercase());
        let sql = format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND (lower(name) LIKE $1 OR lower(fully_qualified_name) LIKE $1)
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR (fully_qualified_name, id) > ($3, $4))
               {VISIBILITY_SEARCH}
             ORDER BY fully_qualified_name, id
             LIMIT $5"
        );
        let q = sqlx::query(&sql)
            .bind(pattern)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch)
            .bind(&allow)
            .bind(&deny);
        self.asset_page(q, page).await
    }

    async fn list_children_visible(
        &self,
        parent_id: Option<Uuid>,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND (($1::uuid IS NULL AND parent_id IS NULL) OR parent_id = $1)
               AND (fully_qualified_name LIKE ANY($2))
               AND NOT (fully_qualified_name LIKE ANY($3))
             ORDER BY name"
        ))
        .bind(parent_id)
        .bind(&allow)
        .bind(&deny)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn count_documented_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<(i64, i64), StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok((0, 0));
        };
        // Both counts in one statement. Two queries could observe different
        // states and produce a coverage ratio above 1.
        //
        // `btrim(description) <> ''` rather than `IS NOT NULL`: whitespace is
        // not documentation, and counting it would make the number reward
        // someone typing a space into every field.
        let row = sqlx::query(
            "SELECT count(*) FILTER (WHERE description IS NOT NULL
                                       AND btrim(description) <> '') AS described,
                    count(*) AS total
             FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))",
        )
        .bind(&allow)
        .bind(&deny)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok((row.get("described"), row.get("total")))
    }

    async fn recently_changed_visible(
        &self,
        limit: i64,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))
             ORDER BY updated_at DESC, id DESC
             LIMIT $3"
        ))
        .bind(&allow)
        .bind(&deny)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(asset_from_row).collect())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn count_assets_by_kind_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        // Counted through the same predicate as the rows. A total computed
        // before filtering says "47 results" above 12 rows, which leaks the
        // existence of 35 assets the reader may not see.
        let rows = sqlx::query(
            "SELECT kind, count(*) AS n FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))
             GROUP BY kind",
        )
        .bind(&allow)
        .bind(&deny)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                AssetKind::parse(row.get::<&str, _>("kind"))
                    .ok()
                    .map(|kind| (kind, row.get::<i64, _>("n")))
            })
            .collect())
    }
}

/// The one place `AccessPredicate` becomes SQL.
///
/// Returns `None` for "nothing visible", which callers must answer with an
/// empty result rather than a broader query — the alternative is a predicate
/// that silently matches everything.
fn lower(predicate: &AccessPredicate) -> Option<(Vec<String>, Vec<String>)> {
    match predicate {
        AccessPredicate::Nothing => None,
        // `%` matches every FQN. An empty deny array is correct rather than a
        // sentinel: `x LIKE ANY('{}')` is false, so `NOT (...)` is true and
        // every row passes the deny check. A NUL sentinel would have been both
        // unnecessary and rejected — Postgres text cannot contain NUL.
        AccessPredicate::All => Some((vec!["%".to_string()], Vec::new())),
        AccessPredicate::Fqn {
            allow_prefixes,
            deny_prefixes,
        } => Some((
            allow_prefixes.iter().map(|p| format!("{p}%")).collect(),
            deny_prefixes.iter().map(|p| format!("{p}%")).collect(),
        )),
    }
}

const VISIBILITY: &str =
    "AND (fully_qualified_name LIKE ANY($5)) AND NOT (fully_qualified_name LIKE ANY($6))";
const VISIBILITY_SEARCH: &str =
    "AND (fully_qualified_name LIKE ANY($6)) AND NOT (fully_qualified_name LIKE ANY($7))";

fn asset_from_row(row: PgRow) -> Asset {
    Asset {
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        change_description: row
            .get::<Option<serde_json::Value>, _>("change_description")
            .and_then(|v| serde_json::from_value(v).ok()),
        deleted: row.get("deleted"),
        deleted_at: row.get("deleted_at"),
        id: row.get("id"),
        kind: AssetKind::parse(row.get::<&str, _>("kind")).unwrap_or(AssetKind::Table),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        parent_id: row.get("parent_id"),
        description: row.get("description"),
        properties: row.get("properties"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

const ASSET_COLUMNS: &str = "id, kind, name, fully_qualified_name, parent_id, description, properties, version_major, version_minor, updated_by, change_description, deleted, deleted_at, created_at, updated_at";

impl PostgresStorage {
    async fn asset_page(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let assets: Vec<Asset> = rows.into_iter().map(asset_from_row).collect();
        Ok(Page::from_overfetch(assets, page.limit, |asset| {
            Cursor::new(asset.fully_qualified_name.clone(), asset.id)
        }))
    }
}
