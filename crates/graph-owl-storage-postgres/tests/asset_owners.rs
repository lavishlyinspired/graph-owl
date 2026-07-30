//! Epic 11 Slice C against a real Postgres.
//!
//! `00c-domain-model.md`: "**Single-owner models fail immediately** — every real
//! asset has a producing team and an accountable individual." So the claims here
//! are about *plural, mixed-kind* ownership surviving the schema, and about the
//! order that validation's `owners[1].id` indexing depends on.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::ownership::{OwnerKind, OwnerRef};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{OwnersWrite, Storage, StoredUser};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
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

async fn team(storage: &PostgresStorage, id: &str, name: &str) -> OwnerRef {
    storage
        .upsert_team(&graph_owl_storage::Team {
            id: id.to_string(),
            display_name: name.to_string(),
            description: None,
            members: vec![],
        })
        .await
        .expect("team");
    OwnerRef {
        id: id.to_string(),
        kind: OwnerKind::Team,
    }
}

// The headline criterion: many owners, mixing users and teams.
#[tokio::test]
async fn an_asset_can_be_owned_by_a_person_and_a_team_at_once() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    let platform = team(&storage, "platform", "Platform Team").await;

    let outcome = storage
        .set_asset_owners(orders, &[priya, platform])
        .await
        .expect("set");

    let OwnersWrite::Set(resolved) = outcome else {
        panic!("expected Set, got {outcome:?}");
    };
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].kind, OwnerKind::User);
    assert_eq!(resolved[1].kind, OwnerKind::Team);
    // Denormalized, so a console does not need N follow-up requests to turn ids
    // into names.
    assert_eq!(resolved[0].display_name, "Priya");
    assert_eq!(resolved[1].display_name, "Platform Team");
}

// **Order is a correctness requirement, not presentation.** Validation reports
// failures as `owners[1].id`, so a read that reordered owners would make the index
// name the wrong entry and a client would "fix" the one that was fine.
#[tokio::test]
async fn owners_come_back_in_the_order_they_were_set() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let platform = team(&storage, "platform", "Platform").await;
    let priya = user(&storage, "priya", "Priya").await;

    storage
        .set_asset_owners(orders, &[platform, priya])
        .await
        .expect("set");
    let read = storage.asset_owners(orders).await.expect("read");

    assert_eq!(read[0].id, "platform");
    assert_eq!(read[1].id, "priya");
}

// "Owner referencing a nonexistent principal → 400 naming the index."
#[tokio::test]
async fn an_unknown_principal_is_reported_by_index() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    let ghost = OwnerRef {
        id: "nobody".to_string(),
        kind: OwnerKind::User,
    };

    let outcome = storage
        .set_asset_owners(orders, &[priya, ghost])
        .await
        .expect("no hard error");

    assert_eq!(
        outcome,
        OwnersWrite::UnknownPrincipal {
            index: 1,
            id: "nobody".to_string()
        }
    );
}

// **And nothing is applied.** A bad owner at index 1 must not leave index 0
// written — a partially applied ownership change is worse than a rejected one,
// because it looks like it worked.
#[tokio::test]
async fn a_rejected_owner_list_changes_nothing() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(orders, &[platform.clone()])
        .await
        .expect("set");

    storage
        .set_asset_owners(
            orders,
            &[
                priya,
                OwnerRef {
                    id: "nobody".to_string(),
                    kind: OwnerKind::Team,
                },
            ],
        )
        .await
        .expect("no hard error");

    let read = storage.asset_owners(orders).await.expect("read");
    assert_eq!(read.len(), 1, "the previous owner list should be intact");
    assert_eq!(read[0].id, "platform");
}

