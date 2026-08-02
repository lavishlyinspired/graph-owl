//! Epic 11 Slice D: ownership inherits down the containment hierarchy.
//!
//! The value is stated in the plan: "a 5,000-table catalog is navigable without
//! tagging every table individually, **while still showing where ownership is
//! genuinely absent**." Both halves matter, and the second is the one that is
//! easy to lose — an inheritance that does not say it inherited turns an
//! ownership gap into a fully-owned-looking catalog, which is worse than no
//! inheritance at all.
//!
//! These run against a real Postgres because the walk *is* the SQL. A unit test
//! over a hand-built tree would assert that a Rust loop terminates, which was
//! never the risk.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::ownership::{OwnerKind, OwnerRef};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{Storage, StoredUser};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

async fn child(
    storage: &PostgresStorage,
    kind: AssetKind,
    fqn: &str,
    parent_id: Option<Uuid>,
) -> Uuid {
    let now = Utc::now();
    let name = fqn.rsplit('.').next().expect("a leaf segment").to_string();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind,
            name,
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
        })
        .await
        .expect("asset")
        .id
}

/// `warehouse` → `warehouse.retail` → `warehouse.retail.public` →
/// `warehouse.retail.public.orders`, returned outermost-first.
async fn estate(storage: &PostgresStorage) -> [Uuid; 4] {
    let service = child(storage, AssetKind::Service, "warehouse", None).await;
    let database = child(
        storage,
        AssetKind::Database,
        "warehouse.retail",
        Some(service),
    )
    .await;
    let schema = child(
        storage,
        AssetKind::Schema,
        "warehouse.retail.public",
        Some(database),
    )
    .await;
    let table = child(
        storage,
        AssetKind::Table,
        "warehouse.retail.public.orders",
        Some(schema),
    )
    .await;
    [service, database, schema, table]
}

async fn user(storage: &PostgresStorage, id: &str, name: &str) -> OwnerRef {
    storage
        .upsert_user(&StoredUser {
            id: id.to_string(),
            display_name: name.to_string(),
            email: None,
            is_admin: false,
            is_bot: false,
            roles: vec![],
        })
        .await
        .expect("user");
    OwnerRef {
        id: id.to_string(),
        kind: OwnerKind::User,
    }
}

// The headline case. Nobody named an owner on the table; the schema has one; the
// table answers "who owns this" rather than shrugging.
#[tokio::test]
async fn a_table_with_no_owner_reports_its_schemas_owner_as_inherited() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, _database, schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(schema, &[priya])
        .await
        .expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 1, "{owners:?}");
    assert_eq!(owners[0].id, "priya");
    assert_eq!(owners[0].display_name, "Priya");
    assert!(
        owners[0].inherited,
        "an owner found by walking up must say so: {owners:?}"
    );
}

// The other half of the flag's job. An owner recorded on the entity itself is a
// deliberate governance statement, and reporting it as inherited would send a
// steward looking for a parent that decided it.
#[tokio::test]
async fn an_owner_recorded_on_the_table_is_not_reported_as_inherited() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, _database, _schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(table, &[priya])
        .await
        .expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 1);
    assert!(!owners[0].inherited, "{owners:?}");
}

// **Mutator watch: a single-hop-only walk passes every test above and fails
// this one.** Ownership is usually recorded at the database or service level, so
// the multi-hop case is the common one in a real estate, not the exotic one.
#[tokio::test]
async fn inheritance_crosses_more_than_one_level() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, database, _schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(database, &[priya])
        .await
        .expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 1, "two hops up is still an ancestor");
    assert_eq!(owners[0].id, "priya");
    assert!(owners[0].inherited);
}

// **Mutator watch: accumulate-all-ancestors passes the two tests above and fails
// this one.** Inheritance answers "who do I ask", and a list that grows with the
// depth of the tree answers "who might conceivably care" — the schema's owner is
// the answer precisely because it is the nearest one.
#[tokio::test]
async fn inheritance_stops_at_the_nearest_owned_ancestor() {
    let (storage, _db, _url) = test_storage().await;
    let [service, _database, schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    let raj = user(&storage, "raj", "Raj").await;
    storage
        .set_asset_owners(service, &[raj])
        .await
        .expect("set");
    storage
        .set_asset_owners(schema, &[priya])
        .await
        .expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 1, "only the nearest: {owners:?}");
    assert_eq!(owners[0].id, "priya");
}

// A direct owner is the whole answer, not the first entry of one. Without this,
// "stop at the nearest ancestor" could be implemented as "stop at the nearest
// *strict* ancestor" and quietly append a parent's owner to every owned table.
#[tokio::test]
async fn a_directly_owned_table_does_not_also_list_its_schemas_owner() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, _database, schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    let raj = user(&storage, "raj", "Raj").await;
    storage
        .set_asset_owners(schema, &[priya])
        .await
        .expect("set");
    storage.set_asset_owners(table, &[raj]).await.expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 1, "{owners:?}");
    assert_eq!(owners[0].id, "raj");
    assert!(!owners[0].inherited);
}

// An unowned estate is a reportable state, not a failure. This is the row the
// ownership-gap report exists to find, so it must survive the walk intact.
#[tokio::test]
async fn nothing_owned_anywhere_is_an_empty_list_not_an_error() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, _database, _schema, table] = estate(&storage).await;

    let owners = storage.asset_owners(table).await.expect("read");

    assert!(owners.is_empty(), "{owners:?}");
}

// The inherited list is the ancestor's list *entire*, and the plural model is
// the point of Slice C — an inheritance that silently took only the first owner
// would drop the accountable individual or the producing team, whichever was
// recorded second.
#[tokio::test]
async fn every_owner_of_the_nearest_ancestor_is_inherited_in_order() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, _database, schema, table] = estate(&storage).await;
    storage
        .upsert_team(&graph_owl_storage::Team {
            id: "platform".to_string(),
            display_name: "Platform Team".to_string(),
            description: None,
            members: vec![],
            parent_team_id: None,
        })
        .await
        .expect("team");
    let priya = user(&storage, "priya", "Priya").await;
    let platform = OwnerRef {
        id: "platform".to_string(),
        kind: OwnerKind::Team,
    };
    storage
        .set_asset_owners(schema, &[platform, priya])
        .await
        .expect("set");

    let owners = storage.asset_owners(table).await.expect("read");

    assert_eq!(owners.len(), 2, "{owners:?}");
    assert_eq!(owners[0].id, "platform");
    assert_eq!(owners[1].id, "priya");
    assert!(owners.iter().all(|owner| owner.inherited));
}

// The asset read carries the same answer as the dedicated endpoint. Two reads
// that disagree about who owns a table is the failure mode a console shows to a
// steward, and it is the reason this is projected in one place.
#[tokio::test]
async fn the_asset_read_carries_the_same_inherited_owners() {
    let (storage, _db, _url) = test_storage().await;
    let [_service, database, _schema, table] = estate(&storage).await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(database, &[priya])
        .await
        .expect("set");

    let asset = storage
        .get_asset(table)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(asset.owners.len(), 1, "{:?}", asset.owners);
    assert_eq!(asset.owners[0].id, "priya");
    assert!(asset.owners[0].inherited);
}
