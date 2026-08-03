//! Epic 2: the containment cascade, which until now was a constraint nobody
//! had asked a question of.
//!
//! `assets.parent_id REFERENCES assets (id) ON DELETE CASCADE` is declared in
//! `V3__create_assets.sql` with a stated reason — an orphaned column addresses
//! nothing and is invisible to every hierarchy query while still occupying an
//! FQN. Nothing in the product hard-deletes an asset today (deletion is a
//! tombstone, so a connector re-run cannot resurrect one), which is exactly why
//! this went untested: the guarantee has no caller yet.
//!
//! It gets one when `00g-operations.md`'s erasure path lands, and a constraint
//! discovered to be wrong *then* is discovered while deleting somebody's
//! personal data. These are characterisation tests: they document what the
//! schema already promises, so a later migration cannot quietly withdraw it.

mod common;

use chrono::Utc;
use graph_owl_core::{Asset, AssetKind, envelope::EntityVersion};
use graph_owl_storage::Storage;
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

fn asset(kind: AssetKind, name: &str, fqn: &str, parent_id: Option<Uuid>) -> Asset {
    let now = Utc::now();
    Asset {
        id: Uuid::new_v4(),
        kind,
        name: name.to_string(),
        fully_qualified_name: fqn.to_string(),
        parent_id,
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
    }
}

/// service → database → schema → table → column, which is the whole hierarchy.
async fn estate(storage: &PostgresStorage) -> Vec<Uuid> {
    let mut ids = Vec::new();
    let mut parent = None;
    for (kind, name, fqn) in [
        (AssetKind::Service, "hdfc-core", "hdfc-core"),
        (AssetKind::Database, "retail", "hdfc-core.retail"),
        (AssetKind::Schema, "payments", "hdfc-core.retail.payments"),
        (
            AssetKind::Table,
            "upi_transactions",
            "hdfc-core.retail.payments.upi_transactions",
        ),
        (
            AssetKind::Column,
            "amount",
            "hdfc-core.retail.payments.upi_transactions.amount",
        ),
    ] {
        let written = storage
            .upsert_asset(asset(kind, name, fqn, parent))
            .await
            .expect("write");
        parent = Some(written.id);
        ids.push(written.id);
    }
    ids
}

/// Deleting a schema takes its tables and their columns with it — the whole
/// subtree, not just the immediate children.
#[tokio::test]
async fn a_hard_delete_removes_the_entire_subtree_beneath_it() {
    let (storage, _container, connection_string) = test_storage().await;
    let ids = estate(&storage).await;
    let (service, database, schema, table, column) = (ids[0], ids[1], ids[2], ids[3], ids[4]);

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("connect");
    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(schema)
        .execute(&pool)
        .await
        .expect("delete the schema");

    for gone in [schema, table, column] {
        assert!(
            storage.get_asset(gone).await.expect("read").is_none(),
            "{gone} should have been cascaded away"
        );
    }
    // And the negative, which is what stops `ON DELETE CASCADE` from being
    // satisfied by a rule that deletes everything: the ancestors are untouched.
    for kept in [service, database] {
        assert!(
            storage.get_asset(kept).await.expect("read").is_some(),
            "{kept} is an ancestor, not a descendant"
        );
    }
}

/// A sibling subtree is not collateral damage. Without this, a cascade that
/// deleted by FQN prefix — or by nothing at all — would pass the test above.
#[tokio::test]
async fn a_cascade_does_not_reach_a_sibling_branch() {
    let (storage, _container, connection_string) = test_storage().await;
    let ids = estate(&storage).await;
    let (database, schema) = (ids[1], ids[2]);

    let sibling = storage
        .upsert_asset(asset(
            AssetKind::Schema,
            "ledger",
            "hdfc-core.retail.ledger",
            Some(database),
        ))
        .await
        .expect("write");
    let sibling_table = storage
        .upsert_asset(asset(
            AssetKind::Table,
            "entries",
            "hdfc-core.retail.ledger.entries",
            Some(sibling.id),
        ))
        .await
        .expect("write");

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("connect");
    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(schema)
        .execute(&pool)
        .await
        .expect("delete the schema");

    for survivor in [sibling.id, sibling_table.id] {
        assert!(
            storage.get_asset(survivor).await.expect("read").is_some(),
            "{survivor} is in a different branch and must be untouched"
        );
    }
}

/// A **soft** delete is not a cascade at the database level at all. The row
/// stays, its children stay, and the tombstone is what makes a connector re-run
/// unable to resurrect it — which is the behaviour the product actually relies
/// on, and it must not be confused with the constraint above.
#[tokio::test]
async fn a_soft_delete_leaves_every_row_in_place() {
    let (storage, _container, _) = test_storage().await;
    let ids = estate(&storage).await;
    let (schema, column) = (ids[2], ids[4]);

    storage
        .soft_delete_asset(schema, "alice")
        .await
        .expect("soft delete");

    let tombstone = storage
        .get_asset(schema)
        .await
        .expect("read")
        .expect("the row survives a soft delete — that is the point of one");
    assert!(tombstone.deleted);
    assert!(
        storage.get_asset(column).await.expect("read").is_some(),
        "a soft delete does not remove rows, so the subtree is still readable"
    );
}
