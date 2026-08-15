//! Domain-pack subject attachments, in Postgres — Plan 109 Slice 1.
//!
//! [`findings`](crate::findings)'s sibling: `canonical`/`attached` are
//! `TEXT`, not a foreign key to `assets`, for the same DN-3 reason
//! `findings.subject` already is — a domain-pack subject has no catalog
//! asset row by design.

use async_trait::async_trait;
use graph_owl_core::finding::Evidence;
use graph_owl_core::resolution::{MergeDecidedBy, SubjectAttachment};
use graph_owl_storage::{AttachmentSplitOutcome, StorageError};
use sqlx::Row;
use uuid::Uuid;

use crate::PostgresStorage;

fn subject_attachment_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<SubjectAttachment, StorageError> {
    let evidence: serde_json::Value = row.get("evidence");
    let evidence: Vec<Evidence> =
        serde_json::from_value(evidence).map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let decided_by: serde_json::Value = row.get("decided_by");
    let decided_by: MergeDecidedBy =
        serde_json::from_value(decided_by).map_err(|e| StorageError::Unexpected(e.to_string()))?;

    Ok(SubjectAttachment {
        id: row.get("id"),
        canonical: row.get("canonical"),
        attached: row.get("attached"),
        evidence,
        confidence: row.get("confidence"),
        decided_by,
        decided_at: row.get("decided_at"),
        attached_at_t: row.get("attached_at_t"),
        split_at: row.get("split_at"),
    })
}

#[async_trait]
impl graph_owl_storage::AttachmentStore for PostgresStorage {
    async fn create_subject_attachment(
        &self,
        record: SubjectAttachment,
    ) -> Result<SubjectAttachment, StorageError> {
        let evidence = serde_json::to_value(&record.evidence)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let decided_by = serde_json::to_value(&record.decided_by)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO subject_attachments
                 (id, canonical, attached, evidence, confidence, decided_by, decided_at, attached_at_t, split_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(record.id)
        .bind(&record.canonical)
        .bind(&record.attached)
        .bind(evidence)
        .bind(record.confidence)
        .bind(decided_by)
        .bind(record.decided_at)
        .bind(record.attached_at_t)
        .bind(record.split_at)
        .execute(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(record)
    }

    async fn get_subject_attachment(
        &self,
        id: Uuid,
    ) -> Result<Option<SubjectAttachment>, StorageError> {
        let row = sqlx::query("SELECT * FROM subject_attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        row.as_ref().map(subject_attachment_from_row).transpose()
    }

    async fn subject_attachments_for(
        &self,
        subject: &str,
    ) -> Result<Vec<SubjectAttachment>, StorageError> {
        let rows = sqlx::query(
            "SELECT * FROM subject_attachments WHERE canonical = $1 OR attached = $1
             ORDER BY decided_at, id",
        )
        .bind(subject)
        .fetch_all(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        rows.iter().map(subject_attachment_from_row).collect()
    }

    async fn split_subject_attachment(
        &self,
        id: Uuid,
        split_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<AttachmentSplitOutcome, StorageError> {
        // Atomic for the common path, the same reason
        // `split_merge_record` does this: the `WHERE split_at IS NULL`
        // makes a concurrent double-split lose the race at the database
        // rather than between a Rust-side read and write.
        let row = sqlx::query(
            "UPDATE subject_attachments SET split_at = $2
              WHERE id = $1 AND split_at IS NULL
             RETURNING *",
        )
        .bind(id)
        .bind(split_at)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if let Some(row) = row {
            return Ok(AttachmentSplitOutcome::Split(Box::new(
                subject_attachment_from_row(&row)?,
            )));
        }

        match self.get_subject_attachment(id).await? {
            None => Ok(AttachmentSplitOutcome::NotFound),
            Some(existing) => Ok(AttachmentSplitOutcome::AlreadySplit {
                split_at: existing
                    .split_at
                    .expect("not updated by the statement above because it is already split"),
            }),
        }
    }
}
