//! Epic 24 Slice A at the storage layer: FQN uniqueness (global and
//! scoped-by-glossary), and "delete a glossary with terms → refused unless
//! recursive" — the two properties an HTTP test cannot pin down as precisely
//! as a direct repository test can.

mod common;

use chrono::Utc;
use graph_owl_core::glossary::TermStatus;
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
