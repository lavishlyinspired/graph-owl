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
            parent_team_id: None,
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
        .set_asset_owners(orders, std::slice::from_ref(&platform))
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

// ---- Slice E: assets are filterable by owner ----

/// A three-level estate: service → database → table, so inheritance has somewhere
/// to travel. Returns `(service, database, table)`.
async fn estate(storage: &PostgresStorage, prefix: &str) -> (Uuid, Uuid, Uuid) {
    let now = Utc::now();
    let mut parent = None;
    let mut ids = Vec::new();
    for (kind, name) in [
        (AssetKind::Service, format!("{prefix}-svc")),
        (AssetKind::Database, format!("{prefix}-db")),
        (AssetKind::Table, format!("{prefix}-tbl")),
    ] {
        let fqn = match &parent {
            None => name.clone(),
            Some(_) => format!("{prefix}.{name}"),
        };
        let written = storage
            .upsert_asset(Asset {
                id: Uuid::new_v4(),
                kind,
                name: name.clone(),
                fully_qualified_name: fqn,
                parent_id: parent,
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
            .expect("asset");
        parent = Some(written.id);
        ids.push(written.id);
    }
    (ids[0], ids[1], ids[2])
}

fn everything() -> graph_owl_authz::AccessPredicate {
    graph_owl_authz::AccessPredicate::All
}

fn first_page() -> graph_owl_core::page::PageRequest {
    graph_owl_core::page::PageRequest::new(Some(50), None).expect("page")
}

async fn ids_owned_by(storage: &PostgresStorage, owner: &str) -> Vec<Uuid> {
    storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter {
                owner: Some(owner),
                ..Default::default()
            },
            &first_page(),
            &everything(),
        )
        .await
        .expect("list")
        .data
        .iter()
        .map(|asset| asset.id)
        .collect()
}

// "Matches directly-owned entities."
#[tokio::test]
async fn filtering_by_owner_matches_a_directly_owned_asset() {
    let (storage, _db, _url) = test_storage().await;
    let (service, _database, _table) = estate(&storage, "direct").await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .set_asset_owners(service, &[priya])
        .await
        .expect("set");

    assert!(ids_owned_by(&storage, "priya").await.contains(&service));
}

// **The criterion that makes this feature worth having.** "Matches inherited
// ownership (table owned via its schema)." A direct-only filter answers "show me
// everything my team owns" with "the four things somebody remembered to tag".
#[tokio::test]
async fn filtering_by_owner_matches_an_asset_that_only_inherits_its_owner() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "inherit").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(service, &[platform])
        .await
        .expect("set");

    let matched = ids_owned_by(&storage, "platform").await;

    assert!(matched.contains(&service), "the owned service itself");
    assert!(matched.contains(&database), "the database inherits it");
    assert!(matched.contains(&table), "and so does the table below it");
}

// And the negative that makes the test above about *this* owner rather than about
// a filter that matches everything.
#[tokio::test]
async fn filtering_by_an_owner_excludes_assets_owned_by_somebody_else() {
    let (storage, _db, _url) = test_storage().await;
    let (mine, _, _) = estate(&storage, "mine").await;
    let (theirs, _, _) = estate(&storage, "theirs").await;
    let priya = user(&storage, "priya", "Priya").await;
    let ravi = user(&storage, "ravi", "Ravi").await;
    storage.set_asset_owners(mine, &[priya]).await.expect("set");
    storage
        .set_asset_owners(theirs, &[ravi])
        .await
        .expect("set");

    let matched = ids_owned_by(&storage, "priya").await;

    assert!(matched.contains(&mine));
    assert!(!matched.contains(&theirs), "somebody else's asset matched");
}

// **Inheritance stops at the nearest owned ancestor**, and the filter has to agree
// with the read path about that. If the service is owned by one team and the
// database below it by another, the table's effective owner is the database's —
// so filtering by the service's team must not match the table, or the filter and
// the header would disagree about who owns it.
#[tokio::test]
async fn a_nearer_owner_shadows_a_further_one_for_the_filter_too() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "shadow").await;
    let platform = team(&storage, "platform", "Platform").await;
    let data_eng = team(&storage, "data-eng", "Data Engineering").await;
    storage
        .set_asset_owners(service, &[platform])
        .await
        .expect("set");
    storage
        .set_asset_owners(database, &[data_eng])
        .await
        .expect("set");

    let outer = ids_owned_by(&storage, "platform").await;
    let inner = ids_owned_by(&storage, "data-eng").await;

    assert!(outer.contains(&service));
    assert!(!outer.contains(&table), "the nearer owner should shadow");
    assert!(inner.contains(&table), "the nearer owner should match");
}

// "`?owner={team}` includes assets owned by that team, not by its members
// individually." A filter that expanded team membership would return a steward's
// personal assets when they asked what their team owns.
#[tokio::test]
async fn filtering_by_a_team_does_not_match_assets_owned_by_its_members() {
    let (storage, _db, _url) = test_storage().await;
    let (personal, _, _) = estate(&storage, "personal").await;
    let priya = user(&storage, "priya", "Priya").await;
    storage
        .upsert_team(&graph_owl_storage::Team {
            id: "platform".to_string(),
            display_name: "Platform".to_string(),
            description: None,
            members: vec!["priya".to_string()],
            parent_team_id: None,
        })
        .await
        .expect("team");
    storage
        .set_asset_owners(personal, &[priya])
        .await
        .expect("set");

    let by_team = ids_owned_by(&storage, "platform").await;

    assert!(
        !by_team.contains(&personal),
        "a member's own asset is not the team's"
    );
}

