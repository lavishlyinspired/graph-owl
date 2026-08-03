//! Epic 11 Slices B, F and G against a real Postgres.
//!
//! Slice B's mutator watch names the trap directly: "a check that only compares
//! immediate parent passes depth-1 and fails depth-3". So the cycle tests run at
//! depths 1, 2 and 3, and each one has to fail for a different reason if the walk
//! is wrong.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::ownership::{OwnerKind, OwnerRef};
use graph_owl_core::page::PageRequest;
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{FollowOutcome, PrincipalDeletion, Storage, StoredUser, Team};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

async fn team(storage: &PostgresStorage, id: &str, parent: Option<&str>) {
    storage
        .upsert_team(&Team {
            id: id.to_string(),
            display_name: format!("The {id} team"),
            description: None,
            members: vec![],
            parent_team_id: parent.map(ToString::to_string),
        })
        .await
        .expect("team");
}

async fn user(storage: &PostgresStorage, id: &str) -> OwnerRef {
    storage
        .upsert_user(&StoredUser {
            id: id.to_string(),
            display_name: id.to_string(),
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

fn first_page() -> PageRequest {
    PageRequest::new(Some(50), None).expect("page")
}

// ---- Slice B: nesting ----

#[tokio::test]
async fn a_team_reports_into_its_parent() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;
    team(&storage, "data-eng", Some("platform")).await;

    let stored = storage
        .find_team("data-eng")
        .await
        .expect("read")
        .expect("present");
    let children = storage.child_teams("platform").await.expect("read");

    assert_eq!(stored.parent_team_id.as_deref(), Some("platform"));
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, "data-eng");
}

// A root reports no parent, so the field means something when it is set.
#[tokio::test]
async fn a_root_team_has_no_parent() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;

    let stored = storage
        .find_team("platform")
        .await
        .expect("read")
        .expect("present");

    assert_eq!(stored.parent_team_id, None);
    assert!(
        storage
            .child_teams("platform")
            .await
            .expect("read")
            .is_empty()
    );
}

// **Depth 1.** The cycle a careless update creates.
#[tokio::test]
async fn a_team_cannot_be_its_own_parent() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;

    assert!(
        storage
            .would_cycle("platform", "platform")
            .await
            .expect("check")
    );
}

// **Depth 2.** `A parentOf B`, then `B parentOf A`.
#[tokio::test]
async fn a_two_team_cycle_is_detected() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;
    team(&storage, "data-eng", Some("platform")).await;

    // Making platform report into data-eng would close the loop.
    assert!(
        storage
            .would_cycle("platform", "data-eng")
            .await
            .expect("check")
    );
}

// **Depth 3 — the one a naive check misses.** `A → B → C`, then `C parentOf A`.
// A comparison of immediate parents passes depth 1 and 2 and lets this through,
// leaving an ancestor walk that never terminates.
#[tokio::test]
async fn a_three_team_cycle_is_detected() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "exec", None).await;
    team(&storage, "platform", Some("exec")).await;
    team(&storage, "data-eng", Some("platform")).await;

    assert!(
        storage
            .would_cycle("exec", "data-eng")
            .await
            .expect("check")
    );
}

// And the negative that makes all three about *cycles* rather than about a check
// that always refuses: a legitimate deepening is allowed.
#[tokio::test]
async fn a_legitimate_nesting_is_not_a_cycle() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "exec", None).await;
    team(&storage, "platform", Some("exec")).await;
    team(&storage, "analytics", None).await;

    assert!(
        !storage
            .would_cycle("analytics", "platform")
            .await
            .expect("check")
    );
}

// ---- Slice F: following ----

// "Follow is idempotent (double-follow → `200`, one edge)."
#[tokio::test]
async fn following_twice_creates_one_edge() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    user(&storage, "priya").await;

    let first = storage.follow_asset(orders, "priya").await.expect("follow");
    let second = storage.follow_asset(orders, "priya").await.expect("follow");

    assert_eq!(first, FollowOutcome::Followed);
    assert_eq!(second, FollowOutcome::AlreadyFollowing);
    assert_eq!(storage.follower_count(orders).await.expect("count"), 1);
}

