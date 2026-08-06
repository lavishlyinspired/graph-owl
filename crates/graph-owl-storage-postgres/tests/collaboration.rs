//! Epic 35 storage against a real Postgres: threads/posts, change proposals,
//! announcements, reactions and the merged activity query — the schema and
//! query behaviour an in-memory fake cannot prove (FK enforcement, tombstone
//! persistence, the window comparison, the toggle's `ON CONFLICT`).

mod common;

use chrono::{Duration, Utc};
use graph_owl_core::collaboration::{
    Announcement, Post, Proposal, ProposalStatus, ReactionKind, Thread,
};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::lifecycle::LifecycleState;
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{Storage, StoredUser};
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
    asset_with_parent(storage, name, None).await
}

async fn asset_with_parent(storage: &PostgresStorage, name: &str, parent_id: Option<Uuid>) -> Uuid {
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind: if parent_id.is_some() {
                AssetKind::Table
            } else {
                AssetKind::Service
            },
            name: name.to_string(),
            fully_qualified_name: name.to_string(),
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
            lifecycle: LifecycleState::default(),
            deprecation: None,
        })
        .await
        .expect("asset")
        .id
}

async fn seed_user(storage: &PostgresStorage, id: &str) {
    storage
        .upsert_user(&StoredUser {
            id: id.to_string(),
            display_name: id.to_string(),
            email: None,
            is_admin: false,
            is_bot: false,
            roles: Vec::new(),
        })
        .await
        .expect("a user");
}

fn thread(about: Uuid, created_by: &str) -> Thread {
    let now = Utc::now();
    Thread {
        id: Uuid::new_v4(),
        about,
        field: None,
        created_by: created_by.to_string(),
        created_at: now,
        resolved: false,
        resolved_by: None,
        resolved_at: None,
    }
}

fn post(thread_id: Uuid, author: &str) -> Post {
    Post {
        id: Uuid::new_v4(),
        thread_id,
        author: author.to_string(),
        message: "hello".to_string(),
        created_at: Utc::now(),
        edited_at: None,
        deleted: false,
    }
}

// ---- Slice A: threads and posts ----

#[tokio::test]
async fn a_thread_and_its_opening_post_round_trip() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;

    let inserted = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let fetched = storage
        .get_thread(inserted.id)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(fetched.about, about);
    assert_eq!(fetched.created_by, "alice");
    assert!(!fetched.resolved);
}

#[tokio::test]
async fn a_thread_anchored_to_a_field_carries_it() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let mut t = thread(about, "alice");
    t.field = Some("description".to_string());

    let inserted = storage.insert_thread(t).await.expect("thread");

    assert_eq!(inserted.field, Some("description".to_string()));
}

#[tokio::test]
async fn deleting_a_post_tombstones_it_rather_than_removing_the_row() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let p = storage
        .insert_post(post(t.id, "alice"))
        .await
        .expect("post");

    let removed = storage.delete_post(p.id).await.expect("delete");
    assert!(removed);

    let (posts, total) = storage.list_posts(t.id, 50, 0).await.expect("list");
    assert_eq!(
        total, 1,
        "the post must still be present for thread structure, just tombstoned"
    );
    assert!(posts[0].deleted, "{posts:?}");
    assert_eq!(
        posts[0].message, "hello",
        "the tombstone preserves the row rather than erasing content at the storage layer"
    );
}

#[tokio::test]
async fn editing_a_post_records_edited_at() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let p = storage
        .insert_post(post(t.id, "alice"))
        .await
        .expect("post");
    assert!(p.edited_at.is_none());

    let now = Utc::now();
    let updated = storage
        .update_post(p.id, "edited message", now)
        .await
        .expect("update")
        .expect("present");

    assert_eq!(updated.message, "edited message");
    assert!(updated.edited_at.is_some());
}

#[tokio::test]
async fn threads_are_listable_and_filterable_by_resolved_state() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let open = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let to_resolve = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    storage
        .resolve_thread(to_resolve.id, "alice", Utc::now())
        .await
        .expect("resolve");

    let (unresolved, total_unresolved) = storage
        .list_threads(about, Some(false), 50, 0)
        .await
        .expect("list");
    assert_eq!(total_unresolved, 1);
    assert_eq!(unresolved[0].id, open.id);

    let (resolved, total_resolved) = storage
        .list_threads(about, Some(true), 50, 0)
        .await
        .expect("list");
    assert_eq!(total_resolved, 1);
    assert_eq!(resolved[0].id, to_resolve.id);
}

// ---- Slice B: resolve/reopen ----

#[tokio::test]
async fn resolving_then_reopening_clears_the_resolution() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");

    let resolved = storage
        .resolve_thread(t.id, "alice", Utc::now())
        .await
        .expect("resolve")
        .expect("present");
    assert!(resolved.resolved);
    assert_eq!(resolved.resolved_by, Some("alice".to_string()));
    assert!(resolved.resolved_at.is_some());

    let reopened = storage
        .reopen_thread(t.id)
        .await
        .expect("reopen")
        .expect("present");
    assert!(!reopened.resolved);
    assert!(reopened.resolved_by.is_none());
    assert!(reopened.resolved_at.is_none());
}

