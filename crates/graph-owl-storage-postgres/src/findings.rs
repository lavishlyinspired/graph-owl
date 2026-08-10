//! Findings, in Postgres — Epic 105 P5.
//!
//! The table carries the invariants too (`governed_by` NOT NULL, a non-empty
//! `evidence` array, a reason required on rejection). Both refusing is the
//! point: a check that lives only in application code is a check the next
//! writer skips.

use async_trait::async_trait;
use graph_owl_core::finding::{Evidence, Finding, FindingStatus};
use graph_owl_storage::StorageError;
use sqlx::Row;
use uuid::Uuid;

use crate::PostgresStorage;

fn finding_from_row(row: &sqlx::postgres::PgRow) -> Result<Finding, StorageError> {
    let status: String = row.get("status");
    let evidence: serde_json::Value = row.get("evidence");
    let evidence = evidence
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| Evidence {
                    subject: item["subject"].as_str().unwrap_or_default().to_string(),
                    predicate: item["predicate"].as_str().unwrap_or_default().to_string(),
                    value: item["value"].as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Finding {
        id: row.get("id"),
        pack: row.get("pack"),
        label: row.get("label"),
        subject: row.get("subject"),
        summary: row.get("summary"),
        governed_by: row.get("governed_by"),
        evidence,
        // An unrecognised status is a schema drift, not a pending finding —
        // defaulting would resurrect a decided one into the queue.
        status: FindingStatus::parse(&status).ok_or_else(|| {
            StorageError::Unexpected(format!("finding has an unknown status `{status}`"))
        })?,
        detected_at: row.get("detected_at"),
        decided_by: row.get("decided_by"),
        reason: row.get("reason"),
    })
}

#[async_trait]
impl graph_owl_storage::FindingStore for PostgresStorage {
    async fn record_finding(&self, finding: &Finding) -> Result<bool, StorageError> {
        let evidence = serde_json::to_value(
            finding
                .evidence
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "subject": e.subject,
                        "predicate": e.predicate,
                        "value": e.value,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `ON CONFLICT DO NOTHING` against the identity index, which spans
        // the evidence digest and **every** status: an identical finding is
        // left exactly as it is, whether a reviewer is still working on it or
        // already decided it. `rows_affected` then distinguishes "new" from
        // "already known" without a second query.
        let result = sqlx::query(
            "INSERT INTO findings
                 (id, pack, label, subject, summary, governed_by, evidence,
                  status, evidence_digest)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8)
             ON CONFLICT DO NOTHING",
        )
        .bind(finding.id)
        .bind(&finding.pack)
        .bind(&finding.label)
        .bind(&finding.subject)
        .bind(&finding.summary)
        .bind(&finding.governed_by)
        .bind(&evidence)
        .bind(finding.evidence_digest())
        .execute(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_findings(
        &self,
        pack: Option<&str>,
        status: Option<FindingStatus>,
    ) -> Result<Vec<Finding>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, pack, label, subject, summary, governed_by, evidence,
                    status, detected_at, decided_by, reason
             FROM findings
             WHERE ($1::text IS NULL OR pack = $1)
               AND ($2::text IS NULL OR status = $2)
             ORDER BY detected_at DESC, id",
        )
        .bind(pack)
        .bind(status.map(FindingStatus::as_str))
        .fetch_all(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        rows.iter().map(finding_from_row).collect()
    }

    async fn decide_finding(
        &self,
        id: Uuid,
        status: FindingStatus,
        decided_by: &str,
        reason: Option<&str>,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE findings
                SET status = $2, decided_by = $3, reason = $4, decided_at = now()
              WHERE id = $1",
        )
        .bind(id)
        .bind(status.as_str())
        .bind(decided_by)
        .bind(reason)
        .execute(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}
