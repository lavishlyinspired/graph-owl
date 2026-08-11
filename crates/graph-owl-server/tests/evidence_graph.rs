//! `GET /findings/{id}/evidence-graph` — Epic 105 P7, the traversal half
//! (`plans/105e-evidence-chain-walk.md`).
//!
//! **What this proves that `graph-owl-api`'s own `finding_evidence_graph_tests`
//! cannot**: those use a fake `TraversalEngine` that echoes back whatever
//! `Subgraph` the test hands it — they prove the orchestration (subject
//! resolution, error mapping) but not that a real finding's subject actually
//! reaches a `gst:Supplier` node over a real Postgres-backed traversal. This
//! runs the real pipeline end to end: `POST /graph/import/rdf` lands a
//! `gst:issuedBy` edge exactly as `packs/gst`'s own connector does
//! (`plans/105c-gst-causal-graph.md` Slice 1), the real
//! `missing-in-gstr2b.sparql` query text (verbatim, matching `reconcile.rs`'s
//! own precedent) produces the finding, and the evidence graph is asked for
//! the Supplier its `issuedBy` edge points at — a node the finding's flat
//! `evidence` list never names, because the rule only bound `?gstin`, not
//! `?supplier` itself.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use tower::ServiceExt;

const MISSING_IN_GSTR2B: &str = r"
PREFIX gst: <https://graph-owl.dev/packs/gst#>

SELECT ?invoice ?number ?gstin ?taxAmount
WHERE {
  GRAPH ?register {
    ?invoice a gst:PurchaseInvoice ;
             gst:issuedBy ?supplier ;
             gst:invoiceNumber ?number ;
             gst:taxAmount ?taxAmount .
    ?supplier gst:supplierGstin ?gstin .
  }
  OPTIONAL {
    GRAPH ?authority {
      ?filed a gst:Gstr2bInvoice ;
             gst:issuedBy ?filedSupplier ;
             gst:invoiceNumber ?number .
      ?filedSupplier gst:supplierGstin ?gstin .
    }
  }
  FILTER (!BOUND(?filed))
}
ORDER BY ?number
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

/// A purchase-register invoice pointing at a real `gst:Supplier` node via
/// `issuedBy`, never filed in GSTR-2B — the shape `packs/gst`'s own connector
/// emits since Epic 105c Slice 1, not a flattened stand-in.
async fn seed_invoice_with_a_real_supplier_node(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    for (name, value_type) in [
        ("supplierGstin", 1),
        ("invoiceNumber", 1),
        ("taxAmount", 1),
        ("issuedBy", 0),
    ] {
        let (status, body) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": value_type, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}: {body}");
    }

    let turtle = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:supplier-29AACCG0527D1Z8 rdf:type gst:Supplier ;
            gst:supplierGstin "29AACCG0527D1Z8" .

        gst:p-INV-1003 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-29AACCG0527D1Z8 ;
            gst:invoiceNumber "INV-1003" ;
            gst:taxAmount "45000.00" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-purchase-register&format=turtle",
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import: {body}");
}