#[tokio::test]
async fn unresolved_thread_count_only_counts_unresolved() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let to_resolve = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    storage
        .resolve_thread(to_resolve.id, "alice", Utc::now())
        .await
        .expect("resolve");

    let count = storage.unresolved_thread_count(about).await.expect("count");

    assert_eq!(count, 1);
}

// ---- Slice C: change proposals ----

fn proposal(about: Uuid, proposed_by: &str) -> Proposal {
    Proposal {
        id: Uuid::new_v4(),
        about,
        field: "description".to_string(),
        current_value: Some("old".to_string()),
        proposed_value: Some("new".to_string()),
        rationale: "it is stale".to_string(),
        status: ProposalStatus::Pending,
        proposed_by: proposed_by.to_string(),
        decided_by: None,
        decided_at: None,
        decision_reason: None,
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn a_proposal_round_trips_pending() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;

    let inserted = storage
        .insert_change_proposal(proposal(about, "alice"))
        .await
        .expect("proposal");

    let fetched = storage
        .get_change_proposal(inserted.id)
        .await
        .expect("read")
        .expect("present");
    assert_eq!(fetched.status, ProposalStatus::Pending);
    assert_eq!(fetched.proposed_by, "alice");
}

#[tokio::test]
async fn deciding_a_pending_proposal_attributes_the_decider_and_reason() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    seed_user(&storage, "bob").await;
    let about = asset(&storage, "orders").await;
    let inserted = storage
        .insert_change_proposal(proposal(about, "alice"))
        .await
        .expect("proposal");

    let decided = storage
        .decide_change_proposal(
            inserted.id,
            ProposalStatus::Rejected,
            "bob",
            Utc::now(),
            Some("not accurate".to_string()),
        )
        .await
        .expect("decide")
        .expect("present");

    assert_eq!(decided.status, ProposalStatus::Rejected);
    assert_eq!(decided.decided_by, Some("bob".to_string()));
    assert_eq!(decided.decision_reason, Some("not accurate".to_string()));
    // The proposer is untouched by who decided it — attribution on accept
    // (tested at the facade/HTTP layer) depends on this column staying put.
    assert_eq!(decided.proposed_by, "alice");
}

#[tokio::test]
async fn deciding_an_already_decided_proposal_leaves_it_unchanged() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let inserted = storage
        .insert_change_proposal(proposal(about, "alice"))
        .await
        .expect("proposal");
    storage
        .decide_change_proposal(
            inserted.id,
            ProposalStatus::Accepted,
            "alice",
            Utc::now(),
            None,
        )
        .await
        .expect("decide");

    let second = storage
        .decide_change_proposal(
            inserted.id,
            ProposalStatus::Rejected,
            "alice",
            Utc::now(),
            Some("too late".to_string()),
        )
        .await
        .expect("decide")
        .expect("present");

    assert_eq!(
        second.status,
        ProposalStatus::Accepted,
        "a second decision must not overwrite the first at the storage layer"
    );
}

#[tokio::test]
async fn proposals_are_listable_per_entity_and_per_user() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let orders = asset(&storage, "orders").await;
    let customers = asset(&storage, "customers").await;
    storage
        .insert_change_proposal(proposal(orders, "alice"))
        .await
        .expect("proposal");
    storage
        .insert_change_proposal(proposal(customers, "alice"))
        .await
        .expect("proposal");

    let (for_orders, total_for_orders) = storage
        .list_change_proposals_for_entity(orders, None, 50, 0)
        .await
        .expect("list");
    assert_eq!(total_for_orders, 1);
    assert_eq!(for_orders[0].about, orders);

    let (by_alice, total_by_alice) = storage
        .list_change_proposals_by_user("alice", 50, 0)
        .await
        .expect("list");
    assert_eq!(total_by_alice, 2);
    assert!(by_alice.iter().all(|p| p.proposed_by == "alice"));
}

// ---- Slice D: announcements ----

