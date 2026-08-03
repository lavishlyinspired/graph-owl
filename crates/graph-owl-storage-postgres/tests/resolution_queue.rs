//! Epic 17 Slice F against a real Postgres: `queue_for_review` is idempotent
//! by construction (`UNIQUE (target_id, candidate_id)` plus an
//! `ON CONFLICT DO UPDATE` that changes nothing), which is the entire
//! mechanism behind "a rejection is not re-queued on the next re-ingestion
//! of the same draft".

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::resolution::{Evidence, MergeDecidedBy, ReviewQueueEntry, ReviewStatus};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{ReviewQueueFilter, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

async fn asset(storage: &PostgresStorage, name: &str) -> Uuid {
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind: AssetKind::Service,
            name: name.to_string(),
            fully_qualified_name: name.to_string(),
            parent_id: None,
            description: None,
            properties: None,
            extension: None,
            owners: Vec::new(),
            version: EntityVersion::initial(),
            updated_by: "system".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            lifecycle: Default::default(),
            deprecation: None,
        })
        .await
        .expect("asset")
        .id
}

fn entry(target: Uuid, candidate: Uuid, score: f64) -> ReviewQueueEntry {
    ReviewQueueEntry {
        id: Uuid::new_v4(),
        target,
        candidate,
        score,
        evidence: vec![Evidence::SameParent],
        status: ReviewStatus::Pending,
        decided_by: None,
        decided_at: None,
        created_at: Utc::now(),
    }
}

fn all_pending() -> ReviewQueueFilter {
    ReviewQueueFilter {
        limit: 50,
        ..Default::default()
    }
}

#[tokio::test]
async fn a_queued_entry_round_trips() {
    let (storage, _db) = test_storage().await;
    let target = asset(&storage, "orders").await;
    let candidate = asset(&storage, "ORDERS").await;

    let written = storage
        .queue_for_review(entry(target, candidate, 0.75))
        .await
        .expect("queue");

    let fetched = storage
        .get_review_queue_entry(written.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched, written);
    assert_eq!(fetched.status, ReviewStatus::Pending);
}

#[tokio::test]
async fn queuing_the_same_pair_twice_returns_the_original_entry() {
    let (storage, _db) = test_storage().await;
    let target = asset(&storage, "orders").await;
    let candidate = asset(&storage, "ORDERS").await;

    let first = storage
        .queue_for_review(entry(target, candidate, 0.75))
        .await
        .expect("first queue");
    // A different id, a different score — simulating a second, independent
    // resolution of the same draft against the same candidate.
    let second = storage
        .queue_for_review(entry(target, candidate, 0.99))
        .await
        .expect("second queue");

    assert_eq!(
        second, first,
        "a second queue attempt for the same pair must return the first entry unchanged"
    );
}

#[tokio::test]
async fn a_rejected_entry_is_not_reset_by_a_later_queue_attempt() {
    let (storage, _db) = test_storage().await;
    let target = asset(&storage, "orders").await;
    let candidate = asset(&storage, "ORDERS").await;

    let written = storage
        .queue_for_review(entry(target, candidate, 0.75))
        .await
        .expect("queue");
    storage
        .decide_review_queue_entry(
            written.id,
            ReviewStatus::Rejected,
            MergeDecidedBy::Human {
                user_id: "alice".to_string(),
            },
            Utc::now(),
        )
        .await
        .expect("reject");

    // The re-ingestion scenario: the same draft resolves again and tries to
    // queue the identical pair.
    storage
        .queue_for_review(entry(target, candidate, 0.75))
        .await
        .expect("re-queue attempt");

    let fetched = storage
        .get_review_queue_entry(written.id)
        .await
        .expect("get")
        .expect("must still exist");
    assert_eq!(
        fetched.status,
        ReviewStatus::Rejected,
        "a rejection must survive a later re-resolution of the same draft"
    );

    let (pending, total) = storage
        .list_review_queue(&all_pending())
        .await
        .expect("pending list");
    assert_eq!(total, 0, "a rejected entry must not appear as pending");
    assert!(pending.is_empty());
}

#[tokio::test]
async fn deciding_an_already_decided_entry_leaves_it_unchanged() {
    let (storage, _db) = test_storage().await;
    let target = asset(&storage, "orders").await;
    let candidate = asset(&storage, "ORDERS").await;
    let written = storage
        .queue_for_review(entry(target, candidate, 0.75))
        .await
        .expect("queue");

    let alice = MergeDecidedBy::Human {
        user_id: "alice".to_string(),
    };
    storage
        .decide_review_queue_entry(
            written.id,
            ReviewStatus::Confirmed,
            alice.clone(),
            Utc::now(),
        )
        .await
        .expect("first decide");

    let bob = MergeDecidedBy::Human {
        user_id: "bob".to_string(),
    };
    let second = storage
        .decide_review_queue_entry(written.id, ReviewStatus::Rejected, bob, Utc::now())
        .await
        .expect("second decide")
        .expect("entry exists");

    assert_eq!(
        second.status,
        ReviewStatus::Confirmed,
        "the first decision must stand; a second decide call must not flip it"
    );
    assert_eq!(second.decided_by, Some(alice));
}

#[tokio::test]
async fn deciding_an_unknown_entry_is_none() {
    let (storage, _db) = test_storage().await;
    let result = storage
        .decide_review_queue_entry(
            Uuid::new_v4(),
            ReviewStatus::Confirmed,
            MergeDecidedBy::Auto,
            Utc::now(),
        )
        .await
        .expect("decide");
    assert_eq!(result, None);
}

#[tokio::test]
async fn the_queue_is_filterable_by_score_range_and_kind() {
    let (storage, _db) = test_storage().await;
    let target = asset(&storage, "orders").await;
    let low = asset(&storage, "low-cand").await;
    let high = asset(&storage, "high-cand").await;
    storage
        .queue_for_review(entry(target, low, 0.65))
        .await
        .expect("low");
    storage
        .queue_for_review(entry(target, high, 0.95))
        .await
        .expect("high");

    let (narrow, narrow_total) = storage
        .list_review_queue(&ReviewQueueFilter {
            min_score: Some(0.9),
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("narrow");
    assert_eq!(narrow_total, 1);
    assert_eq!(narrow[0].candidate, high);

    let (by_kind, by_kind_total) = storage
        .list_review_queue(&ReviewQueueFilter {
            kind: Some(AssetKind::Service),
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("by kind");
    assert_eq!(by_kind_total, 2, "both entries target a Service asset");
    let _ = by_kind;
}
