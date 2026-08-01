//! Epic 24 Slice A at the storage layer: FQN uniqueness (global and
//! scoped-by-glossary), and "delete a glossary with terms → refused unless
//! recursive" — the two properties an HTTP test cannot pin down as precisely
//! as a direct repository test can.

mod common;

use chrono::Utc;
use graph_owl_core::glossary::{SkosRelation, TermStatus};
use graph_owl_storage::{
    ConflictKind, Glossary, GlossaryDeletion, GlossaryTermRecord, GlossaryTermUpdate, Storage,
    StorageError,
};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

fn mock_glossary(name: &str) -> Glossary {
    let now = Utc::now();
    Glossary {
        id: Uuid::new_v4(),
        name: name.to_string(),
        description: None,
        fully_qualified_name: name.to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn mock_term(glossary_id: Uuid, glossary_fqn: &str, name: &str) -> GlossaryTermRecord {
    let now = Utc::now();
    GlossaryTermRecord {
        id: Uuid::new_v4(),
        glossary_id,
        name: name.to_string(),
        fully_qualified_name: format!("{glossary_fqn}.{name}"),
        definition: String::new(),
        status: TermStatus::Draft,
        synonyms: Vec::new(),
        abbreviations: Vec::new(),
        version: graph_owl_core::envelope::EntityVersion { major: 1, minor: 0 },
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn inserting_a_glossary_persists_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let glossary = mock_glossary("Finance");

    storage
        .insert_glossary(glossary.clone())
        .await
        .expect("insert should succeed");

    let found = storage
        .get_glossary(glossary.id)
        .await
        .expect("get_glossary should succeed");
    assert_eq!(found, Some(glossary));
}

#[tokio::test]
async fn a_duplicate_glossary_fqn_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_glossary(mock_glossary("Finance"))
        .await
        .expect("first insert should succeed");

    let result = storage.insert_glossary(mock_glossary("Finance")).await;

    assert!(matches!(
        result,
        Err(StorageError::Conflict {
            kind: ConflictKind::Fqn,
            ..
        })
    ));
}

// **The negative the conflict test above cannot exercise on its own**: a
// non-uniqueness database error (here, the `CHECK (name <> '')` constraint)
// must surface as `Unexpected`, not be reported as a `Conflict` too. Without
// this, the unique-violation guard could match on *any* database error and
// still pass the positive test.
#[tokio::test]
async fn inserting_a_glossary_with_an_empty_name_is_rejected_as_unexpected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let glossary = Glossary {
        name: String::new(),
        ..mock_glossary("Finance")
    };

    let result = storage.insert_glossary(glossary).await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn every_glossary_is_listed() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_glossary(mock_glossary("Finance"))
        .await
        .expect("insert should succeed");
    storage
        .insert_glossary(mock_glossary("Support"))
        .await
        .expect("insert should succeed");

    let listed = storage
        .list_glossaries()
        .await
        .expect("list_glossaries should succeed");

    assert_eq!(listed.len(), 2);
}

// **The scoped-uniqueness pair the plan names.** A term FQN nests under its
// glossary, so the same term name in two different glossaries produces two
// different FQNs and both inserts must succeed.
#[tokio::test]
async fn the_same_term_name_in_two_glossaries_both_succeed() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    let support = mock_glossary("Support");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_glossary(support.clone())
        .await
        .expect("insert should succeed");

    let first = storage
        .insert_term(mock_term(
            finance.id,
            &finance.fully_qualified_name,
            "Customer",
        ))
        .await;
    let second = storage
        .insert_term(mock_term(
            support.id,
            &support.fully_qualified_name,
            "Customer",
        ))
        .await;

    assert!(first.is_ok(), "{first:?}");
    assert!(second.is_ok(), "{second:?}");
}

// The negative: within **one** glossary the same name collides, because both
// terms would derive the same FQN.
#[tokio::test]
async fn the_same_term_name_twice_in_one_glossary_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(mock_term(
            finance.id,
            &finance.fully_qualified_name,
            "Customer",
        ))
        .await
        .expect("first insert should succeed");

    let result = storage
        .insert_term(mock_term(
            finance.id,
            &finance.fully_qualified_name,
            "Customer",
        ))
        .await;

    assert!(matches!(
        result,
        Err(StorageError::Conflict {
            kind: ConflictKind::Fqn,
            ..
        })
    ));
}