#[tokio::test]
async fn unfollowing_removes_the_edge() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    user(&storage, "priya").await;
    storage.follow_asset(orders, "priya").await.expect("follow");

    storage
        .unfollow_asset(orders, "priya")
        .await
        .expect("unfollow");

    assert_eq!(storage.follower_count(orders).await.expect("count"), 0);
}

// Unfollowing something you do not follow is not an error either — a retried
// unfollow is the state you asked for.
#[tokio::test]
async fn unfollowing_something_unfollowed_is_not_an_error() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    user(&storage, "priya").await;

    assert!(storage.unfollow_asset(orders, "priya").await.is_ok());
}

#[tokio::test]
async fn a_users_follows_are_listed() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let mart = asset(&storage, "mart").await;
    let ignored = asset(&storage, "ignored").await;
    user(&storage, "priya").await;
    storage.follow_asset(orders, "priya").await.expect("follow");
    storage.follow_asset(mart, "priya").await.expect("follow");

    let followed: Vec<Uuid> = storage
        .assets_followed_by("priya", &first_page())
        .await
        .expect("list")
        .data
        .iter()
        .map(|a| a.id)
        .collect();

    assert!(followed.contains(&orders));
    assert!(followed.contains(&mart));
    assert!(!followed.contains(&ignored));
}

// One person's follows are not another's.
#[tokio::test]
async fn follows_are_per_user() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    user(&storage, "priya").await;
    user(&storage, "ravi").await;
    storage.follow_asset(orders, "priya").await.expect("follow");

    let ravi = storage
        .assets_followed_by("ravi", &first_page())
        .await
        .expect("list");

    assert!(ravi.data.is_empty());
    assert_eq!(storage.follower_count(orders).await.expect("count"), 1);
}

// ---- Slice G: deleting a principal ----

// "Deleting a principal owning nothing succeeds."
#[tokio::test]
async fn a_principal_owning_nothing_can_be_deleted() {
    let (storage, _db, _url) = test_storage().await;
    let priya = user(&storage, "priya").await;

    let outcome = storage
        .delete_principal(&priya, None)
        .await
        .expect("delete");

    assert_eq!(outcome, PrincipalDeletion::Deleted { reassigned: 0 });
    assert!(storage.find_user("priya").await.expect("read").is_none());
}

// "Deleting an owner of assets → `409` reporting how many assets and of which
// types." The port reports the holdings; the facade turns them into the message.
#[tokio::test]
async fn deleting_an_owner_reports_what_it_still_holds() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya").await;
    storage
        .set_asset_owners(orders, std::slice::from_ref(&priya))
        .await
        .expect("set");

    let outcome = storage
        .delete_principal(&priya, None)
        .await
        .expect("no hard error");

    let PrincipalDeletion::StillHolds(holdings) = outcome else {
        panic!("expected StillHolds, got {outcome:?}");
    };
    assert_eq!(holdings.owned_total(), 1);
    assert_eq!(holdings.owned_by_kind, vec![(AssetKind::Service, 1)]);
    assert!(
        storage.find_user("priya").await.expect("read").is_some(),
        "the principal must survive a refused delete"
    );
}

// "`?reassignTo={id}` transfers ownership then deletes, in one transaction" —
// asserting *both* halves, because a partial reassign is the named mutator watch.
#[tokio::test]
async fn reassignment_moves_every_asset_and_removes_the_principal() {
    let (storage, _db, _url) = test_storage().await;
    let one = asset(&storage, "one").await;
    let two = asset(&storage, "two").await;
    let priya = user(&storage, "priya").await;
    let ravi = user(&storage, "ravi").await;
    for id in [one, two] {
        storage
            .set_asset_owners(id, std::slice::from_ref(&priya))
            .await
            .expect("set");
    }

    let outcome = storage
        .delete_principal(&priya, Some(&ravi))
        .await
        .expect("delete");

    assert_eq!(outcome, PrincipalDeletion::Deleted { reassigned: 2 });
    assert!(storage.find_user("priya").await.expect("read").is_none());
    for id in [one, two] {
        let owners = storage.asset_owners(id).await.expect("read");
        assert_eq!(owners.len(), 1, "asset {id} should have exactly one owner");
        assert_eq!(owners[0].id, "ravi");
    }
}

