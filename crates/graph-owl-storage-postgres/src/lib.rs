use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::contradiction::{Review, Verdict};
use graph_owl_core::memory::{Authorship, LinkRelation, Memory, MemoryKind, MemoryLink};
use graph_owl_core::ownership::{EntityReference, OwnerKind, OwnerRef};
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    envelope::{ChangeDescription, EntityVersion, classify},
    page::{Cursor, Page, PageRequest},
};
use graph_owl_storage::{
    ConflictKind, MemoryWrite, OwnersWrite, Storage, StorageError, StoredUser, SupersedeOutcome,
    UpdateOutcome,
};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

/// Every column a `Memory` is rebuilt from, by id. Named once so the shape
/// cannot drift between the single read and the by-subject read.
const MEMORY_COLUMNS: &str = "SELECT id, kind, content, summary, author_kind, author_user_id,
            author_agent_id, author_model, confidence, as_of, supersedes, superseded_by
     FROM memories WHERE id = $1";

/// The wire spelling of a kind.
///
/// A `match` rather than a `Serialize` round-trip through JSON: the column has a
/// `CHECK` listing these exact strings, so a rename that forgets the migration
/// has to fail to compile rather than fail at 3am on the first write.
const fn memory_kind_str(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Rationale => "rationale",
        MemoryKind::Incident => "incident",
        MemoryKind::Decision => "decision",
        MemoryKind::Caveat => "caveat",
    }
}

const fn verdict_str(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Confirmed => "confirmed",
        Verdict::Dismissed => "dismissed",
    }
}

/// Rebuild a verdict from the column.
///
/// An unrecognised value is an error rather than a default. Defaulting to
/// `Dismissed` would silently hide a pair a reviewer confirmed; defaulting to
/// `Confirmed` would put their name against a judgement they did not make. The
/// `CHECK` means this can only happen across versions, which is exactly when a
/// loud failure is wanted.
fn verdict_from(value: &str) -> Result<Verdict, StorageError> {
    match value {
        "confirmed" => Ok(Verdict::Confirmed),
        "dismissed" => Ok(Verdict::Dismissed),
        other => Err(StorageError::Unexpected(format!(
            "unknown contradiction verdict in storage: {other}"
        ))),
    }
}

const fn relation_str(relation: LinkRelation) -> &'static str {
    match relation {
        LinkRelation::About => "about",
        LinkRelation::Affects => "affects",
        LinkRelation::Evidence => "evidence",
        LinkRelation::Follows => "follows",
        LinkRelation::Contradicts => "contradicts",
        LinkRelation::Mentions => "mentions",
    }
}

/// Rebuild a kind from the column.
///
/// An unrecognised value is an error rather than a default. A row written by a
/// newer version reading back as `Rationale` would silently reclassify somebody's
/// decision, and the `CHECK` means this can only happen across versions — which
/// is exactly when a loud failure is wanted.
fn memory_kind_from(value: &str) -> Result<MemoryKind, StorageError> {
    match value {
        "rationale" => Ok(MemoryKind::Rationale),
        "incident" => Ok(MemoryKind::Incident),
        "decision" => Ok(MemoryKind::Decision),
        "caveat" => Ok(MemoryKind::Caveat),
        other => Err(StorageError::Unexpected(format!(
            "unknown memory kind in storage: {other}"
        ))),
    }
}

fn relation_from(value: &str) -> Result<LinkRelation, StorageError> {
    match value {
        "about" => Ok(LinkRelation::About),
        "affects" => Ok(LinkRelation::Affects),
        "evidence" => Ok(LinkRelation::Evidence),
        "follows" => Ok(LinkRelation::Follows),
        "contradicts" => Ok(LinkRelation::Contradicts),
        "mentions" => Ok(LinkRelation::Mentions),
        other => Err(StorageError::Unexpected(format!(
            "unknown memory link relation in storage: {other}"
        ))),
    }
}

/// Write a memory's links, resolving each target to an asset or a memory.
///
/// Returns `Some((index, target))` for the **first** unresolvable link rather
/// than collecting them all: the client has to fix that one regardless, and
/// reporting four failures when the first is a typo in a copied id is noise.
///
/// Deliberately *not* a `MemoryWrite`: two callers need the same fact in two
/// different outcome shapes, and returning one of them from a shared helper
/// forced the other to unwrap a variant it could never see.
async fn insert_links(
    tx: &mut Transaction<'_, Postgres>,
    memory_id: Uuid,
    links: &[MemoryLink],
) -> Result<Option<(usize, Uuid)>, StorageError> {
    for (index, edge) in links.iter().enumerate() {
        // Which column the target belongs in is a question only the database can
        // answer, and asking it is not wasted work — Slice A requires an
        // unresolvable target to be reported as a client error naming the index,
        // so the lookup is the validation.
        let is_asset: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM assets WHERE id = $1)")
                .bind(edge.target)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let is_memory: bool = if is_asset {
            false
        } else {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memories WHERE id = $1)")
                .bind(edge.target)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?
        };

        if !is_asset && !is_memory {
            return Ok(Some((index, edge.target)));
        }

        sqlx::query(
            "INSERT INTO memory_links (memory_id, relation, asset_target, memory_target)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(memory_id)
        .bind(relation_str(edge.relation))
        .bind(if is_asset { Some(edge.target) } else { None })
        .bind(if is_asset { None } else { Some(edge.target) })
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    }
    Ok(None)
}