// Same negative as the glossary case: a non-uniqueness database error must
// not be reported as a `Conflict`.
#[tokio::test]
async fn inserting_a_term_with_an_empty_name_is_rejected_as_unexpected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = GlossaryTermRecord {
        name: String::new(),
        ..mock_term(finance.id, &finance.fully_qualified_name, "Customer")
    };

    let result = storage.insert_term(term).await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn getting_a_term_by_id_returns_the_persisted_term() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Customer");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let found = storage
        .get_term(term.id)
        .await
        .expect("get_term should succeed");

    assert_eq!(found, Some(term));
}

#[tokio::test]
async fn getting_a_nonexistent_term_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let found = storage
        .get_term(Uuid::new_v4())
        .await
        .expect("get_term should succeed");

    assert_eq!(found, None);
}

#[tokio::test]
async fn a_term_can_be_updated_and_the_change_is_read_back() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Customer");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .update_term(
            term.id,
            GlossaryTermUpdate {
                definition: Some("a paying party".to_string()),
                synonyms: Some(vec!["client".to_string()]),
                abbreviations: None,
            },
        )
        .await
        .expect("update_term should succeed")
        .expect("term should exist");

    assert_eq!(updated.definition, "a paying party");
    assert_eq!(updated.synonyms, vec!["client".to_string()]);
    assert!(updated.updated_at >= term.updated_at);
}

#[tokio::test]
async fn deleting_a_term_removes_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Customer");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let deleted = storage
        .delete_term(term.id)
        .await
        .expect("delete_term should succeed");

    assert!(deleted);
    assert_eq!(storage.get_term(term.id).await.expect("get_term"), None);
}

#[tokio::test]
async fn deleting_a_nonexistent_term_returns_false() {
    let (storage, _container, _connection_string) = test_storage().await;

    let deleted = storage
        .delete_term(Uuid::new_v4())
        .await
        .expect("delete_term should succeed");

    assert!(!deleted);
}

#[tokio::test]
async fn listing_terms_returns_every_term_in_the_glossary() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let customer = mock_term(finance.id, &finance.fully_qualified_name, "Customer");
    let revenue = mock_term(finance.id, &finance.fully_qualified_name, "Revenue");
    storage
        .insert_term(customer.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(revenue.clone())
        .await
        .expect("insert should succeed");

    let listed = storage
        .list_terms(finance.id)
        .await
        .expect("list_terms should succeed");

    let mut expected = vec![customer, revenue];
    expected.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(listed, expected);
}

// ---- delete_glossary: refused unless recursive ----

#[tokio::test]
async fn deleting_a_glossary_with_terms_is_refused() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(mock_term(
            finance.id,
            &finance.fully_qualified_name,
            "Customer",
        ))
        .await
        .expect("insert should succeed");

    let outcome = storage
        .delete_glossary(finance.id, false)
        .await
        .expect("delete_glossary should succeed");

    assert_eq!(outcome, GlossaryDeletion::HasTerms { term_count: 1 });
    // Refused, not partially applied: the glossary must still be there.
    assert!(
        storage
            .get_glossary(finance.id)
            .await
            .expect("get_glossary")
            .is_some()
    );
}

// The positive half, beside the refusal above: an unconditional "has terms"
// check would pass that test and fail only here.
#[tokio::test]
async fn deleting_a_glossary_recursively_takes_its_terms_with_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Customer");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let outcome = storage
        .delete_glossary(finance.id, true)
        .await
        .expect("delete_glossary should succeed");

    assert_eq!(outcome, GlossaryDeletion::Deleted);
    assert_eq!(
        storage
            .get_glossary(finance.id)
            .await
            .expect("get_glossary"),
        None
    );
    assert_eq!(storage.get_term(term.id).await.expect("get_term"), None);
}

#[tokio::test]
async fn deleting_an_empty_glossary_needs_no_recursive_flag() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");

    let outcome = storage
        .delete_glossary(finance.id, false)
        .await
        .expect("delete_glossary should succeed");

    assert_eq!(outcome, GlossaryDeletion::Deleted);
}

