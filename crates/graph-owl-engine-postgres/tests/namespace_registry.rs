//! Epic 105 DN-1: namespaces definable at runtime, so a domain brings its own
//! vocabulary without a release.
//!
//! The unit tests in `graph_owl_core::namespaces` prove the resolution logic;
//! these prove the half that only a real database can — that a declaration
//! *persists*, that the code and IRI are both immutable once written, and that
//! allocation never hands the same code out twice.

mod common;

use graph_owl_core::flake::{Sid, namespace};
use graph_owl_core::namespaces::{RuntimeNamespaces, resolve_iri};
use graph_owl_engine::{NamespaceDef, NamespaceRegistry, RegistryError};
use graph_owl_engine_postgres::PostgresTripleStore;

const HOSP_IRI: &str = "https://example.org/ns/hospitality#";

async fn store() -> (PostgresTripleStore, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, database, connection_string)
}

fn declaration(code: u16, iri: &str) -> NamespaceDef {
    NamespaceDef {
        code,
        iri: iri.to_string(),
        declared_by: "pack:hospitality".to_string(),
    }
}

#[tokio::test]
async fn a_declared_namespace_is_read_back() {
    let (store, _db, _url) = store().await;

    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("a fresh code is free");

    let all = store.namespaces().await.expect("list");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].code, namespace::RUNTIME_START);
    assert_eq!(all[0].iri, HOSP_IRI);
    assert_eq!(
        all[0].declared_by, "pack:hospitality",
        "provenance travels with the namespace — a code outlives the pack that \
         introduced it, so who asked for it has to be recorded"
    );
}

#[tokio::test]
async fn a_declared_namespace_makes_a_domain_iri_resolvable() {
    // The whole point, end to end: a pack declares its vocabulary, a resolver
    // is built from what persisted, and the pack's own IRIs become real graph
    // terms. Before this existed the only way to reach it was adding a
    // constant to `graph-owl-core`.
    let (store, _db, _url) = store().await;
    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("declare");

    let mut resolver = RuntimeNamespaces::new();
    for declared in store.namespaces().await.expect("list") {
        resolver
            .register(declared.code, declared.iri)
            .expect("what the registry stored must satisfy the resolver's rules");
    }

    assert_eq!(
        resolve_iri(&format!("{HOSP_IRI}Property"), &resolver),
        Some(Sid::new(namespace::RUNTIME_START, "Property")),
    );
}

#[tokio::test]
async fn redeclaring_the_same_pair_is_idempotent() {
    // Reloading a pack must be safe to repeat; a redeclaration that failed
    // would turn a restart into an outage.
    let (store, _db, _url) = store().await;
    let def = declaration(namespace::RUNTIME_START, HOSP_IRI);

    store.declare(&def).await.expect("first");
    store.declare(&def).await.expect("second is a no-op");

    assert_eq!(store.namespaces().await.expect("list").len(), 1);
}

#[tokio::test]
async fn a_code_is_never_repointed_at_a_different_iri() {
    let (store, _db, _url) = store().await;
    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("first");

    let clash = store
        .declare(&declaration(
            namespace::RUNTIME_START,
            "https://example.org/ns/something-else#",
        ))
        .await;

    assert!(
        matches!(clash, Err(RegistryError::Duplicate { .. })),
        "every flake already stored with this code would change meaning silently, \
         got {clash:?}"
    );
}

#[tokio::test]
async fn one_iri_never_gets_a_second_code() {
    // Two codes for one IRI would make resolution depend on which row was read
    // first — the same IRI resolving differently run to run.
    let (store, _db, _url) = store().await;
    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("first");

    let clash = store
        .declare(&declaration(namespace::RUNTIME_START + 1, HOSP_IRI))
        .await;

    assert!(
        matches!(clash, Err(RegistryError::Duplicate { .. })),
        "got {clash:?}"
    );
}

#[tokio::test]
async fn a_reserved_code_cannot_be_claimed() {
    let (store, _db, _url) = store().await;

    let refused = store
        .declare(&declaration(namespace::DSC, "https://evil.example/ns#"))
        .await;

    assert!(
        matches!(refused, Err(RegistryError::CoreImmutable { .. })),
        "claiming `dsc:` would redefine the catalog's own vocabulary, got {refused:?}"
    );
    assert!(
        store.namespaces().await.expect("list").is_empty(),
        "and nothing lands"
    );
}

#[tokio::test]
async fn allocation_starts_at_the_runtime_boundary_and_is_monotonic() {
    let (store, _db, _url) = store().await;

    assert_eq!(
        store.next_code().await.expect("next"),
        namespace::RUNTIME_START,
        "the first code handed out is the first the binary does not own"
    );

    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("declare");

    assert_eq!(
        store.next_code().await.expect("next"),
        namespace::RUNTIME_START + 1,
        "and the next is past it"
    );
}

#[tokio::test]
async fn an_abandoned_code_is_never_reissued() {
    // The negative that makes `next_code` MAX+1 rather than lowest-gap: a code
    // whose namespace is no longer declared is still carried by every flake
    // written while it was live, so reissuing it would silently repoint that
    // history at a different vocabulary.
    let (store, _db, _url) = store().await;
    store
        .declare(&declaration(namespace::RUNTIME_START, HOSP_IRI))
        .await
        .expect("declare");
    store
        .declare(&declaration(
            namespace::RUNTIME_START + 5,
            "https://example.org/ns/auto#",
        ))
        .await
        .expect("declare a sparse second");

    assert_eq!(
        store.next_code().await.expect("next"),
        namespace::RUNTIME_START + 6,
        "the gap at +1..+4 is not reused"
    );
}
