//! Epic 104's console acceptance criterion, at the HTTP surface: "on any
//! cross-vocabulary result the alignment that made it reachable is
//! inspectable — a result that crossed an approximate match must be
//! distinguishable from one that did not, and not by colour alone."
//!
//! `Catalog::alignments_touched`/`SparqlOutcome::alignments_used` are
//! exhaustively covered by `graph_owl_api`'s own unit tests
//! (`cross_vocabulary_alignment_tests`). What those tests cannot see is
//! whether a caller of the real `/sparql` endpoint ever receives the field —
//! `query_outcome_json` hand-builds the response, and Epic 99's own
//! `qlRewrite`/`refusedAxioms` fields were once silently dropped there in
//! exactly this way (`sparql_ql_rewrite.rs`).

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::namespace;
use graph_owl_engine::TripleStore;
use graph_owl_ontology::alignment::{
    Alignment, AlignmentSource, MatchPredicate, alignment_to_flakes,
};
use serde_json::Value;
use tower::ServiceExt;

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
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

/// A query that crosses a curated alignment's direct triple must name it —
/// resolved to source kind, confidence and predicate, not just its raw
/// subject — so a reader can tell this result crossed a vocabulary boundary
/// and trust it exactly as much as that alignment deserves.
#[tokio::test]
async fn a_cross_vocabulary_result_names_the_alignment_that_made_it_reachable() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    let alignment = Alignment::Match {
        left: graph_owl_core::flake::Sid::new(namespace::CUI, "C0004057"),
        right: graph_owl_core::flake::Sid::new(namespace::SNOMED_CT, "387458008"),
        predicate: MatchPredicate::ExactMatch,
        source: AlignmentSource::Curated {
            authority: "UMLS".to_string(),
        },
        confidence: 1.0,
        lossy_reverse: false,
    };
    store
        .assert_flakes(&alignment_to_flakes(&alignment, 1))
        .await
        .expect("seed one curated alignment");

    let body = sparql(
        &app,
        "SELECT ?snomed WHERE { \
            <https://uts.nlm.nih.gov/uts/umls/concept/C0004057> \
                <http://www.w3.org/2004/02/skos/core#exactMatch> ?snomed \
         }",
    )
    .await;

    assert_eq!(body["rows"].as_array().expect("rows").len(), 1, "{body}");

    let used = body["alignmentsUsed"].as_array().expect("array");
    assert_eq!(used.len(), 1, "{body}");
    let entry = &used[0];
    assert_eq!(entry["sourceKind"], "curated", "{entry}");
    assert_eq!(entry["sourceDetail"], "UMLS", "{entry}");
    assert_eq!(entry["confidence"], 1.0, "{entry}");
    assert_eq!(entry["predicate"], "exactMatch", "{entry}");
    assert_eq!(entry["lossyReverse"], false, "{entry}");
    assert!(entry["left"].as_str().is_some(), "{entry}");
    assert!(entry["right"].as_str().is_some(), "{entry}");
}

/// **The negative complement.** An ordinary query touching no alignment
/// predicate at all must report an empty array on the wire, not a missing
/// field or `null` — a client checking `.length` must not have to guard
/// against either.
#[tokio::test]
async fn an_ordinary_query_reports_an_empty_alignments_used_array() {
    let (app, _database, _connection_string) = test_app().await;

    let body = sparql(&app, "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 1").await;

    assert!(
        body["alignmentsUsed"]
            .as_array()
            .is_some_and(std::vec::Vec::is_empty),
        "{body}"
    );
}
