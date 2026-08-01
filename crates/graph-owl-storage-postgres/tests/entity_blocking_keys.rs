//! Epic 17 Slice B against a real Postgres: blocking keys computed and
//! indexed on write, candidate generation as an index lookup rather than a
//! scan, and keys that stay current after a rename.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
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

async fn asset(
    storage: &PostgresStorage,
    kind: AssetKind,
    name: &str,
    fqn: &str,
    parent_id: Option<Uuid>,
) -> Asset {
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind,
            name: name.to_string(),
            fully_qualified_name: fqn.to_string(),
            parent_id,
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
}

#[tokio::test]
async fn an_entity_with_no_shared_key_produces_zero_candidates() {
    let (storage, _db) = test_storage().await;
    let lonely = asset(
        &storage,
        AssetKind::Service,
        "zzqxw-only",
        "zzqxw-only",
        None,
    )
    .await;

    let candidates = storage
        .resolution_candidates(lonely.id)
        .await
        .expect("candidates");

    assert!(
        candidates.is_empty(),
        "an isolated asset should have no candidates, not an error"
    );
}

#[tokio::test]
async fn entities_sharing_a_normalized_fqn_key_are_candidates_of_each_other() {
    let (storage, _db) = test_storage().await;
    // Two distinct rows (the unique constraint is on the raw text) that
    // differ only by case — exactly the "connector reports PROD, webhook
    // reports prod" scenario the normalized-FQN key exists for.
    let lower = asset(&storage, AssetKind::Service, "orders", "svc.orders", None).await;
    let upper = asset(&storage, AssetKind::Service, "ORDERS", "SVC.ORDERS", None).await;

    let candidates = storage
        .resolution_candidates(lower.id)
        .await
        .expect("candidates");

    assert!(
        candidates.iter().any(|a| a.id == upper.id),
        "a case-variant FQN should be a candidate via the normalized-FQN key"
    );
}

#[tokio::test]
async fn blocking_keys_are_recomputed_on_rename() {
    let (storage, _db) = test_storage().await;
    let anchor = asset(&storage, AssetKind::Service, "Robert", "svc.anchor", None).await;
    let mut moving = asset(&storage, AssetKind::Service, "Zephyr", "svc.moving", None).await;

    // Before the rename, "Zephyr" shares no blocking key with "Robert".
    let before = storage
        .resolution_candidates(anchor.id)
        .await
        .expect("candidates before rename");
    assert!(
        !before.iter().any(|a| a.id == moving.id),
        "an unrelated name should not be a candidate before the rename"
    );

    // Rename in place: same FQN (the identity), a name whose soundex now
    // matches the anchor's.
    moving.name = "Rupert".to_string();
    moving.updated_at = Utc::now();
    storage.upsert_asset(moving.clone()).await.expect("rename");

    let after = storage
        .resolution_candidates(anchor.id)
        .await
        .expect("candidates after rename");
    assert!(
        after.iter().any(|a| a.id == moving.id),
        "the renamed entity's blocking keys must be recomputed, not left stale"
    );
}

