//! The predicate and namespace registries, in Postgres.
//!
//! Core predicates are seeded by migration rather than written at startup: a
//! definition the application inserts is one an application bug can rewrite,
//! and these are the predicates every stored flake already depends on.
//!
//! The namespace registry below is the sibling that lets a domain bring its
//! own *vocabulary*, not just new predicates inside a vocabulary the binary
//! already knew — see `plans/105-domain-neutrality.md`.

use async_trait::async_trait;
use graph_owl_core::flake::namespace;
use graph_owl_engine::{
    EvidenceBinding, FindingRuleDef, FindingRuleRegistry, NamespaceDef, NamespaceRegistry,
    PackQueryDef, PackQueryRegistry, PredicateDef, PredicateRegistry, RegistryError,
};
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

fn namespace_from_row(row: &sqlx::postgres::PgRow) -> Result<NamespaceDef, RegistryError> {
    let code: i32 = row.get("code");
    Ok(NamespaceDef {
        code: u16::try_from(code)
            .map_err(|_| RegistryError::Backend(format!("namespace code {code} is outside u16")))?,
        iri: row.get("iri"),
        declared_by: row.get("declared_by"),
    })
}

#[async_trait]
impl NamespaceRegistry for PostgresTripleStore {
    async fn declare(&self, definition: &NamespaceDef) -> Result<(), RegistryError> {
        // Refused here rather than by the table's own CHECK, so the caller
        // gets the reason ("that range belongs to the binary") instead of a
        // constraint name. Reported as `CoreImmutable` because it is the same
        // failure a core predicate redefinition is: flakes already written
        // against `dsc:` or `rdf:` would not migrate, they would change
        // meaning in place.
        if definition.code < namespace::RUNTIME_START {
            return Err(RegistryError::CoreImmutable {
                namespace: definition.code,
                name: definition.iri.clone(),
            });
        }

        // Idempotent on an identical pair, because reloading a pack must be
        // safe to repeat — a redeclaration that failed would make a restart a
        // failure. Anything *else* sharing either half is a real conflict.
        let existing = sqlx::query("SELECT code, iri, declared_by FROM namespaces WHERE code = $1")
            .bind(i32::from(definition.code))
            .fetch_optional(self.pool())
            .await
            .map_err(|e| RegistryError::Backend(e.to_string()))?;

        if let Some(row) = existing.as_ref() {
            let found = namespace_from_row(row)?;
            return if found.iri == definition.iri {
                Ok(())
            } else {
                Err(RegistryError::Duplicate {
                    namespace: definition.code,
                    name: found.iri,
                })
            };
        }

        let result =
            sqlx::query("INSERT INTO namespaces (code, iri, declared_by) VALUES ($1, $2, $3)")
                .bind(i32::from(definition.code))
                .bind(&definition.iri)
                .bind(&definition.declared_by)
                .execute(self.pool())
                .await;

        match result {
            Ok(_) => Ok(()),
            // Either a concurrent declarer took the code, or this IRI already
            // has a different code — both are `Duplicate`, and both matter for
            // the same reason: two codes for one IRI make resolution depend on
            // load order.
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                Err(RegistryError::Duplicate {
                    namespace: definition.code,
                    name: definition.iri.clone(),
                })
            }
            Err(e) => Err(RegistryError::Backend(e.to_string())),
        }
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceDef>, RegistryError> {
        let rows = sqlx::query("SELECT code, iri, declared_by FROM namespaces ORDER BY code")
            .fetch_all(self.pool())
            .await
            .map_err(|e| RegistryError::Backend(e.to_string()))?;

        rows.iter().map(namespace_from_row).collect()
    }

