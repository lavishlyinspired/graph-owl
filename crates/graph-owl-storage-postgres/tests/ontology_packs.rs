//! Epic 33 storage against a real Postgres: pack/term/override CRUD, the
//! attachment-count and cross-pack-reference queries Slice E's removal
//! guard reads.

mod common;

use chrono::Utc;
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::glossary::{SkosRelation, TermStatus};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_ontology::pack::{Licence, OntologyPack, OverrideKind, PackOverride};
use graph_owl_storage::{Glossary, GlossaryTermRecord, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

async fn glossary(storage: &PostgresStorage, name: &str) -> Uuid {
    let now = Utc::now();
    storage
        .insert_glossary(Glossary {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: None,
            fully_qualified_name: name.to_string(),
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("glossary")
        .id
}

async fn term(storage: &PostgresStorage, glossary_id: Uuid, name: &str) -> Uuid {
    let now = Utc::now();
    storage
        .insert_term(GlossaryTermRecord {
            id: Uuid::new_v4(),
            glossary_id,
            name: name.to_string(),
            fully_qualified_name: format!("{glossary_id}.{name}"),
            definition: "a term".to_string(),
            status: TermStatus::Approved,
            synonyms: Vec::new(),
            abbreviations: Vec::new(),
            version: EntityVersion { major: 1, minor: 0 },
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("term")
        .id
}

fn pack(pack_id: &str, version: &str, glossary_id: Uuid, term_count: usize) -> OntologyPack {
    OntologyPack {
        id: Uuid::new_v4(),
        pack_id: pack_id.to_string(),
        version: version.to_string(),
        licence: Licence::Permissive {
            name: "MIT".to_string(),
        },
        source_url: "http://ex.org/source".to_string(),
        glossary_id,
        term_count,
        imported_at: Utc::now(),
    }
}

#[tokio::test]
async fn a_pack_round_trips_and_a_second_import_of_the_same_version_conflicts() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;

    let inserted = storage
        .insert_pack(pack("fin", "1.0", glossary_id, 0), b"@prefix skos: <x> .")
        .await
        .expect("insert");

    let fetched = storage
        .get_pack(inserted.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched, inserted);

    let by_version = storage
        .get_pack_by_id_and_version("fin", "1.0")
        .await
        .expect("get by version")
        .expect("exists");
    assert_eq!(by_version.id, inserted.id);

    let conflict = storage
        .insert_pack(pack("fin", "1.0", glossary_id, 0), b"different bytes")
        .await
        .expect_err("same (pack_id, version) must conflict");
    assert!(matches!(
        conflict,
        graph_owl_storage::StorageError::Conflict {
            kind: graph_owl_storage::ConflictKind::PackVersionExists,
            ..
        }
    ));
}

#[tokio::test]
async fn source_turtle_round_trips_and_upgrades_in_place() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;
    let inserted = storage
        .insert_pack(pack("fin", "1.0", glossary_id, 0), b"version one bytes")
        .await
        .expect("insert");

    let source = storage
        .get_pack_source_turtle(inserted.id)
        .await
        .expect("get source")
        .expect("exists");
    assert_eq!(source, b"version one bytes");

    let now = Utc::now();
    storage
        .update_pack_version(inserted.id, "2.0", 5, b"version two bytes", now)
        .await
        .expect("update");

    let updated = storage
        .get_pack(inserted.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(
        updated.id, inserted.id,
        "the row is updated in place, not replaced"
    );
    assert_eq!(updated.version, "2.0");
    assert_eq!(updated.term_count, 5);

    let source = storage
        .get_pack_source_turtle(inserted.id)
        .await
        .expect("get source")
        .expect("exists");
    assert_eq!(source, b"version two bytes");
}

#[tokio::test]
async fn pack_terms_round_trip_and_are_scoped_by_pack() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;
    let pack_a = storage
        .insert_pack(pack("fin-a", "1.0", glossary_id, 1), b"a")
        .await
        .expect("pack a");
    let pack_b = storage
        .insert_pack(pack("fin-b", "1.0", glossary_id, 1), b"b")
        .await
        .expect("pack b");
    let term_a = term(&storage, glossary_id, "loan-a").await;
    let term_b = term(&storage, glossary_id, "loan-b").await;

    storage
        .insert_pack_term(pack_a.id, term_a, "http://ex.org/a#Loan")
        .await
        .expect("link a");
    storage
        .insert_pack_term(pack_b.id, term_b, "http://ex.org/b#Loan")
        .await
        .expect("link b");

    let pack_a_terms = storage.pack_terms(pack_a.id).await.expect("terms a");
    assert_eq!(
        pack_a_terms,
        vec![("http://ex.org/a#Loan".to_string(), term_a)]
    );

    let found = storage
        .pack_term_by_iri(pack_a.id, "http://ex.org/a#Loan")
        .await
        .expect("lookup");
    assert_eq!(found, Some(term_a));

    let not_found = storage
        .pack_term_by_iri(pack_a.id, "http://ex.org/b#Loan")
        .await
        .expect("lookup");
    assert_eq!(
        not_found, None,
        "a term must not be findable under the wrong pack"
    );
}

#[tokio::test]
async fn attachment_counts_only_include_terms_with_at_least_one_attachment() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;
    let pack = storage
        .insert_pack(pack("fin", "1.0", glossary_id, 2), b"x")
        .await
        .expect("pack");
    let attached_term = term(&storage, glossary_id, "attached").await;
    let unattached_term = term(&storage, glossary_id, "unattached").await;
    storage
        .insert_pack_term(pack.id, attached_term, "http://ex.org/fin#Attached")
        .await
        .expect("link");
    storage
        .insert_pack_term(pack.id, unattached_term, "http://ex.org/fin#Unattached")
        .await
        .expect("link");

    storage
        .upsert_user(&graph_owl_storage::StoredUser {
            id: "alice".to_string(),
            display_name: "alice".to_string(),
            email: None,
            is_admin: false,
            is_bot: false,
            roles: Vec::new(),
        })
        .await
        .expect("seed user");

    let asset_id = Uuid::new_v4();
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: asset_id,
            kind: AssetKind::Service,
            name: "orders".to_string(),
            fully_qualified_name: "orders".to_string(),
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
            lifecycle: graph_owl_core::lifecycle::LifecycleState::default(),
            deprecation: None,
        })
        .await
        .expect("asset");
    storage
        .attach_term(attached_term, "orders", "alice")
        .await
        .expect("attach");

    let counts = storage
        .pack_attachment_counts(pack.id)
        .await
        .expect("counts");
    assert_eq!(
        counts,
        vec![("http://ex.org/fin#Attached".to_string(), 1)],
        "the unattached term must not appear at all"
    );
}

