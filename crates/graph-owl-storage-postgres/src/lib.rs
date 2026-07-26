use async_trait::async_trait;
use graph_owl_core::{Relationship, Table, TableUpdate};
use graph_owl_storage::{Storage, StorageError};
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
    async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
        sqlx::query(
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
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                StorageError::Conflict(table.fully_qualified_name.clone())
            }
            _ => StorageError::Unexpected(e.to_string()),
        })?;

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

    async fn list_tables(&self) -> Result<Vec<Table>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, fully_qualified_name, description, created_at, updated_at
             FROM tables",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.into_iter().map(table_from_row).collect())
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
                StorageError::Conflict(format!(
                    "{}:{} -{}-> {}:{}",
                    relationship.from_entity_type,
                    relationship.from_entity_id,
                    relationship.relationship_type,
                    relationship.to_entity_type,
                    relationship.to_entity_id
                ))
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

    async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM entity_relationships WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}
