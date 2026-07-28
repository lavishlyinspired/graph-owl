//! Epic 4 Slice H: predicates definable at runtime, and enforced on write.

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_engine::{EngineError, PredicateDef, PredicateRegistry, RegistryError, TripleStore};
use graph_owl_engine_postgres::PostgresTripleStore;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

async fn store() -> (PostgresTripleStore, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, database, connection_string)
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
    let (store, _container, _url) = store().await;
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
    let (store, _container, _url) = store().await;
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
    let (store, _container, _url) = store().await;
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
    let (store, _container, _url) = store().await;

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
    let (store, _container, _url) = store().await;
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
    let (store, _container, _url) = store().await;

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
    let (store, _container, _url) = store().await;

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
    let (store, _container, _url) = store().await;
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
    let (store, _container, _url) = store().await;

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

fn fact(predicate: Sid) -> Flake {
    Flake::assert(
        Sid::dsc("table-upi-transactions"),
        predicate,
        FlakeValue::String("upi_transactions".into()),
        1,
    )
}

async fn flake_count(store: &PostgresTripleStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count")
}

/// **The gap this closes.** A registry nothing consults is documentation, not
/// a constraint: the vocabulary is only real if a flake outside it cannot be
/// written.
#[tokio::test]
async fn asserting_an_undefined_predicate_is_refused_and_names_it() {
    let (store, _container, _url) = store().await;

    let error = store
        .assert_flakes(&[fact(Sid::dsc("rbiCircular"))])
        .await
        .expect_err("an undefined predicate must be refused");

    assert!(
        matches!(&error, EngineError::UnregisteredPredicate { name, .. } if name == "rbiCircular"),
        "got {error:?}"
    );
    assert_eq!(
        flake_count(&store).await,
        0,
        "a refused batch writes nothing at all"
    );
}

/// One undefined predicate poisons the whole batch, including the flakes
/// beside it that were perfectly valid — the batch is one statement in one
/// transaction, and a half-projected entity reconciles to something no
/// version of that entity ever was.
#[tokio::test]
async fn one_undefined_predicate_refuses_the_whole_batch() {
    let (store, _container, _url) = store().await;

    store
        .assert_flakes(&[
            fact(Sid::dsc("name")),
            fact(Sid::dsc("fqn")),
            fact(Sid::dsc("rbiCircular")),
        ])
        .await
        .expect_err("must be refused");

    assert_eq!(flake_count(&store).await, 0, "including the valid two");
}

/// The point of the whole slice: define the vocabulary, then use it, with no
/// release in between. Also the cache-invalidation test — a cache that
/// answered from before the `define` would refuse a predicate that now exists.
#[tokio::test]
async fn a_predicate_defined_at_runtime_becomes_assertable() {
    let (store, _container, _url) = store().await;

    // Refused first, so the acceptance below is the definition's doing and not
    // an enforcement that never ran.
    store
        .assert_flakes(&[fact(Sid::new(1024, "rbiCircular"))])
        .await
        .expect_err("not yet defined");

    store.define(&custom("rbiCircular")).await.expect("define");

    store
        .assert_flakes(&[fact(Sid::new(1024, "rbiCircular"))])
        .await
        .expect("defined predicates are assertable");
    assert_eq!(flake_count(&store).await, 1);
}

/// graph-owl runs as more than one process against one database. A predicate
/// another instance defined is defined, and a cache that treated its own miss
/// as absence would refuse a write the registry permits — a failure that
/// appears only under the deployment topology nobody tests locally.
#[tokio::test]
async fn a_predicate_defined_by_another_instance_is_accepted() {
    let (store, _container, url) = store().await;
    let other_instance = PostgresTripleStore::connect(&url)
        .await
        .expect("a second instance of the same database");

    // Warm this instance's cache on the seeded vocabulary, so the definition
    // below lands strictly after it was populated.
    store
        .assert_flakes(&[fact(Sid::dsc("name"))])
        .await
        .expect("seeded predicate");

    other_instance
        .define(&custom("dataResidency"))
        .await
        .expect("define elsewhere");

    store
        .assert_flakes(&[fact(Sid::new(1024, "dataResidency"))])
        .await
        .expect("a miss must re-read the registry before calling it absent");
}

/// An uninitialized predicate is not an undefined one. Namespace 0 will never
/// be in the registry, so the registry check would happily claim `0:name is
/// not defined` — true, useless, and pointing at the wrong repair. The caller
/// needs to hear that the field was never set.
#[tokio::test]
async fn an_uninitialized_predicate_reports_the_unset_namespace_not_the_registry() {
    let (store, _container, _url) = store().await;

    let error = store
        .assert_flakes(&[fact(Sid::new(
            graph_owl_core::flake::namespace::UNSET,
            "name",
        ))])
        .await
        .expect_err("must be refused");

    assert!(
        matches!(
            error,
            EngineError::UnsetNamespace {
                position: "predicate"
            }
        ),
        "got {error:?}"
    );
}

/// Retraction is deliberately **not** gated.
///
/// A retraction only ever withdraws a fact that is already in the graph.
/// Refusing one because its predicate is no longer welcome would strand that
/// fact permanently — it could never be written again, and never taken back
/// either. The gate belongs on the way in.
#[tokio::test]
async fn a_retraction_is_not_gated_by_the_registry() {
    let (store, _container, _url) = store().await;

    store
        .retract_flakes(&[fact(Sid::dsc("rbiCircular"))])
        .await
        .expect("withdrawing is always allowed");
}