#[tokio::test]
async fn deleting_an_unknown_glossary_is_not_found() {
    let (storage, _container, _connection_string) = test_storage().await;

    let outcome = storage
        .delete_glossary(Uuid::new_v4(), false)
        .await
        .expect("delete_glossary should succeed");

    assert_eq!(outcome, GlossaryDeletion::NotFound);
}

// ---- search: the fields the migration's search_vector actually indexes ----

#[tokio::test]
async fn a_synonym_match_finds_the_term() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = GlossaryTermRecord {
        synonyms: vec!["client".to_string()],
        ..mock_term(finance.id, &finance.fully_qualified_name, "Customer")
    };
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let hits = storage
        .search_terms("client")
        .await
        .expect("search_terms should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, term.id);
}

#[tokio::test]
async fn an_unrelated_word_does_not_match() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(mock_term(
            finance.id,
            &finance.fully_qualified_name,
            "Customer",
        ))
        .await
        .expect("insert should succeed");

    let hits = storage
        .search_terms("zzzznomatch")
        .await
        .expect("search_terms should succeed");

    assert!(hits.is_empty());
}

// ---- Epic 24 Slice B: SKOS relations ----

async fn two_terms(storage: &PostgresStorage) -> (GlossaryTermRecord, GlossaryTermRecord) {
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let child = mock_term(
        finance.id,
        &finance.fully_qualified_name,
        "Checking Account",
    );
    let parent = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(child.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(parent.clone())
        .await
        .expect("insert should succeed");
    (child, parent)
}

#[tokio::test]
async fn inserting_a_relation_persists_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;
    let relation = SkosRelation::Broader(parent.id.to_string());

    storage
        .insert_term_relation(child.id, relation.clone())
        .await
        .expect("insert_term_relation should succeed");

    let stored = storage
        .term_relations_touching(child.id)
        .await
        .expect("term_relations_touching should succeed");
    assert_eq!(stored, vec![(child.id.to_string(), relation)]);
}

// Asserting the same relation twice must not produce two rows — it is one
// fact, and the primary key already guards it; this proves the write path
// treats a repeat as a no-op rather than surfacing the constraint as an error.
#[tokio::test]
async fn asserting_the_same_relation_twice_is_idempotent() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;
    let relation = SkosRelation::Broader(parent.id.to_string());
    storage
        .insert_term_relation(child.id, relation.clone())
        .await
        .expect("first insert should succeed");

    storage
        .insert_term_relation(child.id, relation.clone())
        .await
        .expect("the second insert should succeed rather than conflict");

    let stored = storage
        .term_relations_touching(child.id)
        .await
        .expect("term_relations_touching should succeed");
    assert_eq!(stored.len(), 1, "one fact, not two rows");
}

#[tokio::test]
async fn term_relations_touching_finds_rows_where_the_term_is_the_target() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;
    storage
        .insert_term_relation(child.id, SkosRelation::Broader(parent.id.to_string()))
        .await
        .expect("insert should succeed");

    // The parent never declared anything — it is only ever the *target* of
    // the child's `broader`. Finding it here is what makes deriving
    // `narrower` on read possible at all.
    let touching_parent = storage
        .term_relations_touching(parent.id)
        .await
        .expect("term_relations_touching should succeed");

    assert_eq!(
        touching_parent,
        vec![(
            child.id.to_string(),
            SkosRelation::Broader(parent.id.to_string())
        )]
    );
}

// Every kind the CHECK constraint admits must round-trip through
// `term_relations_touching`, one relation per term so each is read back
// alone rather than only ever exercised alongside `broader`.
#[tokio::test]
async fn every_relation_kind_round_trips_through_term_relations_touching() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let a = mock_term(finance.id, &finance.fully_qualified_name, "A");
    let b = mock_term(finance.id, &finance.fully_qualified_name, "B");
    let c = mock_term(finance.id, &finance.fully_qualified_name, "C");
    let d = mock_term(finance.id, &finance.fully_qualified_name, "D");
    for t in [&a, &b, &c, &d] {
        storage
            .insert_term(t.clone())
            .await
            .expect("insert should succeed");
    }

    let cases = [
        (a.id, SkosRelation::Narrower(b.id.to_string())),
        (b.id, SkosRelation::Related(c.id.to_string())),
        (
            c.id,
            SkosRelation::ExactMatch("http://x.example/exact".to_string()),
        ),
        (
            d.id,
            SkosRelation::CloseMatch("http://x.example/close".to_string()),
        ),
    ];
    for (owner, relation) in &cases {
        storage
            .insert_term_relation(*owner, relation.clone())
            .await
            .expect("insert_term_relation should succeed");
    }

    for (owner, relation) in &cases {
        let touching = storage
            .term_relations_touching(*owner)
            .await
            .expect("term_relations_touching should succeed");
        assert!(
            touching.contains(&(owner.to_string(), relation.clone())),
            "expected {relation:?} to round-trip for {owner}, got {touching:?}"
        );
    }
}