fn announcement(
    about: Uuid,
    starts_at: chrono::DateTime<Utc>,
    ends_at: chrono::DateTime<Utc>,
) -> Announcement {
    Announcement {
        id: Uuid::new_v4(),
        about,
        message: "deprecated soon".to_string(),
        starts_at,
        ends_at,
        created_by: "alice".to_string(),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn an_announcement_is_active_at_the_boundary_of_its_window() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let now = Utc::now();
    storage
        .insert_announcement(announcement(about, now, now + Duration::hours(1)))
        .await
        .expect("announcement");

    let active_at_start = storage
        .active_announcements(&[about], now)
        .await
        .expect("active");
    assert_eq!(active_at_start.len(), 1, "inclusive start");

    let active_at_end = storage
        .active_announcements(&[about], now + Duration::hours(1))
        .await
        .expect("active");
    assert!(active_at_end.is_empty(), "exclusive end");
}

#[tokio::test]
async fn an_announcement_outside_its_window_is_retained_but_not_active() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let now = Utc::now();
    storage
        .insert_announcement(announcement(
            about,
            now - Duration::days(2),
            now - Duration::days(1),
        ))
        .await
        .expect("announcement");

    let active = storage
        .active_announcements(&[about], now)
        .await
        .expect("active");
    assert!(active.is_empty());

    let (all, total) = storage
        .list_announcements(about, 50, 0)
        .await
        .expect("list");
    assert_eq!(total, 1, "retained for listing even though inactive");
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn an_announcement_on_a_container_is_visible_via_the_ancestor_id_list() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let schema = asset(&storage, "schema").await;
    let table = asset_with_parent(&storage, "orders", Some(schema)).await;
    let now = Utc::now();
    storage
        .insert_announcement(announcement(schema, now, now + Duration::hours(1)))
        .await
        .expect("announcement");

    // Mirrors what the facade does: fold in `ancestors_of(table)` before
    // asking storage for active announcements across the whole chain.
    let mut ids = vec![table];
    ids.extend(
        storage
            .ancestors_of(table)
            .await
            .expect("ancestors")
            .into_iter()
            .map(|a| a.id),
    );

    let active = storage
        .active_announcements(&ids, now)
        .await
        .expect("active");
    assert_eq!(
        active.len(),
        1,
        "the schema's announcement reaches its table"
    );
}

// ---- Slice E: reactions ----

#[tokio::test]
async fn reacting_twice_with_the_same_kind_toggles_it_off() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let p = storage
        .insert_post(post(t.id, "alice"))
        .await
        .expect("post");

    assert!(
        !storage
            .has_reacted(p.id, "alice", ReactionKind::Helpful)
            .await
            .expect("has")
    );
    storage
        .add_reaction(p.id, "alice", ReactionKind::Helpful)
        .await
        .expect("add");
    assert!(
        storage
            .has_reacted(p.id, "alice", ReactionKind::Helpful)
            .await
            .expect("has")
    );

    let counts = storage.reaction_counts(p.id).await.expect("counts");
    assert_eq!(counts, vec![(ReactionKind::Helpful, 1)]);

    storage
        .remove_reaction(p.id, "alice", ReactionKind::Helpful)
        .await
        .expect("remove");
    assert!(
        !storage
            .has_reacted(p.id, "alice", ReactionKind::Helpful)
            .await
            .expect("has")
    );
    let counts = storage.reaction_counts(p.id).await.expect("counts");
    assert!(counts.is_empty(), "{counts:?}");
}

#[tokio::test]
async fn reactions_are_scoped_per_kind_not_shared() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    let p = storage
        .insert_post(post(t.id, "alice"))
        .await
        .expect("post");

    storage
        .add_reaction(p.id, "alice", ReactionKind::Helpful)
        .await
        .expect("add");
    storage
        .add_reaction(p.id, "alice", ReactionKind::Agree)
        .await
        .expect("add");

    let counts = storage.reaction_counts(p.id).await.expect("counts");
    assert_eq!(counts.len(), 2, "{counts:?}");
}

// ---- Slice F: merged activity ----

#[tokio::test]
async fn collaboration_activity_merges_threads_posts_and_proposals_for_one_entity() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let other = asset(&storage, "customers").await;
    storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");
    storage
        .insert_change_proposal(proposal(about, "alice"))
        .await
        .expect("proposal");
    // A different entity's activity must not leak in.
    storage
        .insert_thread(thread(other, "alice"))
        .await
        .expect("thread");

    let rows = storage
        .collaboration_activity_for_entity(about, 50)
        .await
        .expect("activity");

    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().all(|r| r.actor == "alice"));
}

// ---- The FK boundary: soft delete retains, hard delete cascades ----

#[tokio::test]
async fn a_soft_deleted_entity_retains_its_threads() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");

    storage
        .soft_delete_asset(about, "alice")
        .await
        .expect("soft delete");

    let still_there = storage.get_thread(t.id).await.expect("read");
    assert!(still_there.is_some(), "a soft delete must not cascade");
}

/// This project's `Storage` trait has no hard-delete for assets — deletion
/// is soft everywhere the API reaches (`00g-operations.md`'s erasure story
/// is still open). The `ON DELETE CASCADE` on `threads.about` is schema
/// design for when that lands, and this proves the constraint itself does
/// what its migration comment claims, via a raw delete rather than a
/// `Storage` method that does not exist yet.
#[tokio::test]
async fn the_cascade_constraint_removes_threads_if_the_asset_row_is_ever_actually_deleted() {
    let (storage, _db) = test_storage().await;
    seed_user(&storage, "alice").await;
    let about = asset(&storage, "orders").await;
    let t = storage
        .insert_thread(thread(about, "alice"))
        .await
        .expect("thread");

    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(about)
        .execute(storage.pool())
        .await
        .expect("raw delete");

    let gone = storage.get_thread(t.id).await.expect("read");
    assert!(
        gone.is_none(),
        "the FK cascade must remove threads with their asset"
    );
}
