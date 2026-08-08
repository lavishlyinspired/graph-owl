//! Epic 6 Slices D and E, at the HTTP surface.
//!
//! The reasoner itself is exhaustively unit-tested in `graph-owl-reasoning`
//! without a database. What only an end-to-end run can show is the part that
//! is about *storage*: that conclusions land in their own graph, that a re-run
//! replaces rather than accumulates, and that the asserted base is byte-for-byte
//! what it was before the run — which is the guarantee that makes enabling
//! reasoning a reversible decision.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use graph_owl_engine::TripleStore;
use serde_json::Value;
use tower::ServiceExt;

fn dsc(id: &str) -> Sid {
    Sid::dsc(id)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}
fn sub_class_of() -> Sid {
    Sid::new(namespace::RDFS, "subClassOf")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

/// A three-level hierarchy: `payments` is a `PiiTable`, which is a
/// `SensitiveTable`, which is a `GovernedTable`. Depth 3 so the conclusion
/// under test needs two rounds of inference rather than one.
async fn seed_ontology(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    let facts = vec![
        Flake::assert(
            dsc("payments"),
            rdf_type(),
            FlakeValue::Ref(dsc("PiiTable")),
            t,
        ),
        Flake::assert(
            dsc("PiiTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("SensitiveTable")),
            t,
        ),
        Flake::assert(
            dsc("SensitiveTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("GovernedTable")),
            t,
        ),
    ];
    store
        .assert_flakes(&facts)
        .await
        .expect("seed the ontology");
}

async fn run_reasoning(app: &axum::Router) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reasoning/runs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn explain(app: &axum::Router, s: &str, p: &str, o: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/reasoning/explain?s={s}&p={p}&o={o}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// Everything in the default graph, as stored.
async fn default_graph(store: &graph_owl_engine_postgres::PostgresTripleStore) -> Vec<Flake> {
    let mut flakes = store
        .query_pattern(&TriplePattern {
            cx: Some(None),
            ..Default::default()
        })
        .await
        .expect("default graph");
    flakes.sort_by_key(|f| (f.s.to_string(), f.p.to_string(), format!("{:?}", f.o)));
    flakes
}

async fn overlay(store: &graph_owl_engine_postgres::PostgresTripleStore) -> Vec<Flake> {
    store
        .query_pattern(&TriplePattern {
            cx: Some(Some(Sid::dsc("graph:reasoning"))),
            ..Default::default()
        })
        .await
        .expect("reasoning graph")
}

/// **Decision 2's guarantee, and the reason the overlay is a separate graph.**
/// A run must leave the asserted base exactly as it found it. Derivations
/// written beside assertions cannot be taken back, because the next run's
/// wholesale replacement would delete asserted data along with them.
#[tokio::test]
async fn a_run_leaves_the_asserted_base_exactly_as_it_found_it() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let before = default_graph(&store).await;
    let report = run_reasoning(&app).await;
    let after = default_graph(&store).await;

    assert_eq!(before, after, "a run wrote into the default graph");
    assert!(
        report["derived"].as_u64().expect("derived") >= 2,
        "depth 3 implies two types: {report}"
    );
    assert_eq!(
        report["capped"],
        Value::Null,
        "fixpoint, not a cap: {report}"
    );
}

/// And the conclusions are actually *there* — in their own graph, so the
/// assertion above is about separation rather than about a run that did
/// nothing.
#[tokio::test]
async fn conclusions_land_in_the_reasoning_graph() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    run_reasoning(&app).await;

    let derived = overlay(&store).await;
    assert!(
        derived
            .iter()
            .any(|f| f.s == dsc("payments") && f.o == FlakeValue::Ref(dsc("GovernedTable"))),
        "the depth-3 conclusion is missing: {derived:#?}"
    );
    assert!(
        derived
            .iter()
            .all(|f| f.cx == Some(Sid::dsc("graph:reasoning"))),
        "{derived:#?}"
    );
}

/// A scheduled run must converge. Accumulation would grow the overlay without
/// bound and leave conclusions standing after the facts behind them are gone.
#[tokio::test]
async fn a_second_run_replaces_rather_than_accumulating() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let first = run_reasoning(&app).await;
    let after_first = overlay(&store).await.len();
    let second = run_reasoning(&app).await;
    let after_second = overlay(&store).await.len();

    assert_eq!(after_first, after_second, "the overlay grew across runs");
    assert_eq!(first["derived"], second["derived"]);
    assert_eq!(
        second["replaced"], first["derived"],
        "the second run withdrew exactly what the first wrote: {second}"
    );
    assert_eq!(first["replaced"], 0, "nothing to replace yet: {first}");
}

/// **Withdrawing a premise withdraws the conclusion.** This is what makes the
/// overlay derived rather than merely written: a conclusion that outlives its
/// premise is a fact nobody asserted and nothing implies.
#[tokio::test]
async fn retracting_an_axiom_removes_its_conclusions_on_the_next_run() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;
    run_reasoning(&app).await;

    let t = store.next_time().await.expect("a transaction time");
    store
        .retract_flakes(&[Flake::assert(
            dsc("SensitiveTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("GovernedTable")),
            t,
        )])
        .await
        .expect("retract the axiom");
    run_reasoning(&app).await;

    let derived = overlay(&store).await;
    assert!(
        !derived
            .iter()
            .any(|f| f.o == FlakeValue::Ref(dsc("GovernedTable"))),
        "the conclusion outlived its premise: {derived:#?}"
    );
    // And the negative: the conclusion that still holds is still there, so the
    // assertion above is about the retracted axiom rather than about an empty
    // overlay.
    assert!(
        derived
            .iter()
            .any(|f| f.o == FlakeValue::Ref(dsc("SensitiveTable"))),
        "{derived:#?}"
    );
}

/// The recursive explanation, end to end. One level would name
/// `payments type SensitiveTable` as a premise and stop — and why *that* held
/// is the half a reviewer is actually checking.
#[tokio::test]
async fn a_derived_fact_explains_all_the_way_down_to_assertions() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, body) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("GovernedTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "derived", "{body}");
    let chain = &body["chains"][0];
    assert_eq!(chain["rule"], "subClassOf", "{body}");
    let deeper = chain["premises"]
        .as_array()
        .expect("premises")
        .iter()
        .find(|p| p["status"] == "derived")
        .expect("one premise is itself derived");
    assert!(
        deeper["chains"][0]["premises"]
            .as_array()
            .expect("inner premises")
            .iter()
            .all(|p| p["status"] == "asserted"),
        "depth 2 rests on assertions: {body}"
    );
}