#[tokio::test]
async fn broader_edges_lists_every_broader_pair() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;
    storage
        .insert_term_relation(child.id, SkosRelation::Broader(parent.id.to_string()))
        .await
        .expect("insert should succeed");
    // A `related` edge must not appear among the `broader` edges — the
    // negative that proves the query filters by kind rather than returning
    // every row.
    storage
        .insert_term_relation(parent.id, SkosRelation::Related(child.id.to_string()))
        .await
        .expect("insert should succeed");

    let edges = storage
        .broader_edges()
        .await
        .expect("broader_edges should succeed");

    assert_eq!(edges, vec![(child.id.to_string(), parent.id.to_string())]);
}

#[tokio::test]
async fn deleting_a_relation_the_term_declared_removes_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;
    let relation = SkosRelation::Broader(parent.id.to_string());
    storage
        .insert_term_relation(child.id, relation.clone())
        .await
        .expect("insert should succeed");

    let deleted = storage
        .delete_term_relation(child.id, &relation)
        .await
        .expect("delete_term_relation should succeed");

    assert!(deleted);
    assert!(
        storage
            .term_relations_touching(child.id)
            .await
            .expect("term_relations_touching")
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_a_relation_that_was_never_stored_returns_false() {
    let (storage, _container, _connection_string) = test_storage().await;
    let (child, parent) = two_terms(&storage).await;

    let deleted = storage
        .delete_term_relation(child.id, &SkosRelation::Broader(parent.id.to_string()))
        .await
        .expect("delete_term_relation should succeed");

    assert!(!deleted);
}

// ---- Epic 24 Slice C: review workflow ----

async fn seed_user(storage: &PostgresStorage, id: &str) {
    storage
        .upsert_user(&graph_owl_storage::StoredUser {
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

#[tokio::test]
async fn setting_reviewers_replaces_rather_than_merges() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    seed_user(&storage, "bob").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");
    storage
        .set_term_reviewers(term.id, &["alice".to_string()])
        .await
        .expect("set_term_reviewers should succeed");

    storage
        .set_term_reviewers(term.id, &["bob".to_string()])
        .await
        .expect("set_term_reviewers should succeed");

    let reviewers = storage
        .term_reviewers(term.id)
        .await
        .expect("term_reviewers should succeed");
    assert_eq!(reviewers, vec!["bob".to_string()]);
}

#[tokio::test]
async fn assigning_an_unknown_user_as_reviewer_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let result = storage
        .set_term_reviewers(term.id, &["nobody".to_string()])
        .await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn transitioning_a_term_updates_its_status_and_bumps_the_version() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .transition_term(
            term.id,
            TermStatus::Draft,
            TermStatus::InReview,
            "alice",
            None,
            None,
        )
        .await
        .expect("transition_term should succeed")
        .expect("the term should exist");

    assert_eq!(updated.status, TermStatus::InReview);
    assert!(updated.version.minor > term.version.minor);
}

#[tokio::test]
async fn transitioning_records_a_transition_row() {
    let (storage, _container, connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    storage
        .transition_term(
            term.id,
            TermStatus::Draft,
            TermStatus::InReview,
            "alice",
            None,
            None,
        )
        .await
        .expect("transition_term should succeed");

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("connect");
    let row: (String, String, String) = sqlx::query_as(
        "SELECT from_status, to_status, actor FROM term_transitions WHERE term_id = $1",
    )
    .bind(term.id)
    .fetch_one(&pool)
    .await
    .expect("a transition row");
    assert_eq!(
        row,
        (
            "draft".to_string(),
            "inReview".to_string(),
            "alice".to_string()
        )
    );
}

#[tokio::test]
async fn transitioning_a_deprecated_term_records_the_reason_and_successor() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let old_term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    let new_term = mock_term(finance.id, &finance.fully_qualified_name, "Account V2");
    storage
        .insert_term(old_term.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(new_term.clone())
        .await
        .expect("insert should succeed");

    storage
        .transition_term(
            old_term.id,
            TermStatus::Draft,
            TermStatus::Deprecated,
            "alice",
            Some("superseded".to_string()),
            Some(new_term.id),
        )
        .await
        .expect("transition_term should succeed");

    let fetched = storage
        .get_term(old_term.id)
        .await
        .expect("get_term should succeed")
        .expect("the term should exist");
    assert_eq!(fetched.status, TermStatus::Deprecated);
}

#[tokio::test]
async fn transitioning_an_unknown_term_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let result = storage
        .transition_term(
            Uuid::new_v4(),
            TermStatus::Draft,
            TermStatus::InReview,
            "alice",
            None,
            None,
        )
        .await
        .expect("transition_term should succeed");

    assert_eq!(result, None);
}

// ---- Epic 24 Slice D: terms attach to assets and columns ----

#[tokio::test]
async fn attaching_a_term_persists_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    storage
        .attach_term(term.id, "warehouse.public.orders", "alice")
        .await
        .expect("attach_term should succeed");

    let page = storage
        .term_usage(
            term.id,
            &graph_owl_core::page::PageRequest::new(None, None).expect("valid"),
        )
        .await
        .expect("term_usage should succeed");
    assert_eq!(page.data, vec!["warehouse.public.orders".to_string()]);
}

// Re-attaching what is already attached must not duplicate the row — same
// idempotence rule as asserting a SKOS relation twice.
#[tokio::test]
async fn attaching_the_same_target_twice_is_idempotent() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");
    storage
        .attach_term(term.id, "warehouse.public.orders", "alice")
        .await
        .expect("first attach should succeed");

    storage
        .attach_term(term.id, "warehouse.public.orders", "alice")
        .await
        .expect("the second attach should succeed rather than conflict");

    let page = storage
        .term_usage(
            term.id,
            &graph_owl_core::page::PageRequest::new(None, None).expect("valid"),
        )
        .await
        .expect("term_usage should succeed");
    assert_eq!(page.data.len(), 1, "one fact, not two rows");
}