// "Unknown owner id → empty page, not `404`." A filter is a question, and
// "nothing" is a valid answer to it.
#[tokio::test]
async fn filtering_by_an_unknown_owner_is_an_empty_page_rather_than_an_error() {
    let (storage, _db, _url) = test_storage().await;
    estate(&storage, "somebody").await;

    let page = storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter {
                owner: Some("nobody-at-all"),
                ..Default::default()
            },
            &first_page(),
            &everything(),
        )
        .await
        .expect("an empty page, not an error");

    assert!(page.data.is_empty());
}

// "Combines with other filters." The kind filter and the owner filter have to
// intersect rather than one overriding the other.
#[tokio::test]
async fn the_owner_filter_combines_with_the_kind_filter() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "combined").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(service, &[platform])
        .await
        .expect("set");

    let tables = storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter {
                kind: Some(AssetKind::Table),
                owner: Some("platform"),
                ..Default::default()
            },
            &first_page(),
            &everything(),
        )
        .await
        .expect("list")
        .data
        .iter()
        .map(|a| a.id)
        .collect::<Vec<_>>();

    assert!(tables.contains(&table), "the owned table");
    assert!(!tables.contains(&service), "kind filter still applies");
    assert!(!tables.contains(&database), "kind filter still applies");
}

// Absent means unfiltered. A filter that treated `None` as "match nothing" would
// empty every existing list endpoint.
#[tokio::test]
async fn no_owner_filter_returns_everything_visible() {
    let (storage, _db, _url) = test_storage().await;
    let (service, _, _) = estate(&storage, "unfiltered").await;

    let page = storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter::default(),
            &first_page(),
            &everything(),
        )
        .await
        .expect("list");

    assert!(page.data.iter().any(|a| a.id == service));
}

// ---- The ownership-gap report ----
//
// "Which assets have no owner anywhere up their chain" is the query Slice D's
// `inherited` flag exists to make answerable: inheriting *without* saying so turns
// a 5,000-table catalog that nobody has assigned into one that reads as fully
// owned, and then the gap report has nothing to report.

async fn unowned_ids(storage: &PostgresStorage) -> Vec<Uuid> {
    storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter {
                unowned: true,
                ..Default::default()
            },
            &first_page(),
            &everything(),
        )
        .await
        .expect("list")
        .data
        .iter()
        .map(|asset| asset.id)
        .collect()
}

#[tokio::test]
async fn the_gap_report_lists_an_asset_nobody_owns() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "orphan").await;

    let gaps = unowned_ids(&storage).await;

    assert!(gaps.contains(&service));
    assert!(gaps.contains(&database));
    assert!(gaps.contains(&table));
}

// **The criterion that makes the report worth having.** An owner on the service
// covers everything beneath it, so none of the chain is a gap — a report that
// only checked direct ownership would list every table in the estate and be
// ignored within a day.
#[tokio::test]
async fn an_inherited_owner_closes_the_gap_for_everything_below_it() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "covered").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(service, &[platform])
        .await
        .expect("set");

    let gaps = unowned_ids(&storage).await;

    assert!(!gaps.contains(&service), "owned directly");
    assert!(!gaps.contains(&database), "covered by inheritance");
    assert!(!gaps.contains(&table), "covered by inheritance");
}

// The gap is the exact inverse of the owner filter over the same estate: every
// asset is in one set or the other, never both and never neither. If the two ever
// disagree, one of them is lying about effective ownership.
#[tokio::test]
async fn the_gap_report_is_the_inverse_of_the_owner_filter() {
    let (storage, _db, _url) = test_storage().await;
    let (owned_service, owned_db, owned_table) = estate(&storage, "has").await;
    let (bare_service, bare_db, bare_table) = estate(&storage, "hasnt").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(owned_service, &[platform])
        .await
        .expect("set");

    let gaps = unowned_ids(&storage).await;
    let owned = ids_owned_by(&storage, "platform").await;

    for id in [owned_service, owned_db, owned_table] {
        assert!(owned.contains(&id), "should be owned");
        assert!(!gaps.contains(&id), "and so not a gap");
    }
    for id in [bare_service, bare_db, bare_table] {
        assert!(gaps.contains(&id), "should be a gap");
        assert!(!owned.contains(&id), "and so not owned");
    }
}

// Removing the last owner reopens the gap. Ownership is not a one-way ratchet, and
// a report that cached the answer would keep an asset closed after its owner left.
#[tokio::test]
async fn dropping_the_last_owner_reopens_the_gap() {
    let (storage, _db, _url) = test_storage().await;
    let (service, _database, table) = estate(&storage, "reopened").await;
    let platform = team(&storage, "platform", "Platform").await;
    storage
        .set_asset_owners(service, &[platform])
        .await
        .expect("set");
    assert!(!unowned_ids(&storage).await.contains(&table));

    storage.set_asset_owners(service, &[]).await.expect("clear");

    assert!(unowned_ids(&storage).await.contains(&table));
}

// Composes with the kind filter, so "which *tables* has nobody claimed" is one
// request rather than a client-side intersection.
#[tokio::test]
async fn the_gap_report_combines_with_the_kind_filter() {
    let (storage, _db, _url) = test_storage().await;
    let (service, database, table) = estate(&storage, "bykind").await;

    let tables = storage
        .list_assets_visible(
            &graph_owl_storage::AssetFilter {
                kind: Some(AssetKind::Table),
                unowned: true,
                ..Default::default()
            },
            &first_page(),
            &everything(),
        )
        .await
        .expect("list")
        .data
        .iter()
        .map(|a| a.id)
        .collect::<Vec<_>>();

    assert!(tables.contains(&table));
    assert!(!tables.contains(&service), "kind filter still applies");
    assert!(!tables.contains(&database), "kind filter still applies");
}