#[tokio::test]
async fn an_asserted_fact_explains_as_asserted() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, body) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("PiiTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "asserted", "{body}");
}

/// A fact that is neither stated nor implied has no explanation, and saying so
/// with a `404` is the difference between "nothing supports this" and "this is
/// supported by nothing", which read the same and mean opposite things.
#[tokio::test]
async fn a_fact_that_is_neither_asserted_nor_derived_is_not_found() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, _) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("PublicTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A malformed identifier is the caller's mistake, not a missing fact — and
/// `400` rather than `404` is what tells them which.
#[tokio::test]
async fn an_unparseable_identifier_is_rejected_rather_than_missing() {
    let (app, _database, _) = test_app().await;

    let (status, _) = explain(&app, "not-a-sid", "1:type", "1:x").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `count` tables under a real hierarchy.
///
/// Through the API rather than straight into the store, so lineage goes through
/// the write path that projects it — which is the thing under test.
async fn tables(app: &axum::Router, count: usize) -> Vec<String> {
    async fn create(app: &axum::Router, body: Value) -> Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/assets")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::CREATED);
        json_body(response).await
    }

    let service = create(
        app,
        serde_json::json!({ "kind": "service", "name": "hdfc-core" }),
    )
    .await;
    let database = create(
        app,
        serde_json::json!({ "kind": "database", "name": "retail", "parentId": service["id"] }),
    )
    .await;
    let schema = create(
        app,
        serde_json::json!({ "kind": "schema", "name": "payments", "parentId": database["id"] }),
    )
    .await;

    let mut ids = Vec::new();
    for n in 0..count {
        let table = create(
            app,
            serde_json::json!({
                "kind": "table",
                "name": format!("t{n}"),
                "parentId": schema["id"],
            }),
        )
        .await;
        ids.push(table["id"].as_str().expect("id").to_string());
    }
    ids
}

/// **Demo 4's second claim, end to end.** Classify one table as PII and watch
/// the classification reach everything downstream as a *derived* fact, with a
/// chain that names the edge that carried it.
///
/// This is the test that proves lineage reaches the graph at all: before it,
/// lineage lived only in a relational table and nothing could reason over it.
#[tokio::test]
async fn a_classification_propagates_along_projected_lineage() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    // A chain of three, catalogued through the API so lineage goes through the
    // real write path — projection included.
    let ids = tables(&app, 3).await;

    for pair in ids.windows(2) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/lineage")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"fromAssetId":"{}","toAssetId":"{}"}}"#,
                        pair[0], pair[1]
                    )))
                    .expect("request"),
            )
            .await
            .expect("handled");
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "lineage should assert"
        );
    }

    // `pii` opts in to following `feeds`, and the head of the chain carries it.
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[
            Flake::assert(
                dsc("pii"),
                dsc("propagatesAlong"),
                FlakeValue::Ref(dsc("feeds")),
                t,
            ),
            Flake::assert(
                dsc(&ids[0]),
                dsc("classification"),
                FlakeValue::Ref(dsc("pii")),
                t,
            ),
        ])
        .await
        .expect("classify the source");

    let report = run_reasoning(&app).await;
    assert!(
        report["derived"].as_u64().expect("derived") >= 2,
        "two tables downstream: {report}"
    );

    // The far end of the chain is marked, and it is a *conclusion* — in the
    // reasoning graph, not beside the facts somebody asserted.
    let derived = overlay(&store).await;
    assert!(
        derived.iter().any(|f| f.s == dsc(&ids[2])
            && f.p == dsc("classification")
            && f.o == FlakeValue::Ref(dsc("pii"))),
        "the classification did not reach the end of the chain: {derived:#?}"
    );

    // And it explains itself, naming the edge that carried it.
    let (status, body) = explain(
        &app,
        &dsc(&ids[1]).to_string(),
        &dsc("classification").to_string(),
        &dsc("pii").to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "derived", "{body}");
    assert_eq!(body["chains"][0]["rule"], "classificationFlows", "{body}");
}