#[tokio::test]
async fn detaching_a_term_removes_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");
    storage
        .attach_term(term.id, "warehouse.public.orders", "alice")
        .await
        .expect("attach_term should succeed");

    let detached = storage
        .detach_term(term.id, "warehouse.public.orders")
        .await
        .expect("detach_term should succeed");

    assert!(detached);
    let page = storage
        .term_usage(
            term.id,
            &graph_owl_core::page::PageRequest::new(None, None).expect("valid"),
        )
        .await
        .expect("term_usage should succeed");
    assert!(page.data.is_empty());
}

#[tokio::test]
async fn detaching_something_never_attached_returns_false() {
    let (storage, _container, _connection_string) = test_storage().await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let term = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    storage
        .insert_term(term.clone())
        .await
        .expect("insert should succeed");

    let detached = storage
        .detach_term(term.id, "warehouse.public.orders")
        .await
        .expect("detach_term should succeed");

    assert!(!detached);
}

#[tokio::test]
async fn term_usage_lists_only_this_terms_attachments() {
    let (storage, _container, _connection_string) = test_storage().await;
    seed_user(&storage, "alice").await;
    let finance = mock_glossary("Finance");
    storage
        .insert_glossary(finance.clone())
        .await
        .expect("insert should succeed");
    let account = mock_term(finance.id, &finance.fully_qualified_name, "Account");
    let revenue = mock_term(finance.id, &finance.fully_qualified_name, "Revenue");
    storage
        .insert_term(account.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_term(revenue.clone())
        .await
        .expect("insert should succeed");
    storage
        .attach_term(account.id, "warehouse.public.orders", "alice")
        .await
        .expect("attach_term should succeed");
    storage
        .attach_term(revenue.id, "warehouse.public.sales", "alice")
        .await
        .expect("attach_term should succeed");

    let page = storage
        .term_usage(
            account.id,
            &graph_owl_core::page::PageRequest::new(None, None).expect("valid"),
        )
        .await
        .expect("term_usage should succeed");

    assert_eq!(page.data, vec!["warehouse.public.orders".to_string()]);
}
