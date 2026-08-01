//! Epic 17 Slice G against a real Postgres.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::resolution::MentionResolution;
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::Storage;
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

#[tokio::test]
async fn a_recorded_mention_is_listed_by_its_source_most_recent_first() {
    let (storage, _db) = test_storage().await;
    let entity = asset(&storage, "orders").await;
    let source = Uuid::new_v4();
    let other_source = Uuid::new_v4();

    let first = MentionResolution {
        id: Uuid::new_v4(),
        source,
        text: "orders".to_string(),
        entity,
        confidence: 0.8,
        resolved_at: Utc::now(),
    };
    storage
        .record_mention_resolution(first.clone())
        .await
        .expect("first");

    let second = MentionResolution {
        id: Uuid::new_v4(),
        source,
        text: "orders table".to_string(),
        entity,
        confidence: 0.7,
        resolved_at: Utc::now() + chrono::Duration::seconds(1),
    };
    storage
        .record_mention_resolution(second.clone())
        .await
        .expect("second");

    // A different source's mention must not bleed into this source's list.
    storage
        .record_mention_resolution(MentionResolution {
            id: Uuid::new_v4(),
            source: other_source,
            text: "orders".to_string(),
            entity,
            confidence: 0.9,
            resolved_at: Utc::now(),
        })
        .await
        .expect("unrelated");

    let listed = storage
        .mention_resolutions_for_source(source)
        .await
        .expect("list");

    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0], second, "most recent first");
    assert_eq!(listed[1], first);
}

#[tokio::test]
async fn a_source_with_no_mentions_lists_empty() {
    let (storage, _db) = test_storage().await;
    let listed = storage
        .mention_resolutions_for_source(Uuid::new_v4())
        .await
        .expect("list");
    assert!(listed.is_empty());
}
