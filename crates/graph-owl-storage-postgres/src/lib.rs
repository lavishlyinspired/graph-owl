use async_trait::async_trait;
use graph_owl_core::Table;
use graph_owl_storage::{Storage, StorageError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

mod embedded {
    refinery::embed_migrations!("migrations");
}

const UNIQUE_VIOLATION: &str = "23505";

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

        Ok(row.map(|row| Table {
            id: row.get("id"),
            name: row.get("name"),
            fully_qualified_name: row.get("fully_qualified_name"),
            description: row.get("description"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }
}