/// And the negative that makes the opt-in mean something: a marking nobody
/// declared propagating stays where it was put. Epic 25 made that the default
/// deliberately — a blanket rule turns the estate one colour in a single run.
#[tokio::test]
async fn a_classification_without_an_opt_in_does_not_travel() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[
            Flake::assert(dsc("raw"), dsc("feeds"), FlakeValue::Ref(dsc("curated")), t),
            Flake::assert(
                dsc("raw"),
                dsc("classification"),
                FlakeValue::Ref(dsc("deprecated")),
                t,
            ),
        ])
        .await
        .expect("seed");

    run_reasoning(&app).await;

    let derived = overlay(&store).await;
    assert!(
        !derived.iter().any(|f| f.s == dsc("curated")),
        "a marking nobody opted in travelled anyway: {derived:#?}"
    );
}

/// **Removing a lineage edge withdraws it from the graph too.** A projection
/// that only ever adds leaves the reasoner concluding from an edge the catalog
/// no longer holds — a marking nobody can trace back to anything.
#[tokio::test]
async fn removing_a_lineage_edge_withdraws_its_triple() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    let ids = tables(&app, 2).await;

    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/lineage")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"fromAssetId":"{}","toAssetId":"{}"}}"#,
                    ids[0], ids[1]
                )))
                .expect("request"),
        )
        .await
        .expect("handled");
    let edge = json_body(created).await;

    async fn feeds_in_graph(store: &graph_owl_engine_postgres::PostgresTripleStore) -> Vec<Flake> {
        store
            .query_pattern(&TriplePattern {
                p: Some(dsc("feeds")),
                cx: Some(None),
                ..Default::default()
            })
            .await
            .expect("feeds")
    }

    assert_eq!(
        feeds_in_graph(&store).await.len(),
        1,
        "the edge should have been projected"
    );

    let removed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/lineage/{}", edge["id"].as_str().expect("id")))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);

    assert!(
        feeds_in_graph(&store).await.is_empty(),
        "the triple outlived the edge it mirrored"
    );
}

/// Epic 100, at the HTTP surface — **Phase 1.4 of
/// `plans/EPIC-COMPLETION-PLAN.md`**. `detect_ontology_profiles`/
/// `route_ontology_reasoning`/`force_ontology_reasoning` were correct and
/// tested since this epic shipped, but `POST /reasoning/runs` never called
/// any of them — an ontology outside OWL 2 RL loaded into the RL engine
/// silently produced a "confidently wrong hierarchy" through the real API,
/// exactly the failure this epic exists to prevent.
mod profile_routing {
    use super::*;

    async fn seed_reflexive_property(store: &graph_owl_engine_postgres::PostgresTripleStore) {
        // `owl:ReflexiveProperty` is RL's own simplest forbidden construct
        // (`graph_owl_ontology::profile::detect_rl`'s first check) — no
        // OWL 2 RL rule this reasoner implements even uses it, so this
        // cannot be confused with the reasoner's own vocabulary.
        let t = store.next_time().await.expect("a transaction time");
        store
            .assert_flakes(&[Flake::assert(
                dsc("knows"),
                rdf_type(),
                FlakeValue::Ref(Sid::new(namespace::OWL, "ReflexiveProperty")),
                t,
            )])
            .await
            .expect("seed the reflexive property");
    }

    async fn try_run_reasoning(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        let status = response.status();
        (status, json_body(response).await)
    }