    async fn next_code(&self) -> Result<u16, RegistryError> {
        // `MAX + 1`, never "lowest gap". A code whose namespace was abandoned
        // is still carried by every flake written while it was live, so
        // reissuing it would silently repoint that history at a different
        // vocabulary. Allocation is monotonic, exactly as `Sid`'s own doc
        // comment already requires.
        let row = sqlx::query("SELECT COALESCE(MAX(code), $1 - 1) + 1 AS next FROM namespaces")
            .bind(i32::from(namespace::RUNTIME_START))
            .fetch_one(self.pool())
            .await
            .map_err(|e| RegistryError::Backend(e.to_string()))?;

        let next: i32 = row.get("next");
        u16::try_from(next)
            .ok()
            .filter(|&c| c < namespace::NOT_FOUND)
            .ok_or_else(|| {
                RegistryError::Backend(format!(
                    "the runtime namespace range is exhausted at {next}; \
                     codes must stay below {}",
                    namespace::NOT_FOUND
                ))
            })
    }
}

fn finding_rule_from_row(row: &sqlx::postgres::PgRow) -> Result<FindingRuleDef, RegistryError> {
    let evidence: serde_json::Value = row.get("evidence");
    let evidence: Vec<EvidenceBinding> = serde_json::from_value(evidence).map_err(|e| {
        RegistryError::Backend(format!("stored evidence is not the expected shape: {e}"))
    })?;
    Ok(FindingRuleDef {
        pack: row.get("pack"),
        label: row.get("label"),
        summary: row.get("summary"),
        governed_by: row.get("governed_by"),
        query: row.get("query"),
        subject_var: row.get("subject_var"),
        evidence,
        similarity: row.get("similarity"),
        span: row.get("span"),
    })
}

#[async_trait]
impl FindingRuleRegistry for PostgresTripleStore {
    async fn declare(&self, rule: &FindingRuleDef) -> Result<(), RegistryError> {
        let evidence = serde_json::to_value(&rule.evidence).map_err(|e| {
            RegistryError::Backend(format!("evidence could not be serialized: {e}"))
        })?;

        sqlx::query(
            "INSERT INTO finding_rules
                (pack, label, summary, governed_by, query, subject_var, evidence, similarity, span)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (pack, label) DO UPDATE SET
                summary     = EXCLUDED.summary,
                governed_by = EXCLUDED.governed_by,
                query       = EXCLUDED.query,
                subject_var = EXCLUDED.subject_var,
                evidence    = EXCLUDED.evidence,
                similarity  = EXCLUDED.similarity,
                span        = EXCLUDED.span",
        )
        .bind(&rule.pack)
        .bind(&rule.label)
        .bind(&rule.summary)
        .bind(&rule.governed_by)
        .bind(&rule.query)
        .bind(&rule.subject_var)
        .bind(evidence)
        .bind(&rule.similarity)
        .bind(&rule.span)
        .execute(self.pool())
        .await
        .map_err(|e| RegistryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn for_pack(&self, pack: &str) -> Result<Vec<FindingRuleDef>, RegistryError> {
        let rows = sqlx::query(
            "SELECT pack, label, summary, governed_by, query, subject_var, evidence, similarity, span
             FROM finding_rules WHERE pack = $1 ORDER BY label",
        )
        .bind(pack)
        .fetch_all(self.pool())
        .await
        .map_err(|e| RegistryError::Backend(e.to_string()))?;

        rows.iter().map(finding_rule_from_row).collect()
    }
}

#[async_trait]
impl PackQueryRegistry for PostgresTripleStore {
    async fn declare_query(&self, def: &PackQueryDef) -> Result<(), RegistryError> {
        sqlx::query(
            "INSERT INTO pack_queries (pack, name, query)
             VALUES ($1, $2, $3)
             ON CONFLICT (pack, name) DO UPDATE SET query = EXCLUDED.query",
        )
        .bind(&def.pack)
        .bind(&def.name)
        .bind(&def.query)
        .execute(self.pool())
        .await
        .map_err(|e| RegistryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn pack_query(&self, pack: &str, name: &str) -> Result<Option<String>, RegistryError> {
        let row = sqlx::query("SELECT query FROM pack_queries WHERE pack = $1 AND name = $2")
            .bind(pack)
            .bind(name)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| RegistryError::Backend(e.to_string()))?;

        Ok(row.map(|r| r.get("query")))
    }
}
