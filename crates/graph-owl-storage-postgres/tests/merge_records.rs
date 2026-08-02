//! Epic 17 Slices D/E against a real Postgres: a merge is a record with a
//! `split_at`, never a delete, and splitting is idempotent-safe (a second
//! split reports what the first one already did rather than silently
//! reapplying).

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::resolution::{Evidence, MergeDecidedBy, MergeRecord};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{SplitOutcome, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

async fn asset(storage: &PostgresStorage, name: &str, fqn: &str) -> Uuid {
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind: AssetKind::Service,
            name: name.to_string(),
            fully_qualified_name: fqn.to_string(),
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
        })
        .await
        .expect("asset")
        .id
}

fn merge_record(canonical: Uuid, merged: Uuid) -> MergeRecord {
    MergeRecord {
        id: Uuid::new_v4(),
        canonical,
        merged,
        evidence: vec![Evidence::NormalizedFqn],
        confidence: 1.0,
        decided_by: MergeDecidedBy::Auto,
        decided_at: Utc::now(),
        merged_at_t: 1,
        split_at: None,
    }
}

#[tokio::test]
async fn a_created_merge_record_round_trips() {
    let (storage, _db) = test_storage().await;
    let canonical = asset(&storage, "orders", "svc.orders").await;
    let merged = asset(&storage, "ORDERS", "SVC.ORDERS").await;

    let record = storage
        .create_merge_record(merge_record(canonical, merged))
        .await
        .expect("create");

    let fetched = storage
        .get_merge_record(record.id)
        .await
        .expect("get")
        .expect("must exist");

    assert_eq!(fetched, record);
    assert_eq!(fetched.split_at, None);
}

#[tokio::test]
async fn splitting_an_unknown_merge_is_not_found() {
    let (storage, _db) = test_storage().await;
    let outcome = storage
        .split_merge_record(Uuid::new_v4(), Utc::now())
        .await
        .expect("split");
    assert_eq!(outcome, SplitOutcome::NotFound);
}

#[tokio::test]
async fn splitting_a_live_merge_marks_it_split() {
    let (storage, _db) = test_storage().await;
    let canonical = asset(&storage, "orders", "svc.orders").await;
    let merged = asset(&storage, "ORDERS", "SVC.ORDERS").await;
    let record = storage
        .create_merge_record(merge_record(canonical, merged))
        .await
        .expect("create");

    let split_at = Utc::now();
    let outcome = storage
        .split_merge_record(record.id, split_at)
        .await
        .expect("split");

    match outcome {
        SplitOutcome::Split(split) => assert_eq!(split.split_at, Some(split_at)),
        other => panic!("expected Split, got {other:?}"),
    }

    // Not deleted — the record survives with the split recorded on it.
    let fetched = storage
        .get_merge_record(record.id)
        .await
        .expect("get")
        .expect("must still exist");
    assert_eq!(fetched.split_at, Some(split_at));
}

#[tokio::test]
async fn splitting_an_already_split_merge_reports_the_original_split_time() {
    let (storage, _db) = test_storage().await;
    let canonical = asset(&storage, "orders", "svc.orders").await;
    let merged = asset(&storage, "ORDERS", "SVC.ORDERS").await;
    let record = storage
        .create_merge_record(merge_record(canonical, merged))
        .await
        .expect("create");

    let first_split = Utc::now();
    storage
        .split_merge_record(record.id, first_split)
        .await
        .expect("first split");

    let second_attempt = Utc::now();
    let outcome = storage
        .split_merge_record(record.id, second_attempt)
        .await
        .expect("second split attempt");

    assert_eq!(
        outcome,
        SplitOutcome::AlreadySplit {
            split_at: first_split
        },
        "a second split must not silently move the split time forward"
    );
}

#[tokio::test]
async fn most_recent_split_between_finds_the_pair_in_either_role() {
    let (storage, _db) = test_storage().await;
    let canonical = asset(&storage, "orders", "svc.orders").await;
    let merged = asset(&storage, "ORDERS", "SVC.ORDERS").await;
    let record = storage
        .create_merge_record(merge_record(canonical, merged))
        .await
        .expect("create");
    let split_at = Utc::now();
    storage
        .split_merge_record(record.id, split_at)
        .await
        .expect("split");

    let forward = storage
        .most_recent_split_between(canonical, merged)
        .await
        .expect("forward lookup");
    let reversed = storage
        .most_recent_split_between(merged, canonical)
        .await
        .expect("reversed lookup");

    assert_eq!(forward, Some(split_at));
    assert_eq!(reversed, Some(split_at));
}

#[tokio::test]
async fn most_recent_split_between_is_none_for_an_unrelated_pair_or_an_unsplit_merge() {
    let (storage, _db) = test_storage().await;
    let canonical = asset(&storage, "orders", "svc.orders").await;
    let merged = asset(&storage, "ORDERS", "SVC.ORDERS").await;
    let unrelated = asset(&storage, "payments", "svc.payments").await;

    // A live (unsplit) merge must not read as a cooldown.
    storage
        .create_merge_record(merge_record(canonical, merged))
        .await
        .expect("create");
    assert_eq!(
        storage
            .most_recent_split_between(canonical, merged)
            .await
            .expect("lookup"),
        None
    );

    // A pair that never merged at all.
    assert_eq!(
        storage
            .most_recent_split_between(canonical, unrelated)
            .await
            .expect("lookup"),
        None
    );
}