// The kind is not inferred, so a team id submitted as a user is unknown rather
// than silently resolving to a team of the same name. `users.id` and `teams.id`
// are both free text and can collide.
#[tokio::test]
async fn a_principal_of_the_wrong_kind_does_not_resolve() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    team(&storage, "shared-name", "A Team").await;

    let outcome = storage
        .set_asset_owners(
            orders,
            &[OwnerRef {
                id: "shared-name".to_string(),
                kind: OwnerKind::User,
            }],
        )
        .await
        .expect("no hard error");

    assert!(matches!(
        outcome,
        OwnersWrite::UnknownPrincipal { index: 0, .. }
    ));
}

// "Removing all owners is allowed — an unowned asset is a real, reportable state."
#[tokio::test]
async fn an_asset_can_be_left_unowned() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(orders, &[priya])
        .await
        .expect("set");

    let outcome = storage.set_asset_owners(orders, &[]).await.expect("set");

    assert_eq!(outcome, OwnersWrite::Set(Vec::new()));
    assert!(storage.asset_owners(orders).await.expect("read").is_empty());
}

// Replace, not merge: the second call is the whole list.
#[tokio::test]
async fn setting_owners_replaces_rather_than_appends() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    let platform = team(&storage, "platform", "Platform").await;

    storage
        .set_asset_owners(orders, &[priya])
        .await
        .expect("set");
    storage
        .set_asset_owners(orders, &[platform])
        .await
        .expect("set");

    let read = storage.asset_owners(orders).await.expect("read");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].id, "platform");
}

#[tokio::test]
async fn the_same_principal_cannot_own_an_asset_twice() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;

    let result = storage
        .set_asset_owners(orders, &[priya.clone(), priya])
        .await;

    assert!(result.is_err(), "a duplicate owner should be refused");
}

#[tokio::test]
async fn setting_owners_on_a_missing_asset_is_not_found() {
    let (storage, _db, _url) = test_storage().await;

    let outcome = storage
        .set_asset_owners(Uuid::new_v4(), &[])
        .await
        .expect("no hard error");

    assert_eq!(outcome, OwnersWrite::NotFound);
}

// **The reason owners are aggregated in SQL rather than stored denormalized.** A
// renamed team reads correctly everywhere, because the display name is joined at
// read time rather than copied when ownership was assigned.
#[tokio::test]
async fn a_renamed_team_shows_its_new_name() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let platform = team(&storage, "platform", "Platform Team").await;
    storage
        .set_asset_owners(orders, &[platform])
        .await
        .expect("set");

    team(&storage, "platform", "Data Platform").await;

    let read = storage.asset_owners(orders).await.expect("read");
    assert_eq!(read[0].display_name, "Data Platform");
}

// Owners reach the *asset* read path, not only the dedicated one — which is what
// the aggregated subquery in `ASSET_COLUMNS` is for, and what a console list needs.
#[tokio::test]
async fn owners_arrive_with_the_asset_itself() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(orders, &[priya])
        .await
        .expect("set");

    let read = storage
        .get_asset(orders)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(
        read.owners.len(),
        1,
        "owners should ride along with the asset"
    );
    assert_eq!(read.owners[0].display_name, "Priya");
}

// An unowned asset reports an empty list rather than `NULL` — the domain's
// `owners` is always a list, and the two must agree or the version classifier
// sees a field appear and disappear.
#[tokio::test]
async fn an_unowned_asset_reads_as_an_empty_list() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;

    let read = storage
        .get_asset(orders)
        .await
        .expect("read")
        .expect("present");

    assert!(read.owners.is_empty());
}

// Deleting a principal takes the ownership row with it rather than leaving a
// dangling name. The whole value of recording an owner is that somebody can be
// *asked*, and a name that resolves to nobody cannot be asked.
#[tokio::test]
async fn deleting_a_principal_removes_its_ownership_rather_than_dangling() {
    let (storage, _db, connection_string) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(orders, &[priya])
        .await
        .expect("set");

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("pool");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind("priya")
        .execute(&pool)
        .await
        .expect("delete");

    assert!(storage.asset_owners(orders).await.expect("read").is_empty());
}