// "Reassignment bumps each affected asset's version." Ownership changing is a
// change somebody subscribed to Minor bumps should see; a silent transfer makes
// the audit trail claim nothing happened.
#[tokio::test]
async fn reassignment_bumps_the_version_of_each_asset_it_moves() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya").await;
    let ravi = user(&storage, "ravi").await;
    storage
        .set_asset_owners(orders, std::slice::from_ref(&priya))
        .await
        .expect("set");
    let before = storage
        .get_asset(orders)
        .await
        .expect("read")
        .expect("present")
        .version;

    storage
        .delete_principal(&priya, Some(&ravi))
        .await
        .expect("delete");

    let after = storage
        .get_asset(orders)
        .await
        .expect("read")
        .expect("present")
        .version;
    assert_eq!(after.minor, before.minor + 1, "expected a Minor bump");
    assert_eq!(
        after.major, before.major,
        "ownership is not a breaking change"
    );
}

// "Reassigning to a nonexistent principal → `400`."
#[tokio::test]
async fn reassigning_to_an_unknown_principal_is_refused() {
    let (storage, _db, _url) = test_storage().await;
    let orders = asset(&storage, "orders").await;
    let priya = user(&storage, "priya").await;
    storage
        .set_asset_owners(orders, std::slice::from_ref(&priya))
        .await
        .expect("set");

    let outcome = storage
        .delete_principal(
            &priya,
            Some(&OwnerRef {
                id: "nobody".to_string(),
                kind: OwnerKind::User,
            }),
        )
        .await
        .expect("no hard error");

    assert_eq!(outcome, PrincipalDeletion::UnknownTarget);
    assert!(
        storage.find_user("priya").await.expect("read").is_some(),
        "nothing should have been deleted"
    );
    assert_eq!(
        storage.asset_owners(orders).await.expect("read")[0].id,
        "priya",
        "and nothing should have moved"
    );
}

// "Deleting a team with child teams → `409` unless children are reassigned."
#[tokio::test]
async fn deleting_a_team_with_children_reports_them() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;
    team(&storage, "data-eng", Some("platform")).await;

    let outcome = storage
        .delete_principal(
            &OwnerRef {
                id: "platform".to_string(),
                kind: OwnerKind::Team,
            },
            None,
        )
        .await
        .expect("no hard error");

    let PrincipalDeletion::StillHolds(holdings) = outcome else {
        panic!("expected StillHolds, got {outcome:?}");
    };
    assert_eq!(holdings.child_teams, vec!["data-eng".to_string()]);
}

// Reassigning a team moves its children too, or `ON DELETE RESTRICT` would refuse
// the delete and the reassignment would be a no-op that looked like success.
#[tokio::test]
async fn reassigning_a_team_reparents_its_children() {
    let (storage, _db, _url) = test_storage().await;
    team(&storage, "platform", None).await;
    team(&storage, "exec", None).await;
    team(&storage, "data-eng", Some("platform")).await;

    let outcome = storage
        .delete_principal(
            &OwnerRef {
                id: "platform".to_string(),
                kind: OwnerKind::Team,
            },
            Some(&OwnerRef {
                id: "exec".to_string(),
                kind: OwnerKind::Team,
            }),
        )
        .await
        .expect("delete");

    assert!(matches!(outcome, PrincipalDeletion::Deleted { .. }));
    let moved = storage
        .find_team("data-eng")
        .await
        .expect("read")
        .expect("present");
    assert_eq!(moved.parent_team_id.as_deref(), Some("exec"));
}

#[tokio::test]
async fn deleting_a_principal_that_does_not_exist_is_not_found() {
    let (storage, _db, _url) = test_storage().await;

    let outcome = storage
        .delete_principal(
            &OwnerRef {
                id: "nobody".to_string(),
                kind: OwnerKind::User,
            },
            None,
        )
        .await
        .expect("no hard error");

    assert_eq!(outcome, PrincipalDeletion::NotFound);
}
