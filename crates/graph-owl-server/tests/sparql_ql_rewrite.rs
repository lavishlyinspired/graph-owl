//! Epic 99 Slices B/C, at the HTTP surface — **Phase 1.2 of
//! `plans/EPIC-COMPLETION-PLAN.md`**.
//!
//! `Catalog::sparql` always attempts an OWL 2 QL rewrite for a `SELECT`
//! query, populates `SparqlOutcome::ql_rewrite`/`refused_axioms`, and both
//! fields are exhaustively covered by `graph_owl_api`'s own unit tests. What
//! those tests cannot see is whether a caller of the real HTTP endpoint ever
//! receives either field: `query_outcome_json` hand-builds the `/sparql`
//! response and, until this fix, silently dropped both — two of this
//! epic's own checked acceptance criteria ("the rewritten query is
//! retrievable", "an axiom outside QL is reported, not silently dropped")
//! were true only inside the Rust process, never on the wire.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::TripleStore;
use serde_json::Value;
use tower::ServiceExt;

fn dsc(id: &str) -> Sid {
    Sid::dsc(id)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}
fn subclass_of() -> Sid {
    Sid::new(namespace::RDFS, "subClassOf")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

/// A real, catalog-known asset — not a synthetic `dsc:` subject.
/// **`scoped_facts` (`graph_owl_api::Catalog`) narrows every SPARQL result
/// to subjects that resolve through `list_assets_under_fqn`, unlike SHACL
/// validation's `run_validation_as`, which reads the graph directly with no
/// such narrowing.** A raw `Sid::dsc("orders")` asserted straight into the
/// graph — the first version of this fixture — is invisible to `/sparql` no
/// matter what it types itself as, silently producing `factsScanned: 0`
/// with no error at all. Found running this test against a real server, not
/// assumed: the row an in-memory-fake unit test (`graph_owl_api`'s own
/// `a_subclass_query_answers_without_materializing_anything`) gets past this
/// by creating the instance through `catalog.upsert_asset` first and using
/// its returned UUID's `entity_sid` — the same thing this helper does.
async fn create_asset(app: &axum::Router, name: &str) -> uuid::Uuid {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/assets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "kind": "service", "name": name }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    body["id"].as_str().expect("id").parse().expect("a UUID")
}

/// `Table`/`View` are both stated as `rdfs:subClassOf DataAsset` — plain
/// `TBox` classes, never queried as row-binding subjects, so they need no
/// backing asset. `instance` does, since it is what `?x` binds to.
async fn seed_subclass_hierarchy(
    store: &graph_owl_engine_postgres::PostgresTripleStore,
    instance: Sid,
) {
    let t = store.next_time().await.expect("a transaction time");
    let facts = vec![
        Flake::assert(
            dsc("Table"),
            subclass_of(),
            FlakeValue::Ref(dsc("DataAsset")),
            t,
        ),
        Flake::assert(
            dsc("View"),
            subclass_of(),
            FlakeValue::Ref(dsc("DataAsset")),
            t,
        ),
        Flake::assert(instance, rdf_type(), FlakeValue::Ref(dsc("Table")), t),
    ];
    store
        .assert_flakes(&facts)
        .await
        .expect("seed the TBox and one instance");
}

async fn sparql(app: &axum::Router, query: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sparql")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "query": query }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// A query naming `DataAsset` — a class with known subclasses but no
/// instances of its own — must rewrite to a `UNION` over `Table`/`View` to
/// answer at all. The rewrite happening is proven by the row landing
/// (`orders` is typed only `Table`, never `DataAsset` directly); this test's
/// own point is that `qlRewrite` also reaches the wire, naming the branch.
#[tokio::test]
async fn the_rewritten_query_and_its_branches_are_retrievable_over_http() {
    let (app, _database, connection_string) = test_app().await;
    let orders_id = create_asset(&app, "orders").await;
    let orders_sid = graph_owl_core::projection::entity_sid(orders_id);

    let store = graph(&connection_string).await;
    seed_subclass_hierarchy(&store, orders_sid.clone()).await;

    let body = sparql(
        &app,
        "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> \
         SELECT ?x WHERE { ?x rdf:type <https://graph-owl.dev/ns/catalog#DataAsset> }",
    )
    .await;

    // Row bindings render as full IRIs (SPARQL's own convention for a
    // result term), unlike `qlRewrite.branches[].class` below — that field
    // is hand-built in `query_outcome_json` from `Sid::to_string()`'s
    // compact `namespace:id` form, not the evaluator's own row rendering.
    // Both are correct; they are simply two different serializations.
    let orders_iri = format!("<https://graph-owl.dev/ns/catalog#{orders_id}>");
    assert!(
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .any(|row| row["x"] == orders_iri),
        "the rewrite itself must still work: {body}"
    );

    let rewrite = &body["qlRewrite"];
    assert!(
        !rewrite.is_null(),
        "a query that only answers via the QL rewrite must report one: {body}"
    );
    assert!(
        rewrite["expandedQuery"]
            .as_str()
            .is_some_and(|q| !q.is_empty()),
        "{rewrite}"
    );
    let branches = rewrite["branches"].as_array().expect("branches");
    assert!(
        branches
            .iter()
            .any(|b| b["class"] == dsc("Table").to_string()),
        "the Table branch must be named: {rewrite}"
    );
    assert_eq!(
        body["refusedAxioms"].as_array().expect("array").len(),
        0,
        "nothing here is out of profile: {body}"
    );
}