async fn register_and_run_missing_in_gstr2b(app: &axum::Router) -> serde_json::Value {
    let (status, body) = json(
        app,
        "POST",
        "/packs/gst/finding-rules",
        serde_json::json!({
            "rules": [{
                "label": "gst:PotentialMismatch",
                "summary": "An invoice claimed in the purchase register that the supplier never filed",
                "governedBy": "gst:Section16-2-aa",
                "query": MISSING_IN_GSTR2B,
                "subjectVar": "invoice",
                "evidence": [
                    {"predicate": "gst:supplierGstin", "var": "gstin"},
                    {"predicate": "gst:invoiceNumber", "var": "number"},
                    {"predicate": "gst:taxAmount", "var": "taxAmount"},
                ],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register rule: {body}");

    let (status, outcome) =
        json(app, "POST", "/packs/gst/reconcile", serde_json::Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["opened"], 1, "{outcome}");
    outcome
}

#[tokio::test]
async fn a_finding_s_evidence_graph_reaches_the_supplier_node_its_issuedby_edge_points_at() {
    let (app, _db, _url) = test_app().await;
    seed_invoice_with_a_real_supplier_node(&app).await;
    register_and_run_missing_in_gstr2b(&app).await;

    let (status, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let findings = findings.as_array().expect("array");
    assert_eq!(findings.len(), 1, "{findings:?}");
    let id = findings[0]["id"].as_str().expect("finding id");

    // The flat evidence list only ever bound `?gstin`, `?number`, `?taxAmount`
    // — never `?supplier` itself, so this is not restating something already
    // on the finding.
    assert!(
        findings[0]["evidence"]
            .as_array()
            .expect("evidence")
            .iter()
            .all(|e| e["predicate"] != "gst:issuedBy"),
        "the rule's evidence bindings must not already name the supplier, or \
         this test would not distinguish traversal from the flat list: {:?}",
        findings[0]["evidence"]
    );

    let (status, graph) = call(
        &app,
        "GET",
        &format!("/findings/{id}/evidence-graph"),
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");

    let nodes = graph["nodes"].as_array().expect("nodes array");
    assert!(
        nodes.iter().any(|n| n["id"] == "supplier-29AACCG0527D1Z8"),
        "the invoice's own supplier must be reachable by traversal, not just \
         by the GSTIN string the rule happened to bind: {nodes:?}"
    );

    let edges = graph["edges"].as_array().expect("edges array");
    assert!(
        edges
            .iter()
            .any(|e| { e["from"] == "p-INV-1003" && e["to"] == "supplier-29AACCG0527D1Z8" }),
        "the issuedBy edge itself must be in the walk, not just both endpoints \
         independently: {edges:?}"
    );

    // Epic 105 P7's provenance half (`plans/105g-...`) — the invoice was
    // asserted by exactly one document.
    let invoice_node = nodes
        .iter()
        .find(|n| n["id"] == "p-INV-1003")
        .expect("invoice node present");
    assert_eq!(
        invoice_node["sources"],
        serde_json::json!(["gst-purchase-register"]),
        "{invoice_node:?}"
    );
}

/// The two-source case `plans/105g-evidence-provenance-and-near-miss.md`
/// names explicitly: a `gst:Supplier` referenced from both the purchase
/// register and GSTR-2B must report both, over a real Postgres-backed
/// `query_pattern` — `graph-owl-api`'s own `node_sources_tests` prove the
/// dedup logic against a fake; this proves the real flake table actually
/// carries two distinct `cx` values for one subject once two documents
/// land, and that both survive the round trip through the HTTP layer.
#[tokio::test]
async fn a_supplier_claimed_by_both_sides_reports_both_sources() {
    let (app, _db, _url) = test_app().await;
    seed_invoice_with_a_real_supplier_node(&app).await;

    // Matches `packs/gst/fixtures/gstr2b.ttl`'s own convention exactly: the
    // supplier's own `rdf:type` fact is redeclared on the GSTR-2B side too
    // (idempotent — re-asserting an identical flake is a no-op), not just
    // referenced as the object of `issuedBy`. Without this the supplier has
    // no subject-position flakes of its own from this document at all, and
    // `node_sources` — which asks "what document said something *about*
    // this entity", not "what document merely pointed at it" — correctly
    // would not count it as a source. Real GST data does this for exactly
    // this reason.
    let filed = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:supplier-29AACCG0527D1Z8 rdf:type gst:Supplier .

        gst:2b-INV-1001 rdf:type gst:Gstr2bInvoice ;
            gst:issuedBy gst:supplier-29AACCG0527D1Z8 ;
            gst:invoiceNumber "INV-1001" .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-gstr2b&format=turtle",
        "text/turtle",
        filed.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    register_and_run_missing_in_gstr2b(&app).await;

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    let id = findings[0]["id"].as_str().expect("finding id");

    let (status, graph) = call(
        &app,
        "GET",
        &format!("/findings/{id}/evidence-graph"),
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");

    let nodes = graph["nodes"].as_array().expect("nodes array");
    let supplier_node = nodes
        .iter()
        .find(|n| n["id"] == "supplier-29AACCG0527D1Z8")
        .expect("supplier node present");
    let mut sources: Vec<&str> = supplier_node["sources"]
        .as_array()
        .expect("sources array")
        .iter()
        .map(|v| v.as_str().expect("source is a string"))
        .collect();
    sources.sort_unstable();
    assert_eq!(
        sources,
        vec!["gst-gstr2b", "gst-purchase-register"],
        "{supplier_node:?}"
    );
}

#[tokio::test]
async fn a_finding_that_does_not_exist_is_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = call(
        &app,
        "GET",
        &format!("/findings/{}/evidence-graph", uuid::Uuid::new_v4()),
        "application/json",
        String::new(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unsupported_direction_is_a_400_naming_the_field() {
    let (app, _db, _url) = test_app().await;
    seed_invoice_with_a_real_supplier_node(&app).await;
    register_and_run_missing_in_gstr2b(&app).await;

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    let id = findings[0]["id"].as_str().expect("finding id");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/findings/{id}/evidence-graph?direction=sideways"),
        "application/json",
        String::new(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("direction"), "{body}");
}