    #[tokio::test]
    async fn an_out_of_rl_ontology_is_refused_and_names_the_profile_routing_prefers_instead() {
        // A bare `owl:ReflexiveProperty` disqualifies RL specifically
        // (`detect_rl`'s own first check) but does not disqualify EL — EL
        // has no rule against it — so decision 5's "RL, then EL, then QL"
        // preference routes to EL rather than refusing outright. Found
        // running this rather than assumed: the first version of this test
        // expected a full `Refused`, and the real response is the more
        // useful `Route(El)` message instead. Still exactly the property
        // this endpoint must have: it must not silently run the RL engine
        // over a TBox RL does not apply to.
        let (app, _database, connection_string) = test_app().await;
        let store = graph(&connection_string).await;
        seed_reflexive_property(&store).await;

        let (status, body) = try_run_reasoning(&app, "/reasoning/runs").await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let detail = body["errors"][0]["detail"].as_str().expect("detail");
        assert!(
            detail.contains("not in the RL profile") && detail.contains("El"),
            "must name that RL does not apply and which profile routing prefers instead: {detail}"
        );
    }

    #[tokio::test]
    async fn an_ontology_outside_every_profile_is_fully_refused() {
        // `owl:maxCardinality 2` on a restriction — the same fixture
        // `graph_owl_api`'s own `an_ontology_outside_every_profile_is_refused_naming_the_axiom`
        // unit test proves is outside all three profiles at once (RL's
        // 0-or-1 limit, EL's blanket cardinality ban, QL's own
        // `ForbiddenConstruct::Cardinality`), so this is the genuine
        // `RoutingDecision::Refused` branch, not `Route(other)`.
        let (app, _database, connection_string) = test_app().await;
        let store = graph(&connection_string).await;
        let t = store.next_time().await.expect("a transaction time");
        store
            .assert_flakes(&[
                Flake::assert(
                    dsc("Person"),
                    sub_class_of(),
                    FlakeValue::Ref(dsc("restriction-1")),
                    t,
                ),
                Flake::assert(
                    dsc("restriction-1"),
                    Sid::new(namespace::OWL, "maxCardinality"),
                    FlakeValue::Int(2),
                    t,
                ),
            ])
            .await
            .expect("seed maxCardinality restriction");

        let (status, body) = try_run_reasoning(&app, "/reasoning/runs").await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let detail = body["errors"][0]["detail"].as_str().expect("detail");
        assert!(detail.starts_with("refused:"), "{detail}");
        assert!(
            detail.contains("1:Person") && detail.contains("0-or-1 limit"),
            "must name the offending axiom, not just refuse: {detail}"
        );
    }

    #[tokio::test]
    async fn get_ontology_profile_answers_without_running_reasoning() {
        let (app, _database, connection_string) = test_app().await;
        let store = graph(&connection_string).await;
        let t = store.next_time().await.expect("a transaction time");
        store
            .assert_flakes(&[
                Flake::assert(
                    dsc("Person"),
                    sub_class_of(),
                    FlakeValue::Ref(dsc("restriction-1")),
                    t,
                ),
                Flake::assert(
                    dsc("restriction-1"),
                    Sid::new(namespace::OWL, "maxCardinality"),
                    FlakeValue::Int(2),
                    t,
                ),
            ])
            .await
            .expect("seed maxCardinality restriction");

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/ontology/profile")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;

        assert_eq!(body["rl"]["member"], false, "{body}");
        assert_eq!(body["el"]["member"], false, "{body}");
        assert_eq!(body["ql"]["member"], false, "{body}");
        assert_eq!(body["routing"]["outcome"], "refused", "{body}");
        assert_eq!(body["routing"]["firstOffendingAxiom"], "1:Person", "{body}");
    }

    #[tokio::test]
    async fn an_rl_safe_ontology_is_unaffected() {
        // The negative control: the plain `subClassOf` hierarchy every
        // other test in this file seeds is textbook RL, so routing must
        // not refuse it — proving the check is real, not a blanket refusal.
        let (app, _database, connection_string) = test_app().await;
        let store = graph(&connection_string).await;
        seed_ontology(&store).await;

        let (status, body) = try_run_reasoning(&app, "/reasoning/runs").await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["partial"], false, "{body}");
        assert_eq!(body["ignoredAxioms"].as_array().expect("array").len(), 0);
    }

    #[tokio::test]
    async fn force_true_runs_anyway_and_marks_the_result_partial() {
        let (app, _database, connection_string) = test_app().await;
        let store = graph(&connection_string).await;
        seed_reflexive_property(&store).await;

        let (status, body) = try_run_reasoning(&app, "/reasoning/runs?force=true").await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["partial"], true, "{body}");
        let ignored = body["ignoredAxioms"].as_array().expect("array");
        assert_eq!(ignored.len(), 1, "{body}");
        assert_eq!(ignored[0]["subject"], "1:knows", "{body}");
    }
}