#[tokio::test]
async fn a_table_gaining_columns_recomputes_its_column_hash_key_through_its_children() {
    let (storage, _db) = test_storage().await;

    // Names chosen so they do not collide on soundex or name+parent — the
    // only key these two should ever share is the column-hash, and only
    // once both have the same columns.
    let orders = asset(&storage, AssetKind::Table, "orders", "svc.orders_a", None).await;
    let shipments = asset(
        &storage,
        AssetKind::Table,
        "shipments",
        "svc.shipments_b",
        None,
    )
    .await;
    let payments = asset(
        &storage,
        AssetKind::Table,
        "payments",
        "svc.payments_c",
        None,
    )
    .await;

    // Before any columns exist, a table's column-hash key is the fixed
    // empty-set key, shared by every column-less table — not yet evidence of
    // anything.
    for (table, cols) in [
        (&orders, ["svc.orders_a.id", "svc.orders_a.amount"]),
        (&shipments, ["svc.shipments_b.id", "svc.shipments_b.amount"]),
        (&payments, ["svc.payments_c.foo", "svc.payments_c.bar"]),
    ] {
        for col_fqn in cols {
            let col_name = col_fqn.rsplit('.').next().unwrap();
            asset(
                &storage,
                AssetKind::Column,
                col_name,
                col_fqn,
                Some(table.id),
            )
            .await;
        }
    }

    let candidates = storage
        .resolution_candidates(orders.id)
        .await
        .expect("candidates");

    assert!(
        candidates.iter().any(|a| a.id == shipments.id),
        "a table with the same column set (added after creation) should become \
         a candidate once its parent's column-hash key is recomputed through \
         its children"
    );
    assert!(
        !candidates.iter().any(|a| a.id == payments.id),
        "a table with a genuinely different column set must not be a candidate"
    );
}

/// Below a few thousand rows the planner correctly prefers a sequential scan
/// regardless of what indexes exist, so a plan assertion on a small table
/// would test the planner's arithmetic, not this schema.
const NOISE: i64 = 40_000;

#[tokio::test]
async fn candidate_generation_uses_an_index_scan_not_a_sequential_scan() {
    let (storage, _db) = test_storage().await;

    let target = asset(&storage, AssetKind::Service, "Robert", "svc.target", None).await;

    // Bulk-load synthetic noise directly (bypassing the domain write path,
    // which would cost one round trip per row) so the table is large enough
    // that Postgres's planner has a real choice to make.
    let ids: Vec<Uuid> = (0..NOISE).map(|_| Uuid::new_v4()).collect();
    let names: Vec<String> = (0..NOISE).map(|i| format!("noise-{i}")).collect();
    let fqns: Vec<String> = (0..NOISE).map(|i| format!("svc.noise-{i}")).collect();
    let now = Utc::now();

    sqlx::query(
        "INSERT INTO assets (id, kind, name, fully_qualified_name, created_at, updated_at)
         SELECT id, kind, name, fqn, $5, $5
         FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::text[]) AS t(id, kind, name, fqn)",
    )
    .bind(&ids)
    .bind(vec!["service".to_string(); ids.len()])
    .bind(&names)
    .bind(&fqns)
    .bind(now)
    .execute(storage.pool())
    .await
    .expect("bulk-insert noise assets");

    // Almost all noise gets a unique soundex key; a handful deliberately
    // share the target's ("Robert" -> R163) so the candidate set is
    // non-empty as well as index-served.
    let soundex_values: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if i % 10_000 == 0 {
                "R163".to_string()
            } else {
                graph_owl_core::blocking::soundex(n)
            }
        })
        .collect();

    sqlx::query(
        "INSERT INTO entity_blocking_keys (asset_id, key_type, key_value)
         SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[])",
    )
    .bind(&ids)
    .bind(vec!["soundex_name".to_string(); ids.len()])
    .bind(&soundex_values)
    .execute(storage.pool())
    .await
    .expect("bulk-insert noise blocking keys");

    sqlx::query("ANALYZE entity_blocking_keys")
        .execute(storage.pool())
        .await
        .expect("analyze");

    let plan = storage
        .explain_resolution_candidates(target.id)
        .await
        .expect("explain");

    assert!(
        !plan.contains("Seq Scan on entity_blocking_keys"),
        "candidate generation fell back to a sequential scan over {NOISE} rows.\nPlan was:\n{plan}"
    );
    assert!(
        plan.contains("entity_blocking_keys_lookup"),
        "candidate generation should use the lookup index.\nPlan was:\n{plan}"
    );

    let candidates = storage
        .resolution_candidates(target.id)
        .await
        .expect("candidates");
    assert!(
        !candidates.is_empty(),
        "the deliberately-shared soundex key should produce at least one candidate"
    );
}
