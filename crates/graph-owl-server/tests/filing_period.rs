//! `gst:FilingPeriod` as a graph subject and `period-summary`, the
//! registered pack query that traverses to it — Plan 107 Slice 1
//! (`plans/107-filing-period.md`), the walking skeleton for period-scoped
//! questions.
//!
//! **What this proves that a hand-built-rows unit test cannot**: the real
//! `period-summary.sparql` text (verbatim, matching `reconcile.rs`'s own
//! precedent) executed over HTTP, against data landed the way a pack
//! actually lands it, actually traverses the `belongsToPeriod` edge to
//! reach every subject in a period — rather than filtering the pre-existing
//! `gst:period` literal directly on each fact, which is the behaviour this
//! slice adds without removing.
//!
//! **The `gst:period` literal is untouched by this slice** (a direct
//! user decision recorded in the plan): every subject below still carries
//! its own `gst:period` literal exactly as `packs/gst/fixtures/*.ttl`
//! already does, and one test below proves a query binding that literal
//! directly — the same shape `pack.toml`'s `GstinTransposition` rule uses
//! — still works, unaffected by the new edge existing alongside it.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use tower::ServiceExt;

/// Binds `{{period}}` as the `FilingPeriod` subject's own IRI, exactly
/// `provision-in-force.sparql`'s own `VALUES ?invoice { <{{invoice}}> }`
/// shape — **not** a bare period string. Found by running the RED test,
/// not assumed: `substitute_pack_query_bindings`'s `is_safe_iri` requires
/// an absolute-IRI scheme (`has_iri_scheme`, `graph-owl-api/src/lib.rs`),
/// by design ("every real binding this system supplies is an absolute
/// IRI... a value with no scheme is a caller's bare literal, not a
/// term"), so a plain `"2020-07"` is rejected with a `400` before the
/// query ever runs — every existing registered query already assumes
/// this. Matching it needs no change to that shared, already-tested
/// mechanism, and is more consistent with "traversing to it" than
/// resolving a literal first would have been.
const PERIOD_SUMMARY: &str = r"
PREFIX gst: <https://graph-owl.dev/packs/gst#>

SELECT ?subject ?type WHERE {
  VALUES ?period { <{{period}}> }
  GRAPH ?g {
    ?subject gst:belongsToPeriod ?period ;
             a ?type .
  }
}
ORDER BY ?subject
";

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: &str,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {}", token("system")))
                .header("content-type", content_type)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    call(app, method, uri, "application/json", body.to_string()).await
}

