//! The Postgres reference connector.
//!
//! Stays in Rust and in the binary (`15-connectors.md` decision 1a): it proves
//! the trait, needs no second runtime, and keeps a Postgres-only deployment at
//! one process.

use crate::{Connector, ConnectorError, RunScope, SourceRecord};
use async_trait::async_trait;
use graph_owl_core::AssetKind;
use serde_json::json;
use sqlx::{PgPool, Row};

pub struct PostgresConnector {
    pool: PgPool,
    /// The service name this instance is catalogued under — the root of the
    /// hierarchy. Supplied rather than derived, because one physical server can
    /// legitimately be two logical services.
    service_name: String,
}

impl PostgresConnector {
    /// # Errors
    ///
    /// Returns [`ConnectorError::Connection`] if the pool cannot be created.
    pub async fn connect(
        connection_string: &str,
        service_name: impl Into<String>,
    ) -> Result<Self, ConnectorError> {
        let pool = PgPool::connect(connection_string)
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))?;
        Ok(Self {
            pool,
            service_name: service_name.into(),
        })
    }
}

/// Schemas that describe the database rather than the user's data. Cataloguing
/// them buries real assets under hundreds of internal ones.
const SYSTEM_SCHEMAS: [&str; 2] = ["pg_catalog", "information_schema"];

#[async_trait]
impl Connector for PostgresConnector {
    fn type_name(&self) -> &'static str {
        "postgres"
    }

    async fn test_connection(&self) -> Result<(), ConnectorError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn fetch(&self, scope: &RunScope) -> Result<Vec<SourceRecord>, ConnectorError> {
        let database: String = sqlx::query("SELECT current_database()")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConnectorError::Introspection(e.to_string()))?
            .get(0);

        let mut records = Vec::new();

        // Emitted in hierarchy order — service, database, schema, table,
        // column — so the sink never has to hold the whole source in memory
        // waiting for a parent to arrive.
        records.push(SourceRecord {
            kind: AssetKind::Service,
            path: vec![self.service_name.clone()],
            description: Some("PostgreSQL service".to_string()),
            properties: Some(json!({ "engine": "postgres" })),
        });
        records.push(SourceRecord {
            kind: AssetKind::Database,
            path: vec![self.service_name.clone(), database.clone()],
            description: None,
            properties: None,
        });

        let schemas: Vec<String> = sqlx::query(
            "SELECT schema_name FROM information_schema.schemata
             WHERE schema_name <> ALL($1) AND schema_name NOT LIKE 'pg_%'
             ORDER BY schema_name",
        )
        .bind(&SYSTEM_SCHEMAS[..])
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConnectorError::Introspection(e.to_string()))?
        .into_iter()
        .map(|row| row.get::<String, _>("schema_name"))
        .filter(|name| scope.include_schemas.is_empty() || scope.include_schemas.contains(name))
        .collect();

        for schema in &schemas {
            records.push(SourceRecord {
                kind: AssetKind::Schema,
                path: vec![self.service_name.clone(), database.clone(), schema.clone()],
                description: None,
                properties: None,
            });

            let tables = sqlx::query(
                "SELECT table_name, table_type FROM information_schema.tables
                 WHERE table_schema = $1
                 ORDER BY table_name",
            )
            .bind(schema)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ConnectorError::Introspection(e.to_string()))?;

            for table in tables {
                let table_name: String = table.get("table_name");
                let table_type: String = table.get("table_type");
                records.push(SourceRecord {
                    kind: AssetKind::Table,
                    path: vec![
                        self.service_name.clone(),
                        database.clone(),
                        schema.clone(),
                        table_name.clone(),
                    ],
                    description: None,
                    // A view is marked rather than filtered out: it is a real
                    // asset with real lineage, and hiding it makes the graph
                    // wrong rather than smaller.
                    properties: Some(json!({ "tableType": table_type })),
                });

                let columns = sqlx::query(
                    "SELECT column_name, data_type, is_nullable, ordinal_position
                     FROM information_schema.columns
                     WHERE table_schema = $1 AND table_name = $2
                     ORDER BY ordinal_position",
                )
                .bind(schema)
                .bind(&table_name)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| ConnectorError::Introspection(e.to_string()))?;

                for column in columns {
                    let column_name: String = column.get("column_name");
                    records.push(SourceRecord {
                        kind: AssetKind::Column,
                        path: vec![
                            self.service_name.clone(),
                            database.clone(),
                            schema.clone(),
                            table_name.clone(),
                            column_name,
                        ],
                        description: None,
                        properties: Some(json!({
                            "dataType": column.get::<String, _>("data_type"),
                            "nullable": column.get::<String, _>("is_nullable") == "YES",
                            "ordinalPosition": column.get::<i32, _>("ordinal_position"),
                        })),
                    });
                }
            }
        }

        Ok(records)
    }
}
