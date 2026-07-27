//! The predicate registry, in Postgres.
//!
//! Core predicates are seeded by migration rather than written at startup: a
//! definition the application inserts is one an application bug can rewrite,
//! and these are the predicates every stored flake already depends on.

use async_trait::async_trait;
use graph_owl_engine::{PredicateDef, PredicateRegistry, RegistryError};
use sqlx::Row;

use crate::PostgresTripleStore;

const UNIQUE_VIOLATION: &str = "23505";

fn definition_from_row(row: &sqlx::postgres::PgRow) -> Result<PredicateDef, RegistryError> {
    let namespace: i32 = row.get("namespace");
    Ok(PredicateDef {
        namespace: u16::try_from(namespace)
            .map_err(|_| RegistryError::Backend(format!("namespace {namespace} is outside u16")))?,
        name: row.get("name"),
        value_type: row.get("value_type"),
        many: row.get("many"),
        core: row.get("core"),
    })
}

#[async_trait]
impl PredicateRegistry for PostgresTripleStore {
    async fn define(&self, definition: &PredicateDef) -> Result<(), RegistryError> {
        // Refusing a core redefinition *before* attempting the insert, so the
        // caller gets the reason rather than a uniqueness violation that says
        // only "it exists" and not "and you may never change it".
        if let Some(existing) = self.lookup(definition.namespace, &definition.name).await? {
            return Err(if existing.core {
                RegistryError::CoreImmutable {
                    namespace: definition.namespace,
                    name: definition.name.clone(),
                }
            } else {
                RegistryError::Duplicate {
                    namespace: definition.namespace,
                    name: definition.name.clone(),
                }
            });
        }

        // `core` is never settable from here. A runtime caller that could mark
        // its own predicate core would make it permanent by accident, and
        // nothing in this API could then remove it.
        let result = sqlx::query(
            "INSERT INTO predicates (namespace, name, value_type, many, core)
             VALUES ($1, $2, $3, $4, FALSE)",
        )
        .bind(i32::from(definition.namespace))
        .bind(&definition.name)
        .bind(definition.value_type)
        .bind(definition.many)
        .execute(self.pool())
        .await;

        match result {
            Ok(_) => Ok(()),
            // Lost a race with a concurrent definer between the lookup and the
            // insert. Still a duplicate, and reporting it as a backend error
            // would send the caller looking for an outage.
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                Err(RegistryError::Duplicate {
                    namespace: definition.namespace,
                    name: definition.name.clone(),
                })
            }
            Err(e) => Err(RegistryError::Backend(e.to_string())),
        }
    }

    async fn lookup(
        &self,
        namespace: u16,
        name: &str,
    ) -> Result<Option<PredicateDef>, RegistryError> {
        let row = sqlx::query(
            "SELECT namespace, name, value_type, many, core
             FROM predicates WHERE namespace = $1 AND name = $2",
        )
        .bind(i32::from(namespace))
        .bind(name)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| RegistryError::Backend(e.to_string()))?;

        row.as_ref().map(definition_from_row).transpose()
    }

    async fn list(&self, namespace: Option<u16>) -> Result<Vec<PredicateDef>, RegistryError> {
        let rows = sqlx::query(
            "SELECT namespace, name, value_type, many, core
             FROM predicates
             WHERE $1::int IS NULL OR namespace = $1
             ORDER BY namespace, name",
        )
        .bind(namespace.map(i32::from))
        .fetch_all(self.pool())
        .await
        .map_err(|e| RegistryError::Backend(e.to_string()))?;

        rows.iter().map(definition_from_row).collect()
    }
}