#[tokio::test]
async fn exact_match_targets_outside_pack_excludes_the_packs_own_relations() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;
    let pack_a = storage
        .insert_pack(pack("fin-a", "1.0", glossary_id, 1), b"a")
        .await
        .expect("pack a");
    let pack_b = storage
        .insert_pack(pack("fin-b", "1.0", glossary_id, 1), b"b")
        .await
        .expect("pack b");
    let term_a = term(&storage, glossary_id, "loan-a").await;
    let term_b = term(&storage, glossary_id, "loan-b").await;
    storage
        .insert_pack_term(pack_a.id, term_a, "http://ex.org/a#Loan")
        .await
        .expect("link a");
    storage
        .insert_pack_term(pack_b.id, term_b, "http://ex.org/b#Loan")
        .await
        .expect("link b");

    // b's term exactMatches a's term — the reference this query must surface.
    storage
        .insert_term_relation(
            term_b,
            SkosRelation::ExactMatch("http://ex.org/a#Loan".to_string()),
        )
        .await
        .expect("relation");
    // a's own exactMatch on itself must never appear in its own outside list.
    storage
        .insert_term_relation(
            term_a,
            SkosRelation::ExactMatch("http://somewhere-else.org#Thing".to_string()),
        )
        .await
        .expect("relation");

    let targets = storage
        .exact_match_targets_outside_pack(pack_a.id)
        .await
        .expect("targets");
    assert_eq!(targets, vec!["http://ex.org/a#Loan".to_string()]);
}

#[tokio::test]
async fn overrides_round_trip_and_are_addressable_by_term_path() {
    let (storage, _db) = test_storage().await;
    let glossary_id = glossary(&storage, "fin").await;
    let pack = storage
        .insert_pack(pack("fin", "1.0", glossary_id, 1), b"x")
        .await
        .expect("pack");

    let override_ = PackOverride {
        id: Uuid::new_v4(),
        pack_id: pack.id,
        term_path: "http://ex.org/fin#Loan".to_string(),
        kind: OverrideKind::Redefine,
        payload: serde_json::json!({ "definition": "house definition" }),
    };
    let written = storage
        .insert_pack_override(override_.clone())
        .await
        .expect("insert override");
    assert_eq!(written, override_);

    let for_path = storage
        .overrides_for_term_path(pack.id, "http://ex.org/fin#Loan")
        .await
        .expect("overrides for path");
    assert_eq!(for_path, vec![override_.clone()]);

    let for_other_path = storage
        .overrides_for_term_path(pack.id, "http://ex.org/fin#Bond")
        .await
        .expect("overrides for other path");
    assert!(for_other_path.is_empty());

    let deleted = storage
        .delete_pack_override(override_.id)
        .await
        .expect("delete");
    assert!(deleted);
    let deleted_again = storage
        .delete_pack_override(override_.id)
        .await
        .expect("delete again");
    assert!(
        !deleted_again,
        "deleting a second time must report false, not error"
    );
}