/// Two `FilingPeriod` instances, two subjects in `2020-07` (a
/// `PurchaseInvoice` and a `Gstr1Filing`, mirroring the real fixtures'
/// mixed subject types for one period), one subject in `2026-07` — the
/// minimum shape that can prove period-scoping actually excludes the
/// other period rather than returning everything. Each subject keeps its
/// own `gst:period` literal alongside the new edge, exactly the additive
/// (not superseding) decision this slice implements.
async fn seed_two_periods_and_three_subjects(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    let (status, _) = json(
        app,
        "POST",
        "/predicates",
        serde_json::json!({"namespace": 1024, "name": "period", "valueType": 1, "many": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "predicate period");

    let (status, _) = json(
        app,
        "POST",
        "/predicates",
        serde_json::json!({"namespace": 1024, "name": "belongsToPeriod", "valueType": 0, "many": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "predicate belongsToPeriod");

    let turtle = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:period-2020-07 rdf:type gst:FilingPeriod ;
            gst:period "2020-07" .
        gst:period-2026-07 rdf:type gst:FilingPeriod ;
            gst:period "2026-07" .

        gst:pr-INV-1099 rdf:type gst:PurchaseInvoice ;
            gst:period "2020-07" ;
            gst:belongsToPeriod gst:period-2020-07 .
        gst:g1filing-INV-1099 rdf:type gst:Gstr1Filing ;
            gst:period "2020-07" ;
            gst:belongsToPeriod gst:period-2020-07 .
        gst:pr-INV-2099 rdf:type gst:PurchaseInvoice ;
            gst:period "2026-07" ;
            gst:belongsToPeriod gst:period-2026-07 .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-filing-period-test&format=turtle",
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import: {body}");

    let (status, body) = json(
        app,
        "POST",
        "/packs/gst/queries",
        serde_json::json!({
            "queries": [{ "name": "period-summary", "query": PERIOD_SUMMARY }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "declare period-summary: {body}");
}

/// The walking skeleton's own acceptance example: `run_pack_query(gst,
/// period-summary, {period: "2020-07"})` returns every subject for that
/// period, reached by traversing `belongsToPeriod` — not by filtering
/// `gst:period` on each fact — and the other period's subject is absent.
#[tokio::test]
async fn period_summary_traverses_the_edge_to_every_subject_in_the_period() {
    let (app, _container, _) = test_app().await;
    seed_two_periods_and_three_subjects(&app).await;

    let (status, body) = json(
        &app,
        "POST",
        "/packs/gst/queries/period-summary/run",
        serde_json::json!({
            "bindings": { "period": "https://graph-owl.dev/packs/gst#period-2020-07" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = body["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 2, "{body}");
    let by_subject: std::collections::BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| {
            (
                row["subject"].as_str().expect("subject"),
                row["type"].as_str().expect("type"),
            )
        })
        .collect();
    assert_eq!(
        by_subject.get("<https://graph-owl.dev/packs/gst#pr-INV-1099>"),
        Some(&"<https://graph-owl.dev/packs/gst#PurchaseInvoice>"),
        "{body}"
    );
    assert_eq!(
        by_subject.get("<https://graph-owl.dev/packs/gst#g1filing-INV-1099>"),
        Some(&"<https://graph-owl.dev/packs/gst#Gstr1Filing>"),
        "{body}"
    );
    assert!(
        !by_subject.contains_key("<https://graph-owl.dev/packs/gst#pr-INV-2099>"),
        "the other period's subject must not leak into this answer: {body}"
    );
}

/// `run_pack_query`'s own established absent-vs-empty convention: a period
/// IRI naming no `FilingPeriod` that exists returns an empty row set, not
/// an error — the same distinction every other registered pack query
/// already makes between "ran and found nothing" and "failed to run".
#[tokio::test]
async fn a_period_with_no_filing_period_subject_returns_empty_not_an_error() {
    let (app, _container, _) = test_app().await;
    seed_two_periods_and_three_subjects(&app).await;

    let (status, body) = json(
        &app,
        "POST",
        "/packs/gst/queries/period-summary/run",
        serde_json::json!({
            "bindings": { "period": "https://graph-owl.dev/packs/gst#period-1999-01" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = body["rows"].as_array().expect("rows array");
    assert!(rows.is_empty(), "{body}");
}

/// The additive decision this slice implements, proved rather than
/// assumed: a query binding `?period` as a literal directly on the fact —
/// the exact shape `pack.toml`'s `GstinTransposition` rule already uses as
/// evidence — still works, unaffected by `belongsToPeriod` existing
/// alongside it on the same subjects.
#[tokio::test]
async fn the_pre_existing_period_literal_still_binds_directly_unaffected_by_the_new_edge() {
    let (app, _container, _) = test_app().await;
    seed_two_periods_and_three_subjects(&app).await;

    let (status, body) = json(
        &app,
        "POST",
        "/sparql",
        serde_json::json!({
            "query": r#"
                PREFIX gst: <https://graph-owl.dev/packs/gst#>
                SELECT ?invoice WHERE {
                  GRAPH ?g {
                    ?invoice a gst:PurchaseInvoice ;
                             gst:period "2020-07" .
                  }
                }
            "#,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = body["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1, "{body}");
    assert_eq!(
        rows[0]["invoice"], "<https://graph-owl.dev/packs/gst#pr-INV-1099>",
        "{body}"
    );
}
