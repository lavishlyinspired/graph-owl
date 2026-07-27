//! Epic 4 Slice H: predicates definable at runtime.

use graph_owl_engine::{PredicateDef, PredicateRegistry, RegistryError};
use graph_owl_engine_postgres::PostgresTripleStore;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

async fn store() -> (PostgresTripleStore, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .start()
        .await
        .expect("postgres should start");
    let connection_string = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        container.get_host().await.expect("host"),
        container.get_host_port_ipv4(5432).await.expect("port")
    );
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, container)
}

fn custom(name: &str) -> PredicateDef {
    PredicateDef {
        namespace: 1024,
        name: name.to_string(),
        value_type: 1,
        many: false,
        core: false,
    }
}

#[tokio::test]
async fn a_predicate_can_be_defined_and_read_back() {
    let (store, _container) = store().await;
    store.define(&custom("rbiCircular")).await.expect("define");

    let found = store
        .lookup(1024, "rbiCircular")
        .await
        .expect("lookup")
        .expect("the definition should be there");
    assert_eq!(found.name, "rbiCircular");
    assert_eq!(found.value_type, 1);
    assert!(!found.many);
    assert!(!found.core, "a runtime definition is never core");
}

#[tokio::test]
async fn an_unknown_predicate_looks_up_to_nothing() {
    let (store, _container) = store().await;
    assert!(
        store
            .lookup(1024, "neverDefined")
            .await
            .expect("lookup")
            .is_none(),
        "absence is an answer, not an error"
    );
}

#[tokio::test]
async fn defining_the_same_predicate_twice_is_a_conflict() {
    let (store, _container) = store().await;
    store.define(&custom("rbiCircular")).await.expect("first");

    let error = store
        .define(&custom("rbiCircular"))
        .await
        .expect_err("the second must be refused");
    assert!(
        matches!(error, RegistryError::Duplicate { .. }),
        "got {error:?}"
    );
}

/// **The one that matters.** Redefining `dsc:fqn` from a string to a reference
/// would not migrate the flakes already written against it — it would make
/// every one of them unreadable, silently.
#[tokio::test]
async fn a_core_predicate_cannot_be_redefined() {
    let (store, _container) = store().await;

    let error = store
        .define(&PredicateDef {
            namespace: 1,
            name: "fqn".to_string(),
            value_type: 0,
            many: true,
            core: false,
        })
        .await
        .expect_err("core predicates are immutable");

    assert!(
        matches!(error, RegistryError::CoreImmutable { .. }),
        "the error must say *why*, not merely that it exists: {error:?}"
    );

    // And the definition is untouched.
    let fqn = store.lookup(1, "fqn").await.expect("lookup").expect("core");
    assert_eq!(fqn.value_type, 1, "still a string");
    assert!(!fqn.many, "still single-valued");
}

/// A runtime caller that could mark its own predicate core would make it
/// permanent by accident, with nothing in this API able to remove it.
#[tokio::test]
async fn a_runtime_definition_cannot_claim_to_be_core() {
    let (store, _container) = store().await;
    store
        .define(&PredicateDef {
            core: true,
            ..custom("pretender")
        })
        .await
        .expect("define");

    let found = store
        .lookup(1024, "pretender")
        .await
        .expect("lookup")
        .expect("defined");
    assert!(!found.core, "the flag is not settable from outside");
}

/// The whole vocabulary every stored flake depends on must be present, or the
/// registry cannot answer questions about the graph that already exists.
#[tokio::test]
async fn the_core_vocabulary_is_seeded_by_migration() {
    let (store, _container) = store().await;

    for name in [
        "type",
        "name",
        "fqn",
        "description",
        "version",
        "createdAt",
        "updatedAt",
        "updatedBy",
        "deleted",
        "parentTable",
        "parentSchema",
        "fromEntity",
        "toEntity",
        "relType",
    ] {
        let found = store
            .lookup(1, name)
            .await
            .expect("lookup")
            .unwrap_or_else(|| panic!("dsc:{name} is not seeded"));
        assert!(found.core, "dsc:{name} must be marked core");
    }

    // rdf:type lives in the standards namespace and is what marks a reified
    // relationship as one.
    let rdf_type = store
        .lookup(256, "type")
        .await
        .expect("lookup")
        .expect("rdf:type");
    assert!(rdf_type.core);
}

/// Cardinality is a property of the predicate. `dsc:tag` is many-valued and
/// `dsc:name` is not, and the registry is where that is recorded once rather
/// than remembered by every writer.
#[tokio::test]
async fn cardinality_is_recorded_per_predicate() {
    let (store, _container) = store().await;

    let name = store
        .lookup(1, "name")
        .await
        .expect("lookup")
        .expect("seeded");
    assert!(!name.many, "an asset has one name");

    let tag = store
        .lookup(1, "tag")
        .await
        .expect("lookup")
        .expect("seeded");
    assert!(tag.many, "an asset has any number of tags");
}

#[tokio::test]
async fn listing_can_be_scoped_to_one_namespace() {
    let (store, _container) = store().await;
    store.define(&custom("rbiCircular")).await.expect("define");

    let all = store.list(None).await.expect("list");
    let runtime = store.list(Some(1024)).await.expect("list");
    let core = store.list(Some(1)).await.expect("list");

    assert!(all.len() > core.len(), "the whole registry is the largest");
    assert_eq!(runtime.len(), 1);
    assert_eq!(runtime[0].name, "rbiCircular");
    assert!(core.iter().all(|d| d.namespace == 1));
}

/// An organisation extending the vocabulary is the point of the slice.
#[tokio::test]
async fn an_organisation_can_extend_the_vocabulary_without_a_release() {
    let (store, _container) = store().await;

    for (name, value_type, many) in [
        ("rbiCircular", 1, true),
        ("dataResidency", 1, false),
        ("retentionYears", 3, false),
    ] {
        store
            .define(&PredicateDef {
                namespace: 1024,
                name: name.to_string(),
                value_type,
                many,
                core: false,
            })
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    let defined = store.list(Some(1024)).await.expect("list");
    assert_eq!(defined.len(), 3);
    assert!(
        defined.iter().any(|d| d.name == "rbiCircular" && d.many),
        "cardinality survives the round trip: {defined:?}"
    );
}
