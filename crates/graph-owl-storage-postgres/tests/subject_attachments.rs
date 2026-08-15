//! Plan 109 Slice 1 against a real Postgres: an attachment is a record with
//! a `split_at`, never a delete, and splitting is idempotent-safe (a second
//! split reports what the first one already did rather than silently
//! reapplying) — the same contract `merge_records.rs` already proves for
//! catalog assets, exercised here for domain-pack subjects instead.

mod common;

use chrono::Utc;
use graph_owl_core::finding::Evidence;
use graph_owl_core::resolution::{MergeDecidedBy, SubjectAttachment};
use graph_owl_storage::{AttachmentSplitOutcome, AttachmentStore};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn attachment(canonical: &str, attached: &str) -> SubjectAttachment {
    SubjectAttachment {
        id: Uuid::new_v4(),
        canonical: canonical.to_string(),
        attached: attached.to_string(),
        evidence: vec![Evidence {
            subject: attached.to_string(),
            predicate: "supplierGstin".to_string(),
            value: "27AABCU9603R1ZM".to_string(),
            var: None,
        }],
        confidence: 1.0,
        decided_by: MergeDecidedBy::Auto,
        decided_at: Utc::now(),
        attached_at_t: 1,
        split_at: None,
    }
}

#[tokio::test]
async fn a_created_subject_attachment_round_trips() {
    let (storage, _db) = test_storage().await;

    let record = storage
        .create_subject_attachment(attachment(
            "gst:invoice-INV1001-27AABCU9603R1ZM",
            "gst:pr-INV-1001",
        ))
        .await
        .expect("create");

    let fetched = storage
        .get_subject_attachment(record.id)
        .await
        .expect("get")
        .expect("must exist");

    assert_eq!(fetched, record);
    assert_eq!(fetched.split_at, None);
}

#[tokio::test]
async fn subject_attachments_for_finds_the_pair_in_either_role() {
    let (storage, _db) = test_storage().await;
    let canonical = "gst:invoice-INV1001-27AABCU9603R1ZM";
    let attached = "gst:pr-INV-1001";
    let record = storage
        .create_subject_attachment(attachment(canonical, attached))
        .await
        .expect("create");

    let by_canonical = storage
        .subject_attachments_for(canonical)
        .await
        .expect("lookup by canonical");
    let by_attached = storage
        .subject_attachments_for(attached)
        .await
        .expect("lookup by attached");

    assert_eq!(by_canonical, vec![record.clone()]);
    assert_eq!(by_attached, vec![record]);
}

#[tokio::test]
async fn subject_attachments_for_an_unrelated_subject_is_empty() {
    let (storage, _db) = test_storage().await;
    storage
        .create_subject_attachment(attachment(
            "gst:invoice-INV1001-27AABCU9603R1ZM",
            "gst:pr-INV-1001",
        ))
        .await
        .expect("create");

    let found = storage
        .subject_attachments_for("gst:invoice-unrelated")
        .await
        .expect("lookup");

    assert!(found.is_empty());
}

#[tokio::test]
async fn splitting_an_unknown_attachment_is_not_found() {
    let (storage, _db) = test_storage().await;
    let outcome = storage
        .split_subject_attachment(Uuid::new_v4(), Utc::now())
        .await
        .expect("split");
    assert_eq!(outcome, AttachmentSplitOutcome::NotFound);
}

#[tokio::test]
async fn splitting_a_live_attachment_marks_it_split() {
    let (storage, _db) = test_storage().await;
    let record = storage
        .create_subject_attachment(attachment(
            "gst:invoice-INV1001-27AABCU9603R1ZM",
            "gst:pr-INV-1001",
        ))
        .await
        .expect("create");

    let split_at = Utc::now();
    let outcome = storage
        .split_subject_attachment(record.id, split_at)
        .await
        .expect("split");

    match outcome {
        AttachmentSplitOutcome::Split(split) => assert_eq!(split.split_at, Some(split_at)),
        other => panic!("expected Split, got {other:?}"),
    }

    // Not deleted — the record survives with the split recorded on it, and
    // its evidence is unchanged: a split explains a mistaken attachment, it
    // does not erase why it was made.
    let fetched = storage
        .get_subject_attachment(record.id)
        .await
        .expect("get")
        .expect("must still exist");
    assert_eq!(fetched.split_at, Some(split_at));
    assert_eq!(
        fetched.evidence,
        vec![Evidence {
            subject: "gst:pr-INV-1001".to_string(),
            predicate: "supplierGstin".to_string(),
            value: "27AABCU9603R1ZM".to_string(),
            var: None,
        }]
    );
}

#[tokio::test]
async fn splitting_an_already_split_attachment_reports_the_original_split_time() {
    let (storage, _db) = test_storage().await;
    let record = storage
        .create_subject_attachment(attachment(
            "gst:invoice-INV1001-27AABCU9603R1ZM",
            "gst:pr-INV-1001",
        ))
        .await
        .expect("create");

    let first_split = Utc::now();
    storage
        .split_subject_attachment(record.id, first_split)
        .await
        .expect("first split");

    let second_attempt = Utc::now();
    let outcome = storage
        .split_subject_attachment(record.id, second_attempt)
        .await
        .expect("second split attempt");

    assert_eq!(
        outcome,
        AttachmentSplitOutcome::AlreadySplit {
            split_at: first_split
        },
        "a second split must not silently move the split time forward"
    );
}