/// A memory's links, from whichever target column holds each one.
async fn read_links(pool: &PgPool, memory_id: Uuid) -> Result<Vec<MemoryLink>, StorageError> {
    let rows: Vec<(String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT relation, asset_target, memory_target FROM memory_links
         WHERE memory_id = $1 ORDER BY relation, asset_target, memory_target",
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::Unexpected(e.to_string()))?;

    rows.into_iter()
        .map(|(relation, asset_target, memory_target)| {
            // The `CHECK` guarantees exactly one is set, so a row with neither is
            // a corrupt row and not a shape to paper over with a default.
            let target = asset_target.or(memory_target).ok_or_else(|| {
                StorageError::Unexpected(format!("memory link {memory_id} has no target"))
            })?;
            Ok(MemoryLink {
                relation: relation_from(&relation)?,
                target,
            })
        })
        .collect()
}

/// Rebuild a `Memory` from a row plus its links.
///
/// **Constructed field by field rather than through `Memory::new`.** The
/// constructor refuses a memory with no anchor, which is right on the way in and
/// wrong on the way out: a row that somehow lost its anchor must be *readable*
/// so somebody can see and fix it, not unreadable so it becomes invisible — and
/// hiding a row is the failure mode this whole epic is against.
fn memory_from_row(row: &PgRow, links: Vec<MemoryLink>) -> Result<Memory, StorageError> {
    let author_kind: String = row.get("author_kind");
    let authorship = match author_kind.as_str() {
        "human" => Authorship::Human {
            // `ON DELETE SET NULL` on the FK: losing the attribution is better
            // than losing the memory, so a deleted person reads back as an
            // unnamed human rather than as an error or as an agent.
            user_id: row
                .get::<Option<String>, _>("author_user_id")
                .unwrap_or_default(),
        },
        "agent" => Authorship::Agent {
            agent_id: row
                .get::<Option<String>, _>("author_agent_id")
                .unwrap_or_default(),
            model: row
                .get::<Option<String>, _>("author_model")
                .unwrap_or_default(),
        },
        other => {
            return Err(StorageError::Unexpected(format!(
                "unknown authorship kind in storage: {other}"
            )));
        }
    };

    Ok(Memory {
        id: row.get("id"),
        kind: memory_kind_from(&row.get::<String, _>("kind"))?,
        content: row.get("content"),
        summary: row.get("summary"),
        authorship,
        confidence: row.get("confidence"),
        links,
        as_of: row.get("as_of"),
        supersedes: row.get("supersedes"),
        superseded_by: row.get("superseded_by"),
    })
}

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
    #[tracing::instrument(name = "storage.create_lineage_edge", skip_all)]
    async fn create_lineage_edge(
        &self,
        edge: &graph_owl_core::lineage::LineageEdge,
    ) -> Result<(), StorageError> {
        let result = sqlx::query(
            "INSERT INTO lineage_edges
                 (id, from_asset_id, to_asset_id, relationship, source, query, description, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(edge.id)
        .bind(edge.from_asset_id)
        .bind(edge.to_asset_id)
        .bind(edge.relationship.as_str())
        .bind(edge.details.source.as_str())
        .bind(&edge.details.query)
        .bind(&edge.details.description)
        .bind(&edge.created_by)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                Err(StorageError::Conflict {
                    detail: format!(
                        "{} already {} {} according to {}",
                        edge.from_asset_id,
                        edge.relationship.as_str(),
                        edge.to_asset_id,
                        edge.details.source.as_str()
                    ),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                })
            }
            Err(e) => Err(StorageError::Unexpected(e.to_string())),
        }
    }

    #[tracing::instrument(name = "storage.delete_lineage_edge", skip_all)]
    async fn delete_lineage_edge(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::lineage::LineageEdge>, StorageError> {
        // `RETURNING`, so the caller can withdraw the matching triple from the
        // graph. A read followed by a delete races with a concurrent delete and
        // projects a retraction for an edge somebody else already removed.
        let row = sqlx::query(
            "DELETE FROM lineage_edges WHERE id = $1
             RETURNING id, from_asset_id, to_asset_id, relationship, source,
                       query, description, created_at, created_by",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        row.map(|row| {
            let relationship: String = row.get("relationship");
            let source: String = row.get("source");
            Ok(graph_owl_core::lineage::LineageEdge {
                id: row.get("id"),
                from_asset_id: row.get("from_asset_id"),
                to_asset_id: row.get("to_asset_id"),
                relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                    &relationship,
                )
                .map_err(|e| StorageError::Unexpected(format!("unknown relationship {e:?}")))?,
                details: graph_owl_core::lineage::LineageDetails {
                    source: graph_owl_core::lineage::LineageSource::parse(&source).map_err(
                        |e| StorageError::Unexpected(format!("unknown lineage source {e}")),
                    )?,
                    query: row.get("query"),
                    description: row.get("description"),
                },
                created_at: row.get("created_at"),
                created_by: row.get("created_by"),
            })
        })
        .transpose()
    }

    #[tracing::instrument(name = "storage.lineage_edges_touching", skip_all)]
    async fn lineage_edges_touching(
        &self,
        asset_ids: &[Uuid],
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, from_asset_id, to_asset_id, relationship, source, query,
                    description, created_at, created_by
               FROM lineage_edges
              WHERE from_asset_id = ANY($1) OR to_asset_id = ANY($1)",
        )
        .bind(asset_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        rows.into_iter()
            .map(|row| {
                let relationship: String = row.get("relationship");
                let source: String = row.get("source");
                Ok(graph_owl_core::lineage::LineageEdge {
                    id: row.get("id"),
                    from_asset_id: row.get("from_asset_id"),
                    to_asset_id: row.get("to_asset_id"),
                    // A row whose vocabulary this build does not know is a
                    // storage error, not a silent skip: dropping it would make
                    // a lineage graph quietly incomplete, which is the one
                    // thing a lineage graph must never be.
                    relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                        &relationship,
                    )
                    .map_err(|e| StorageError::Unexpected(format!("unknown relationship {e:?}")))?,
                    details: graph_owl_core::lineage::LineageDetails {
                        source: graph_owl_core::lineage::LineageSource::parse(&source).map_err(
                            |e| StorageError::Unexpected(format!("unknown lineage source {e}")),
                        )?,
                        query: row.get("query"),
                        description: row.get("description"),
                    },
                    created_at: row.get("created_at"),
                    created_by: row.get("created_by"),
                })
            })
            .collect()
    }

    #[tracing::instrument(name = "storage.begin_run", skip_all)]
    async fn begin_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO connector_runs
                 (id, connector, service_name, started_at, triggered_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(run.id)
        .bind(&run.connector)
        .bind(&run.service_name)
        .bind(run.started_at)
        .bind(&run.triggered_by)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.finish_run", skip_all)]
    async fn finish_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE connector_runs
                SET finished_at = $2, created = $3, skipped = $4, failed = $5,
                    deleted = $6, failures = $7, refusal = $8
              WHERE id = $1",
        )
        .bind(run.id)
        .bind(run.finished_at)
        .bind(run.created)
        .bind(run.skipped)
        .bind(run.failed)
        .bind(run.deleted)
        .bind(&run.failures)
        .bind(&run.refusal)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.replace_validation_results", skip_all)]
    async fn replace_validation_results(
        &self,
        computed_at_t: i64,
        results: &[graph_owl_storage::ValidationFinding],
    ) -> Result<(), StorageError> {
        // One transaction, so a failed write leaves the previous results in
        // place. The alternative — delete, then fail to insert — empties the
        // queue and reads to a steward as "everything is fixed".
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("DELETE FROM validation_results")
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for finding in results {
            sqlx::query(
                "INSERT INTO validation_results
                     (id, computed_at_t, shape, focus_node, path,
                      constraint_kind, severity, message, actual, suggestion)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(finding.id)
            .bind(computed_at_t)
            .bind(&finding.shape)
            .bind(&finding.focus_node)
            .bind(&finding.path)
            .bind(&finding.constraint_kind)
            .bind(&finding.severity)
            .bind(&finding.message)
            .bind(&finding.actual)
            .bind(&finding.suggestion)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        // A pass that found nothing still records *when* it ran. Without this
        // row an empty queue is ambiguous between "clean" and "never
        // validated", and those call for opposite reactions.
        if results.is_empty() {
            sqlx::query(
                "INSERT INTO validation_results
                     (id, computed_at_t, shape, focus_node, constraint_kind,
                      severity, message)
                 VALUES ($1, $2, '', '', '', 'marker', '')",
            )
            .bind(Uuid::new_v4())
            .bind(computed_at_t)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.validation_results", skip_all)]
    async fn validation_results(
        &self,
        filter: &graph_owl_storage::ValidationFilter,
    ) -> Result<(Vec<graph_owl_storage::ValidationFinding>, i64, usize), StorageError> {
        // The marker row is bookkeeping, never a finding — it exists so a clean
        // pass is distinguishable from no pass, and it must not appear in a
        // queue as a violation of nothing.
        let where_clause = "severity <> 'marker'
              AND ($1::TEXT IS NULL OR severity = $1)
              AND ($2::TEXT IS NULL OR shape = $2)
              AND ($3::TEXT IS NULL OR focus_node = $3)";

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM validation_results WHERE {where_clause}"
        ))
        .bind(&filter.severity)
        .bind(&filter.shape)
        .bind(&filter.focus_node)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `computed_at_t` comes from any row including the marker, so a clean
        // pass still reports its currency.
        let computed_at_t: Option<i64> =
            sqlx::query_scalar("SELECT MAX(computed_at_t) FROM validation_results")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let rows = sqlx::query(&format!(
            "SELECT id, shape, focus_node, path, constraint_kind, severity,
                    message, actual, suggestion
               FROM validation_results
              WHERE {where_clause}
              -- Worst first, then stable: a queue that reorders between polls
              -- cannot be worked from the top.
              ORDER BY CASE severity
                         WHEN 'violation' THEN 0
                         WHEN 'warning' THEN 1
                         ELSE 2
                       END,
                       focus_node, shape, constraint_kind
              LIMIT $4 OFFSET $5"
        ))
        .bind(&filter.severity)
        .bind(&filter.shape)
        .bind(&filter.focus_node)
        .bind(i64::try_from(filter.limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(filter.offset).unwrap_or(0))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let findings = rows
            .into_iter()
            .map(|row| graph_owl_storage::ValidationFinding {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                severity: row.get("severity"),
                message: row.get("message"),
                actual: row.get("actual"),
                suggestion: row.get("suggestion"),
            })
            .collect();

        Ok((
            findings,
            computed_at_t.unwrap_or(0),
            usize::try_from(total).unwrap_or(0),
        ))
    }

    #[tracing::instrument(name = "storage.waive_finding", skip_all)]
    async fn waive_finding(&self, waiver: &graph_owl_storage::Waiver) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO validation_waivers
                 (id, shape, focus_node, path, constraint_kind,
                  reason, waived_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(waiver.id)
        .bind(&waiver.shape)
        .bind(&waiver.focus_node)
        .bind(&waiver.path)
        .bind(&waiver.constraint_kind)
        .bind(&waiver.reason)
        .bind(&waiver.waived_by)
        .bind(waiver.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            // The unique index is what makes a second waiver impossible;
            // translating it here means the API can say so rather than
            // returning an opaque 500 for a condition a caller can fix.
            if e.as_database_error()
                .is_some_and(|db| db.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: "this finding is already waived".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::WaiverExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })
    }

    #[tracing::instrument(name = "storage.revoke_waiver", skip_all)]
    async fn revoke_waiver(&self, id: Uuid) -> Result<bool, StorageError> {
        sqlx::query("DELETE FROM validation_waivers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.waivers", skip_all)]
    async fn waivers(&self) -> Result<Vec<graph_owl_storage::Waiver>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, shape, focus_node, path, constraint_kind,
                    reason, waived_by, waived_at, expires_at
               FROM validation_waivers
              ORDER BY expires_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::Waiver {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                reason: row.get("reason"),
                waived_by: row.get("waived_by"),
                waived_at: row.get("waived_at"),
                expires_at: row.get("expires_at"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.upsert_connector_config", skip_all)]
    async fn upsert_connector_config(
        &self,
        config: &graph_owl_storage::ConnectorConfig,
        secret: Option<&str>,
    ) -> Result<(), StorageError> {
        // `COALESCE($5, connector_configs.secret)` is what makes `None` mean
        // "leave it alone". An edit-then-save round trip cannot resend a
        // credential it was never given, and treating absent as "clear it"
        // would break a connector every time somebody renamed its service.
        sqlx::query(
            "INSERT INTO connector_configs (id, connector, service_name, settings, secret)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (connector, service_name) DO UPDATE
                SET settings   = EXCLUDED.settings,
                    secret     = COALESCE(EXCLUDED.secret, connector_configs.secret),
                    updated_at = now()",
        )
        .bind(config.id)
        .bind(&config.connector)
        .bind(&config.service_name)
        .bind(&config.settings)
        .bind(secret)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.connector_configs", skip_all)]
    async fn connector_configs(
        &self,
    ) -> Result<Vec<graph_owl_storage::ConnectorConfig>, StorageError> {
        // **`secret` is not in the SELECT.** The struct has no field for it, so
        // this could not compile if it were — but naming the columns rather than
        // `SELECT *` means a reviewer can see the omission is deliberate.
        let rows = sqlx::query(
            "SELECT id, connector, service_name, settings,
                    (secret IS NOT NULL) AS has_secret
               FROM connector_configs
              ORDER BY connector, service_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::ConnectorConfig {
                id: row.get("id"),
                connector: row.get("connector"),
                service_name: row.get("service_name"),
                settings: row.get("settings"),
                has_secret: row.get("has_secret"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.connector_secret", skip_all)]
    async fn connector_secret(&self, id: Uuid) -> Result<Option<String>, StorageError> {
        // The only place a credential is read. Deliberately its own method so a
        // reviewer auditing where secrets go has one signature to grep for.
        sqlx::query_scalar("SELECT secret FROM connector_configs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(Option::flatten)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.upsert_team", skip_all)]
    // ---- Epic 31: organizational memory ----

    async fn save_memory(&self, memory: &Memory) -> Result<MemoryWrite, StorageError> {
        // One transaction: a memory whose row was written and whose links were
        // not is an **unanchored** memory — stored, permanently unretrievable,
        // and holding the id somebody was told the write succeeded under. The
        // domain refuses to construct one; the adapter must not create one by
        // failing halfway.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let (author_kind, author_user_id, author_agent_id, author_model) = match &memory.authorship
        {
            Authorship::Human { user_id } => ("human", Some(user_id.clone()), None, None),
            Authorship::Agent { agent_id, model } => {
                ("agent", None, Some(agent_id.clone()), Some(model.clone()))
            }
        };

        sqlx::query(
            "INSERT INTO memories
                (id, kind, content, summary, author_kind, author_user_id,
                 author_agent_id, author_model, confidence, as_of)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(memory.id)
        .bind(memory_kind_str(memory.kind))
        .bind(&memory.content)
        .bind(&memory.summary)
        .bind(author_kind)
        .bind(&author_user_id)
        .bind(&author_agent_id)
        .bind(&author_model)
        .bind(memory.confidence)
        .bind(memory.as_of)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!("memory {} already exists", memory.id),
                    existing_id: Some(memory.id),
                    kind: ConflictKind::MemoryExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        if let Some((index, target)) = insert_links(&mut tx, memory.id, &memory.links).await? {
            // Rolled back explicitly rather than by dropping `tx`: the caller is
            // getting `Ok(UnknownLinkTarget)`, and an implicit rollback on a
            // success-shaped return is the kind of thing a later reader assumes
            // did not happen.
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(MemoryWrite::UnknownLinkTarget { index, target });
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(MemoryWrite::Saved)
    }

    async fn find_memory(&self, id: Uuid) -> Result<Option<Memory>, StorageError> {
        let Some(row) = sqlx::query(MEMORY_COLUMNS)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
        else {
            return Ok(None);
        };
        let links = read_links(&self.pool, id).await?;
        Ok(Some(memory_from_row(&row, links)?))
    }

    async fn memories_about(
        &self,
        subject: Uuid,
        include_superseded: bool,
    ) -> Result<Vec<Memory>, StorageError> {
        // The subject may be an asset or another memory, and the caller does not
        // know which — the domain's `MemoryLink` carries one id precisely because
        // it does not care. Matching either column keeps that true above the
        // adapter rather than pushing the split upward.
        let rows = sqlx::query(
            "SELECT m.id, m.kind, m.content, m.summary, m.author_kind, m.author_user_id,
                    m.author_agent_id, m.author_model, m.confidence, m.as_of,
                    m.supersedes, m.superseded_by
             FROM memories m
             JOIN memory_links l ON l.memory_id = m.id
             WHERE (l.asset_target = $1 OR l.memory_target = $1)
               AND ($2 OR m.superseded_by IS NULL)
             GROUP BY m.id
             ORDER BY m.as_of DESC, m.id",
        )
        .bind(subject)
        .bind(include_superseded)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut memories = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let links = read_links(&self.pool, id).await?;
            memories.push(memory_from_row(row, links)?);
        }
        Ok(memories)
    }

    async fn supersede_memory(
        &self,
        original: Uuid,
        replacement: &Memory,
    ) -> Result<SupersedeOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `FOR UPDATE`, so two concurrent corrections cannot both read "not yet
        // superseded" and both write themselves in. The loser gets
        // `AlreadySuperseded` naming the winner, which is exactly what it needs
        // to retry correctly.
        let existing: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT superseded_by FROM memories WHERE id = $1 FOR UPDATE")
                .bind(original)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some((superseded_by,)) = existing else {
            return Ok(SupersedeOutcome::NotFound);
        };
        if let Some(current) = superseded_by {
            return Ok(SupersedeOutcome::AlreadySuperseded { current });
        }

        let (author_kind, author_user_id, author_agent_id, author_model) =
            match &replacement.authorship {
                Authorship::Human { user_id } => ("human", Some(user_id.clone()), None, None),
                Authorship::Agent { agent_id, model } => {
                    ("agent", None, Some(agent_id.clone()), Some(model.clone()))
                }
            };

        sqlx::query(
            "INSERT INTO memories
                (id, kind, content, summary, author_kind, author_user_id,
                 author_agent_id, author_model, confidence, as_of, supersedes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(replacement.id)
        .bind(memory_kind_str(replacement.kind))
        .bind(&replacement.content)
        .bind(&replacement.summary)
        .bind(author_kind)
        .bind(&author_user_id)
        .bind(&author_agent_id)
        .bind(&author_model)
        .bind(replacement.confidence)
        .bind(replacement.as_of)
        .bind(original)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if let Some((index, target)) =
            insert_links(&mut tx, replacement.id, &replacement.links).await?
        {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            // The same client-fixable condition the create path reports, reported
            // the same way. It previously became an `Unexpected` — a `500` for a
            // body that would have earned a `400` from `POST /memories`.
            return Ok(SupersedeOutcome::UnknownLinkTarget { index, target });
        }

        // The other half. Both or neither — a dangling pair reads as history and
        // is not.
        sqlx::query("UPDATE memories SET superseded_by = $2, updated_at = now() WHERE id = $1")
            .bind(original)
            .bind(replacement.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(SupersedeOutcome::Superseded)
    }

    async fn review_contradiction(
        &self,
        review: Review,
        reviewed_by: &str,
        note: Option<&str>,
    ) -> Result<(), StorageError> {
        // Normalised before it is stored. The schema also enforces `a < b`, so
        // this is belt and braces — but the braces matter: the CHECK would turn a
        // reviewer's click into a 500 rather than quietly ordering it.
        let (a, b) = if review.a < review.b {
            (review.a, review.b)
        } else {
            (review.b, review.a)
        };

        sqlx::query(
            "INSERT INTO memory_contradiction_reviews (a, b, verdict, reviewed_by, note)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (a, b) DO UPDATE
                    SET verdict     = EXCLUDED.verdict,
                        reviewed_by = EXCLUDED.reviewed_by,
                        reviewed_at = now(),
                        note        = EXCLUDED.note",
        )
        .bind(a)
        .bind(b)
        .bind(verdict_str(review.verdict))
        .bind(reviewed_by)
        .bind(note)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn contradiction_reviews(&self) -> Result<Vec<Review>, StorageError> {
        let rows: Vec<(Uuid, Uuid, String)> =
            sqlx::query_as("SELECT a, b, verdict FROM memory_contradiction_reviews")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter()
            .map(|(a, b, verdict)| {
                Ok(Review {
                    a,
                    b,
                    verdict: verdict_from(&verdict)?,
                })
            })
            .collect()
    }

    // ---- Epic 11 Slice C: ownership ----

    async fn set_asset_owners(
        &self,
        asset_id: Uuid,
        owners: &[OwnerRef],
    ) -> Result<OwnersWrite, StorageError> {
        // One transaction: an asset whose old owners were deleted and whose new
        // ones failed to write is an asset that silently became unowned, and
        // "unowned" is a state the gap report acts on.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM assets WHERE id = $1)")
            .bind(asset_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if !exists {
            return Ok(OwnersWrite::NotFound);
        }

        // **Every principal is resolved before anything is written**, so a bad
        // owner at index 2 does not leave indexes 0 and 1 applied. Resolution also
        // produces the display name the read path needs, so the lookup is not
        // wasted work.
        let mut resolved = Vec::with_capacity(owners.len());
        for (index, owner) in owners.iter().enumerate() {
            let table = match owner.kind {
                OwnerKind::User => "SELECT display_name FROM users WHERE id = $1",
                OwnerKind::Team => "SELECT display_name FROM teams WHERE id = $1",
            };
            let display_name: Option<String> = sqlx::query_scalar(table)
                .bind(&owner.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            let Some(display_name) = display_name else {
                tx.rollback()
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
                return Ok(OwnersWrite::UnknownPrincipal {
                    index,
                    id: owner.id.clone(),
                });
            };
            resolved.push(EntityReference {
                id: owner.id.clone(),
                kind: owner.kind,
                display_name,
                // A write records ownership *here*, so what comes back is direct
                // by construction. Inheritance is a read-time projection only.
                inherited: false,
            });
        }

        sqlx::query("DELETE FROM asset_owners WHERE asset_id = $1")
            .bind(asset_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for (ordinal, owner) in owners.iter().enumerate() {
            let ordinal = i32::try_from(ordinal).map_err(|_| {
                StorageError::Unexpected("more owners than an asset can have".to_string())
            })?;
            let (user_id, team_id) = match owner.kind {
                OwnerKind::User => (Some(&owner.id), None),
                OwnerKind::Team => (None, Some(&owner.id)),
            };
            sqlx::query(
                "INSERT INTO asset_owners (asset_id, user_id, team_id, ordinal)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(asset_id)
            .bind(user_id)
            .bind(team_id)
            .bind(ordinal)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                // The unique indexes catch "the same principal twice", which is a
                // client mistake rather than an internal failure — but it is not
                // an *index* mistake, so it does not reuse `UnknownPrincipal`.
                if e.as_database_error()
                    .is_some_and(|d| d.is_unique_violation())
                {
                    StorageError::Conflict {
                        detail: format!("{} is listed as an owner more than once", owner.id),
                        existing_id: None,
                        kind: ConflictKind::AssignmentExists,
                    }
                } else {
                    StorageError::Unexpected(e.to_string())
                }
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(OwnersWrite::Set(resolved))
    }

    async fn asset_owners(&self, asset_id: Uuid) -> Result<Vec<EntityReference>, StorageError> {
        // **The same projection the asset read uses**, deliberately. Two reads
        // that disagree about who owns a table is what a console shows a steward,
        // and the second implementation is where the disagreement comes from —
        // so there is only one, and inheritance is correct here for free.
        let owners: Option<serde_json::Value> = sqlx::query_scalar(&format!(
            "SELECT {OWNERS_JSON} FROM assets WHERE assets.id = $1"
        ))
        .bind(asset_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // No asset is not an error here: the caller has already established the
        // asset exists, or is asking a question whose honest answer is "nobody".
        let Some(owners) = owners else {
            return Ok(Vec::new());
        };
        serde_json::from_value(owners).map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn upsert_team(&self, team: &graph_owl_storage::Team) -> Result<(), StorageError> {
        // One transaction: a team whose row was written and whose membership
        // was not is a team that silently owns things on nobody's behalf.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO teams (id, display_name, description)
             VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE
                SET display_name = EXCLUDED.display_name,
                    description   = EXCLUDED.description",
        )
        .bind(&team.id)
        .bind(&team.display_name)
        .bind(&team.description)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Replaced, not merged. A partial update cannot express "remove
        // everybody", and removal is the operation that has to work.
        sqlx::query("DELETE FROM team_members WHERE team_id = $1")
            .bind(&team.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for member in &team.members {
            sqlx::query("INSERT INTO team_members (team_id, user_id) VALUES ($1, $2)")
                .bind(&team.id)
                .bind(member)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    if e.as_database_error()
                        .is_some_and(|d| d.is_foreign_key_violation())
                    {
                        StorageError::Unexpected(format!(
                            "`{member}` is not a known user; a team member nobody \
                             can resolve is an owner who does not exist"
                        ))
                    } else {
                        StorageError::Unexpected(e.to_string())
                    }
                })?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.find_team", skip_all)]
    async fn find_team(&self, id: &str) -> Result<Option<graph_owl_storage::Team>, StorageError> {
        let row = sqlx::query(
            "SELECT t.id, t.display_name, t.description,
                    COALESCE(
                        ARRAY_AGG(m.user_id ORDER BY m.user_id)
                            FILTER (WHERE m.user_id IS NOT NULL),
                        '{}'
                    ) AS members
               FROM teams t
               LEFT JOIN team_members m ON m.team_id = t.id
              WHERE t.id = $1
              GROUP BY t.id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|row| graph_owl_storage::Team {
            id: row.get("id"),
            display_name: row.get("display_name"),
            description: row.get("description"),
            members: row.get("members"),
        }))
    }

    #[tracing::instrument(name = "storage.teams", skip_all)]
    async fn teams(&self) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
        let rows = sqlx::query(
            "SELECT t.id, t.display_name, t.description,
                    COALESCE(
                        ARRAY_AGG(m.user_id ORDER BY m.user_id)
                            FILTER (WHERE m.user_id IS NOT NULL),
                        '{}'
                    ) AS members
               FROM teams t
               LEFT JOIN team_members m ON m.team_id = t.id
              GROUP BY t.id
              ORDER BY t.id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::Team {
                id: row.get("id"),
                display_name: row.get("display_name"),
                description: row.get("description"),
                members: row.get("members"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.assign_finding", skip_all)]
    async fn assign_finding(
        &self,
        assignment: &graph_owl_storage::Assignment,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO validation_assignments
                 (id, shape, focus_node, path, constraint_kind, assignee, assigned_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(assignment.id)
        .bind(&assignment.shape)
        .bind(&assignment.focus_node)
        .bind(&assignment.path)
        .bind(&assignment.constraint_kind)
        .bind(&assignment.assignee)
        .bind(&assignment.assigned_by)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            let db = e.as_database_error();
            if db.is_some_and(|d| d.is_unique_violation()) {
                StorageError::Conflict {
                    detail: "this finding is already assigned".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::AssignmentExists,
                }
            } else if db.is_some_and(|d| d.is_foreign_key_violation()) {
                // The FK is what makes "assign to a nickname" impossible. Said
                // plainly here so the API can explain it rather than returning
                // a 500 for something the caller can fix.
                StorageError::Unexpected(
                    "that assignee is not a known user; a finding assigned to a \
                     name nobody can resolve looks worked and is not"
                        .to_string(),
                )
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })
    }

    #[tracing::instrument(name = "storage.unassign_finding", skip_all)]
    async fn unassign_finding(&self, id: Uuid) -> Result<bool, StorageError> {
        sqlx::query("DELETE FROM validation_assignments WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.assignments", skip_all)]
    async fn assignments(&self) -> Result<Vec<graph_owl_storage::Assignment>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, shape, focus_node, path, constraint_kind,
                    assignee, assigned_by, assigned_at
               FROM validation_assignments",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::Assignment {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                assignee: row.get("assignee"),
                assigned_by: row.get("assigned_by"),
                assigned_at: row.get("assigned_at"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.recent_runs", skip_all)]
    async fn recent_runs(
        &self,
        service_name: &str,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ConnectorRun>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, connector, service_name, started_at, finished_at,
                    created, skipped, failed, deleted, failures, refusal, triggered_by
               FROM connector_runs
              WHERE ($1 = '' OR service_name = $1)
              ORDER BY started_at DESC
              LIMIT $2",
        )
        .bind(service_name)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::ConnectorRun {
                id: row.get("id"),
                connector: row.get("connector"),
                service_name: row.get("service_name"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
                created: row.get("created"),
                skipped: row.get("skipped"),
                failed: row.get("failed"),
                deleted: row.get("deleted"),
                failures: row.get("failures"),
                refusal: row.get("refusal"),
                triggered_by: row.get("triggered_by"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.source_hashes", skip_all)]
    async fn source_hashes(
        &self,
        fqns: &[String],
    ) -> Result<std::collections::HashMap<String, Option<Vec<u8>>>, StorageError> {
        // Deleted rows are excluded deliberately. A tombstoned asset must look
        // *absent* to a re-run, so the record is created afresh rather than
        // compared against the fingerprint it had before it was deleted — and
        // `upsert_asset` is what refuses to resurrect the tombstone.
        let rows = sqlx::query(
            "SELECT fully_qualified_name, source_hash FROM assets
             WHERE NOT deleted AND fully_qualified_name = ANY($1)",
        )
        .bind(fqns)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("fully_qualified_name"),
                    row.get::<Option<Vec<u8>>, _>("source_hash"),
                )
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.set_source_hash", skip_all)]
    async fn set_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), StorageError> {
        sqlx::query("UPDATE assets SET source_hash = $2 WHERE id = $1")
            .bind(id)
            .bind(hash)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    fn pool_stats(&self) -> Option<graph_owl_storage::PoolStats> {
        Some(graph_owl_storage::PoolStats {
            connections: self.pool.size(),
            // `num_idle` is a `usize` that cannot exceed `size`, which is a
            // `u32` — so the cast is lossless in every state a pool can reach.
            idle: u32::try_from(self.pool.num_idle()).unwrap_or(u32::MAX),
        })
    }

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

    #[tracing::instrument(name = "storage.upsert_asset", skip_all)]
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

        // `RETURNING` cannot carry the owners subquery — it has no `assets` alias
        // to correlate against — so this path reads them. One extra query on a
        // write, rather than a response that reports an owned asset as unowned.
        let mut written = asset_from_row(row);
        written.owners = self.asset_owners(written.id).await?;
        Ok(written)
    }

    #[tracing::instrument(name = "storage.get_asset", skip_all)]
    async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(asset_from_row))
    }

    #[tracing::instrument(name = "storage.get_asset_by_fqn", skip_all)]
    async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets WHERE fully_qualified_name = $1"
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
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
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
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
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
                 SELECT {ASSET_COLUMNS}, {OWNERS_JSON}, 0 AS hops FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id, a.kind, a.name, a.fully_qualified_name, a.parent_id,
                        a.description, a.properties, a.version_major, a.version_minor,
                        a.updated_by, a.change_description, a.deleted, a.deleted_at,
                        a.created_at, a.updated_at, c.hops + 1
                 FROM assets a JOIN chain c ON a.id = c.parent_id
             )
             SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM chain ORDER BY hops DESC"
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
        let Some(terms) = graph_owl_search::tsquery(query) else {
            return Ok(Self::empty_ranked_page(page));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON}, {RANK_KEY} AS sort_key
             FROM assets, to_tsquery('english', $1) AS q (ts)
             WHERE NOT deleted
               AND assets.search_vector @@ q.ts
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR ({RANK_KEY}, id) > ($3, $4))
             ORDER BY {RANK_KEY}, id
             LIMIT $5"
        );
        let q = sqlx::query(&sql)
            .bind(terms)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.ranked_asset_page(q, page).await
    }

    async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError> {
        // `fqn = prefix OR fqn LIKE prefix || '.%'` rather than a bare prefix
        // match: `hdfc-core` must not also match a service called
        // `hdfc-core-archive`, which a plain LIKE would sweep into the scope
        // and then delete.
        //
        // The empty prefix is special-cased to mean *everything*. Left to the
        // general form it becomes `fqn LIKE '.%'`, which is false for every
        // real FQN — so "no restriction" would silently return nothing.
        sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
             WHERE deleted = FALSE AND ($1 = ''
                                        OR fully_qualified_name = $1
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

    #[tracing::instrument(name = "storage.update_asset", skip_all)]
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
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets WHERE id = $1 FOR UPDATE"
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

        // After the commit, for the same reason as `upsert_asset`. Read outside
        // the transaction deliberately: this path never changes owners, so the
        // committed state is the right answer and reading inside would have seen
        // the same rows anyway.
        let mut updated = updated;
        updated.owners = self.asset_owners(updated.id).await?;
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

    #[tracing::instrument(name = "storage.soft_delete_asset", skip_all)]
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

    #[tracing::instrument(name = "storage.list_assets_visible", skip_all)]
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
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
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

    #[tracing::instrument(name = "storage.search_assets_visible", skip_all)]
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
        let Some(terms) = graph_owl_search::tsquery(query) else {
            return Ok(Self::empty_ranked_page(page));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON}, {RANK_KEY} AS sort_key
             FROM assets, to_tsquery('english', $1) AS q (ts)
             WHERE NOT deleted
               AND assets.search_vector @@ q.ts
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR ({RANK_KEY}, id) > ($3, $4))
               {VISIBILITY_SEARCH}
             ORDER BY {RANK_KEY}, id
             LIMIT $5"
        );
        let q = sqlx::query(&sql)
            .bind(terms)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch)
            .bind(&allow)
            .bind(&deny);
        self.ranked_asset_page(q, page).await
    }

    #[tracing::instrument(name = "storage.list_children_visible", skip_all)]
    async fn list_children_visible(
        &self,
        parent_id: Option<Uuid>,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
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
            "SELECT {ASSET_COLUMNS}, {OWNERS_JSON} FROM assets
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

/// The relevance score **and** the keyset cursor for a relevance-ordered page,
/// in one expression.
///
/// `ts_rank_cd` weighs *cover density* — how close the matched terms sit to one
/// another — which is what separates a table called `upi_transactions` from one
/// whose description happens to mention UPI and transactions ten lines apart.
/// Normalisation `32` is `rank / (rank + 1)`; bounded is the point, because an
/// unbounded rank cannot be encoded into a fixed-width sort key.
///
/// One constant rather than a score and a key derived from it: `ORDER BY`, the
/// keyset comparison and the emitted cursor must all be the same expression, and
/// three call sites deriving it separately is three chances for a page boundary
/// to drift.
///
/// `NNNN:fqn`, where `NNNN` is the rank **inverted** — `9999 - rank * 9999` — so
/// that descending relevance is *ascending* string order. Every other list in
/// this adapter paginates with `(sort_key, id) > ($n, $m)`, and inverting here
/// means relevance ordering reuses that comparison unchanged instead of needing
/// a second, differently-directed one.
///
/// Four digits because two documents whose normalised ranks differ by less than
/// 1/10000 are not meaningfully differently relevant to a person, and the FQN
/// suffix makes the ordering total regardless — so the digits only have to
/// separate results a reader could actually tell apart.
const RANK_KEY: &str = "lpad((9999 - (ts_rank_cd(assets.search_vector, q.ts, 32) * 9999)::int)::text, 4, '0') || ':' || assets.fully_qualified_name";

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
        // `try_get`, because the two `RETURNING` paths do not carry this column —
        // a correlated subquery in `RETURNING` cannot see `assets` under that
        // alias. Those paths read owners separately rather than silently
        // reporting none.
        owners: row
            .try_get::<serde_json::Value, _>("owners")
            .ok()
            .and_then(|raw| serde_json::from_value(raw).ok())
            .unwrap_or_default(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// Effective owners as a JSON array, aggregated **in SQL**.
///
/// A correlated subquery rather than a join, because a join multiplies asset rows
/// by owner count and every caller would then have to de-duplicate — and rather
/// than a second query per asset, because a list of 200 assets would become 201
/// round trips against Docker's mapped port at ~30ms each.
///
/// **Effective, not direct** (Epic 11 Slice D). An asset with no owner of its own
/// reports the nearest owned ancestor's owners, flagged `inherited`. The walk is
/// a recursive CTE per row; containment is at most five levels deep
/// (service → database → schema → table → column), so the recursion is bounded by
/// the domain rather than by a limit anybody has to remember to set.
///
/// `ORDER BY hops LIMIT 1` is what makes inheritance **stop at the nearest owned
/// ancestor** rather than accumulate up the chain: "who do I ask about this
/// table" has one answer, and a list that grows with tree depth answers "who
/// might conceivably care" instead.
///
/// `coalesce(..., '[]')` so an unowned asset yields an empty array rather than
/// `NULL`: the domain's `owners` is always a list, and the two must agree or the
/// version classifier sees a field appear and disappear.
///
/// Display names are joined here so a renamed team reads correctly everywhere,
/// and fall back to the id — an owner row can only exist for a live principal
/// (both columns are foreign keys), so the fallback is unreachable defence rather
/// than a real case.
const OWNERS_JSON: &str = "(WITH RECURSIVE ancestry (node, next_up, hops) AS (
            SELECT seed.id, seed.parent_id, 0 FROM assets seed WHERE seed.id = assets.id
        UNION ALL
            SELECT up.id, up.parent_id, ancestry.hops + 1
              FROM assets up JOIN ancestry ON up.id = ancestry.next_up
    ),
    nearest AS (
        SELECT ancestry.node, ancestry.hops
          FROM ancestry
         WHERE EXISTS (SELECT 1 FROM asset_owners o WHERE o.asset_id = ancestry.node)
         ORDER BY ancestry.hops
         LIMIT 1
    )
    SELECT coalesce(json_agg(json_build_object(
        'id',          coalesce(o.user_id, o.team_id),
        'kind',        CASE WHEN o.user_id IS NOT NULL THEN 'user' ELSE 'team' END,
        'displayName', coalesce(u.display_name, t.display_name, o.user_id, o.team_id),
        'inherited',   nearest.hops > 0
    ) ORDER BY o.ordinal), '[]'::json)
    FROM nearest
    JOIN asset_owners o ON o.asset_id = nearest.node
    LEFT JOIN users u ON u.id = o.user_id
    LEFT JOIN teams t ON t.id = o.team_id) AS owners";

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

    /// A relevance-ordered page, whose cursor is the rank key the query
    /// computed rather than the FQN.
    ///
    /// Separate from [`Self::asset_page`] because the cursor has to reproduce
    /// the ordering it came from. Reusing the FQN cursor here would page
    /// through a relevance-ordered result as though it were alphabetical, and
    /// the second page would silently skip and repeat rows.
    ///
    /// [`Self::asset_page`]: Self::asset_page
    async fn ranked_asset_page(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let ranked: Vec<(Asset, String)> = rows
            .into_iter()
            .map(|row| {
                let key: String = row.get("sort_key");
                (asset_from_row(row), key)
            })
            .collect();
        let page = Page::from_overfetch(ranked, page.limit, |(asset, key)| {
            Cursor::new(key.clone(), asset.id)
        });
        Ok(Page {
            data: page.data.into_iter().map(|(asset, _)| asset).collect(),
            paging: page.paging,
        })
    }

    /// A query with no searchable terms matches nothing.
    ///
    /// `to_tsquery('english', '')` raises a syntax error rather than returning
    /// a query that matches nothing, so an all-punctuation search has to be
    /// answered without asking Postgres. An empty result, not an error: the
    /// user typed something unusable, which is not a fault to report.
    fn empty_ranked_page(page: &PageRequest) -> Page<Asset> {
        Page::from_overfetch(Vec::new(), page.limit, |a: &Asset| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        })
    }
}
