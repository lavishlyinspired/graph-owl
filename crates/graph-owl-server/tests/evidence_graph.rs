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
use common::{test_app, test_catalog, token};
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

/// Plan 114: a pack subject has no `AssetKind` — no relational row, no
/// catalog kind — which is exactly why the console's graph canvas had drawn
/// every evidence-graph node in one uniform grey. This proves the *virtual*
/// fix over the real HTTP layer: the fixture's own turtle already asserts
/// `rdf:type` for both nodes (`seed_invoice_with_a_real_supplier_node`), so
/// nothing new is seeded here — the type comes back because it was already
/// in the graph, not because this test adds anything to persist it.
#[tokio::test]
async fn a_node_s_own_rdf_type_reaches_the_evidence_graph_as_its_semantic_type() {
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
    let id = findings.as_array().expect("array")[0]["id"]
        .as_str()
        .expect("finding id");

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

    let invoice_node = nodes
        .iter()
        .find(|n| n["id"] == "p-INV-1003")
        .expect("invoice node present");
    assert_eq!(
        invoice_node["semanticType"],
        serde_json::json!("PurchaseInvoice"),
        "{invoice_node:?}"
    );

    let supplier_node = nodes
        .iter()
        .find(|n| n["id"] == "supplier-29AACCG0527D1Z8")
        .expect("supplier node present — reached only by traversal, not the flat evidence list");
    assert_eq!(
        supplier_node["semanticType"],
        serde_json::json!("Supplier"),
        "{supplier_node:?}"
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

/// `packs/gst/queries/gstin-transposition.sparql`'s own text, verbatim —
/// matching `MISSING_IN_GSTR2B`'s precedent above.
const GSTIN_TRANSPOSITION: &str = r"
PREFIX gst: <https://graph-owl.dev/packs/gst#>

SELECT ?purchase ?number ?claimedGstin ?filedGstin ?period
WHERE {
  GRAPH ?register {
    ?purchase a gst:PurchaseInvoice ;
              gst:issuedBy ?supplier ;
              gst:invoiceNumber ?number ;
              gst:period        ?period .
    ?supplier gst:supplierGstin ?claimedGstin .
  }
  GRAPH ?authority {
    ?filed a gst:Gstr2bInvoice ;
           gst:issuedBy ?filedSupplier ;
           gst:invoiceNumber ?number ;
           gst:period        ?period .
    ?filedSupplier gst:supplierGstin ?filedGstin .
  }
  FILTER (?claimedGstin != ?filedGstin)
}
ORDER BY ?number
";

/// Epic 105 P7's near-miss half (`plans/105g-evidence-provenance-and-near-miss.md`
/// Slice 2) — the shape `packs/gst/fixtures/{purchase-register,gstr2b}.ttl`
/// plant for `GstinTransposition`: the claimed and filed sides are two
/// distinct `gst:Supplier` subjects, deliberately never linked by an edge.
async fn seed_transposition_scenario(app: &axum::Router) {
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
        ("period", 1),
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

    let purchase_register = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:supplier-27AABCU9603R1MZ rdf:type gst:Supplier ;
            gst:supplierGstin "27AABCU9603R1MZ" .

        gst:pr-INV-1004 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy      gst:supplier-27AABCU9603R1MZ ;
            gst:invoiceNumber "INV-1004" ;
            gst:period        "2026-07" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-purchase-register&format=turtle",
        "text/turtle",
        purchase_register.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import: {body}");

    let gstr2b = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier ;
            gst:supplierGstin "27AABCU9603R1ZM" .

        gst:2b-INV-1004 rdf:type gst:Gstr2bInvoice ;
            gst:issuedBy      gst:supplier-27AABCU9603R1ZM ;
            gst:invoiceNumber "INV-1004" ;
            gst:period        "2026-07" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-gstr2b&format=turtle",
        "text/turtle",
        gstr2b.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import: {body}");
}

/// `pack` is a parameter because Plan 111 Slice F needs two findings from two
/// *differently configured* packs in this one binary — one whose manifest
/// declares blocking and one whose does not. Registering both under `gst`
/// would force the two tests to disagree about one process-wide setting, and
/// a test that races another test is a test that lies intermittently.
async fn register_and_run_gstin_transposition_for(
    app: &axum::Router,
    pack: &str,
) -> serde_json::Value {
    let (status, body) = json(
        app,
        "POST",
        &format!("/packs/{pack}/finding-rules"),
        serde_json::json!({
            "rules": [{
                "label": "gst:GstinTransposition",
                "summary": "Same invoice number and period on both sides under near-identical GSTINs",
                "governedBy": "gst:MatchingPolicy",
                "query": GSTIN_TRANSPOSITION,
                "subjectVar": "purchase",
                "evidence": [
                    {"predicate": "gst:invoiceNumber", "var": "number"},
                    {"predicate": "gst:supplierGstin", "var": "claimedGstin"},
                    {"predicate": "gst:supplierGstin", "var": "filedGstin"},
                    {"predicate": "gst:period", "var": "period"},
                ],
                "similarity": {
                    "strategy": "ngram",
                    "n": 3,
                    "left": "claimedGstin",
                    "right": "filedGstin",
                    "atLeast": 0.40,
                    "atMost": 0.999,
                    "resolveBy": "https://graph-owl.dev/packs/gst#supplierGstin",
                },
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register rule: {body}");

    let (status, outcome) = json(
        app,
        "POST",
        &format!("/packs/{pack}/reconcile"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["opened"], 1, "{outcome}");
    outcome
}

async fn register_and_run_gstin_transposition(app: &axum::Router) -> serde_json::Value {
    register_and_run_gstin_transposition_for(app, "gst").await
}

#[tokio::test]
async fn a_gstin_transposition_s_evidence_graph_names_the_unlinked_second_supplier() {
    let (app, _db, _url) = test_app().await;
    seed_transposition_scenario(&app).await;
    register_and_run_gstin_transposition(&app).await;

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
        nodes.iter().all(|n| n["id"] != "supplier-27AABCU9603R1ZM"),
        "the filed-side supplier must not be reachable by the walk — the \
         entire premise of this finding is that no edge connects the two \
         suppliers: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|n| n["id"] == "supplier-27AABCU9603R1MZ"),
        "the claimed-side supplier is reached normally, by its own \
         issuedBy edge: {nodes:?}"
    );

    let near_miss = &graph["nearMiss"];
    assert_eq!(
        near_miss["id"], "supplier-27AABCU9603R1ZM",
        "the second candidate, resolved by its filed GSTIN value rather \
         than by traversal: {graph}"
    );
    assert_eq!(
        near_miss["sources"],
        serde_json::json!(["gst-gstr2b"]),
        "{near_miss:?}"
    );
}

#[tokio::test]
async fn a_finding_whose_rule_has_no_similarity_band_reports_no_near_miss_field() {
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

    let (status, graph) = call(
        &app,
        "GET",
        &format!("/findings/{id}/evidence-graph"),
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    assert!(
        graph["nearMiss"].is_null(),
        "a rule with no similarity band has no near-miss candidate to \
         report: {graph}"
    );
}

/// **Epic 105 P10's `find_evidence()` tool, against the real adapter.**
/// Every test above proves the HTTP route this wraps; this proves
/// `CatalogContext::find_evidence` really does call through to
/// `Catalog::finding_evidence_graph` and really does assemble provenance
/// the same way — the same real-adapter proof `mcp_stdio.rs`'s
/// `traverse_reaches_the_real_catalog_through_the_real_adapter` gives
/// `traverse`, reusing this file's own fixture rather than duplicating it.
#[tokio::test]
async fn find_evidence_reaches_the_supplier_node_through_the_real_adapter() {
    let (catalog, _db, _url) = test_catalog().await;
    let app = graph_owl_server::app(catalog.clone());
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
    let finding_id = uuid::Uuid::parse_str(id).expect("finding id is a uuid");

    let principal = graph_owl_core::Principal::system();
    let reads = graph_owl_mcp::catalog::CatalogContext::new(catalog, principal.clone());
    let context = graph_owl_mcp::ContextSource::find_evidence(&reads, &principal.id, finding_id, 2)
        .await
        .expect("no source error")
        .expect("the finding exists");

    assert!(
        context
            .nodes
            .iter()
            .any(|n| n.id == "supplier-29AACCG0527D1Z8"),
        "{:?}",
        context.nodes
    );
    assert!(
        context
            .edges
            .iter()
            .any(|e| e.from == "p-INV-1003" && e.to == "supplier-29AACCG0527D1Z8"),
        "{:?}",
        context.edges
    );
    let invoice_node = context
        .nodes
        .iter()
        .find(|n| n.id == "p-INV-1003")
        .expect("invoice node present");
    assert_eq!(
        invoice_node.sources,
        vec!["gst-purchase-register".to_string()]
    );
}

/// **The near-miss path, through the real adapter.** The test above never
/// exercises `near_miss_node`'s `Some` branch at all — `PotentialMismatch`
/// has no `[findings.similarity]` band, so `near_miss` is always `None`
/// for it. `GstinTransposition`'s whole premise is a missing edge, which is
/// exactly the case that needs a real near-miss candidate to prove
/// `CatalogContext::find_evidence` folds one in correctly, and excludes it
/// once it *is* reachable (not just when it happens to already be `None`).
#[tokio::test]
async fn find_evidence_names_the_near_miss_through_the_real_adapter() {
    let (catalog, _db, _url) = test_catalog().await;
    let app = graph_owl_server::app(catalog.clone());
    seed_transposition_scenario(&app).await;
    register_and_run_gstin_transposition(&app).await;

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    let id = findings[0]["id"].as_str().expect("finding id");
    let finding_id = uuid::Uuid::parse_str(id).expect("finding id is a uuid");

    let principal = graph_owl_core::Principal::system();
    let reads = graph_owl_mcp::catalog::CatalogContext::new(catalog, principal.clone());
    let context = graph_owl_mcp::ContextSource::find_evidence(&reads, &principal.id, finding_id, 2)
        .await
        .expect("no source error")
        .expect("the finding exists");

    assert!(
        context
            .nodes
            .iter()
            .all(|n| n.id != "supplier-27AABCU9603R1ZM"),
        "the filed-side supplier must not be reachable by the walk: {:?}",
        context.nodes
    );
    let near_miss = context
        .near_miss
        .expect("a near-miss candidate for this finding");
    assert_eq!(near_miss.id, "supplier-27AABCU9603R1ZM");
    assert_eq!(near_miss.sources, vec!["gst-gstr2b".to_string()]);
}

/// A pack directory this binary points at **once**, containing two packs: the
/// real shipped `gst` manifest and a `quietpack` that declares no matching at
/// all.
///
/// **Set once, never per test.** `GRAPH_OWL_PACKS_DIR` is process-wide, and
/// two tests writing different values to it race — a test that races another
/// test is a test that lies intermittently, and this one did: the first
/// mutation run of Slice F aborted with "cargo test failed in an unmutated
/// tree" because the two tests below each set it. Both configurations now
/// live side by side and a test selects between them by *which pack it
/// registers its rule under*, which is a per-request choice rather than a
/// process one.
///
/// The `gst` half is a symlink-free copy of the manifest that actually ships,
/// so these run against the real declaration rather than a fixture's second
/// opinion of one.
fn packs_dir() -> &'static std::path::Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let base = std::env::temp_dir().join(format!("graph-owl-p111f-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(base.join("gst")).expect("gst directory");
        std::fs::create_dir_all(base.join("quietpack")).expect("quiet directory");
        std::fs::copy(
            shipped.join("gst").join("pack.toml"),
            base.join("gst").join("pack.toml"),
        )
        .expect("copy the shipped manifest");
        std::fs::write(
            base.join("quietpack").join("pack.toml"),
            "[pack]\nid = \"quietpack\"\nnamespace = \"https://graph-owl.dev/packs/gst#\"\n\
             prefix = \"gst\"\ndescription = \"declares no matching at all\"\n",
        )
        .expect("quiet manifest");
        // SAFETY: written exactly once, before any handler reads it, and never
        // again for the life of this process.
        unsafe { std::env::set_var("GRAPH_OWL_PACKS_DIR", &base) };
        base
    })
    .as_path()
}

/// Plan 111 Slice F — **the pack's blocking strategies reach the reviewer.**
///
/// The transposition scenario has a purchase invoice and a GSTR-2B invoice
/// carrying the same invoice number under near-identical GSTINs. The
/// evidence walk cannot reach the 2B invoice — no edge joins them, which is
/// the rule's entire premise — so before this the panel showed a finding with
/// no way to see the record it is really about.
///
/// **The candidate says which strategy agreed.** "An n-gram key collided" and
/// "a normalized key collided" are different strengths of evidence and a
/// reviewer's next move differs; one word for both would hide that.
#[tokio::test]
async fn a_findings_evidence_carries_the_packs_own_blocking_candidates() {
    packs_dir();
    let (app, _db, _url) = test_app().await;
    seed_transposition_scenario(&app).await;
    register_and_run_gstin_transposition_for(&app, "gst").await;

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

    let candidates = graph["candidates"].as_array().expect("candidates array");
    let twin = candidates
        .iter()
        .find(|c| c["id"] == "2b-INV-1004")
        .unwrap_or_else(|| panic!("the 2B invoice sharing this invoice number: {graph}"));
    assert_eq!(
        twin["by"].as_array().expect("by").as_slice(),
        [serde_json::Value::String("ngram".to_string())],
        "which strategy agreed is part of the answer: {twin}",
    );
    assert!(
        twin["sources"]
            .as_array()
            .expect("sources")
            .iter()
            .any(|s| s == "gst-gstr2b"),
        "a candidate carries its provenance like every other node here: {twin}",
    );

    // **The finding's own subject is never its own candidate**, and neither
    // is anything the walk already drew — a node listed twice tells a
    // reviewer there is a second record when there is one.
    let walked: Vec<&str> = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|n| n["id"].as_str().expect("id"))
        .collect();
    for candidate in candidates {
        let id = candidate["id"].as_str().expect("id");
        assert!(!walked.contains(&id), "`{id}` is already drawn: {graph}");
        assert_ne!(id, "pr-INV-1004", "a record is not its own near miss");
    }
}

/// **A deployment whose pack declares no blocking gets an empty list, not a
/// failure.** The candidates section is additive to a reviewer's evidence
/// panel; a pack that says nothing about matching must not take the panel
/// down with it.
#[tokio::test]
async fn a_pack_that_declares_no_blocking_still_serves_the_evidence_graph() {
    packs_dir();
    let (app, _db, _url) = test_app().await;
    seed_transposition_scenario(&app).await;
    // The rule is registered under the pack whose manifest declares no
    // matching, so the finding this produces carries that pack — which is how
    // a per-request choice replaces a process-wide one.
    register_and_run_gstin_transposition_for(&app, "quietpack").await;

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=quietpack",
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
    assert!(
        graph["candidates"]
            .as_array()
            .expect("candidates")
            .is_empty(),
        "{graph}"
    );
    // The rest of the panel is unaffected — the point of degrading rather
    // than failing.
    assert!(
        !graph["nodes"].as_array().expect("nodes").is_empty(),
        "{graph}"
    );
}

/// A purchase-register invoice pointing at a real `gst:Supplier` node that
/// also carries `gst:supplierName` — the literal `packs/gst/pack.toml`'s
/// `[console.labels]` declares for the `Supplier` class (Plan 121 Slice 1).
/// A separate seed from `seed_invoice_with_a_real_supplier_node`, which
/// deliberately has no name, so that test keeps proving the `null`
/// no-declared-label path rather than silently gaining one.
async fn seed_invoice_with_a_named_supplier(app: &axum::Router) {
    // `declaredBy: "pack:gst"` — the same shape `loader.py` sends
    // (`{"iri": manifest.namespace, "declaredBy": f"pack:{manifest.id}"}`),
    // not the default-to-caller-identity a bare `{"iri": ...}` falls back to.
    // Label resolution reads this back to find which pack's `[console.labels]`
    // owns a namespace code, so a test that skips it would prove nothing
    // about the real pack-install path.
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#", "declaredBy": "pack:gst"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    for (name, value_type) in [
        ("supplierGstin", 1),
        ("supplierName", 1),
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
            gst:supplierGstin "29AACCG0527D1Z8" ;
            gst:supplierName "Nimbus Freight Logistics" .

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

/// Plan 121 Slice 1 — a node the pack declares a `[console.labels]` predicate
/// for shows that literal, not its bare id, through the real HTTP layer.
/// Uses `packs_dir()`'s copy of the real, shipped `packs/gst/pack.toml`
/// deliberately — a synthetic manifest in this test would prove the
/// mechanism reads *some* config, not that it reads the one this deployment
/// actually ships.
#[tokio::test]
async fn a_node_with_a_declared_label_predicate_shows_its_literal_not_its_bare_id() {
    packs_dir();
    let (app, _db, _url) = test_app().await;
    seed_invoice_with_a_named_supplier(&app).await;
    register_and_run_missing_in_gstr2b(&app).await;

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    let id = findings.as_array().expect("array")[0]["id"]
        .as_str()
        .expect("finding id");

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
    let supplier = nodes
        .iter()
        .find(|n| n["id"] == "supplier-29AACCG0527D1Z8")
        .unwrap_or_else(|| panic!("supplier node present: {nodes:?}"));
    assert_eq!(
        supplier["label"],
        serde_json::json!("Nimbus Freight Logistics"),
        "a Supplier node must show the literal packs/gst/pack.toml's \
         [console.labels] declares for its class: {supplier:?}"
    );

    // The negative case: the invoice node's own class (`PurchaseInvoice`) has
    // no `[console.labels]` entry at all, so it must degrade to `null` rather
    // than fail the whole picture or fabricate a label from nothing.
    let invoice = nodes
        .iter()
        .find(|n| n["id"] == "p-INV-1003")
        .unwrap_or_else(|| panic!("invoice node present: {nodes:?}"));
    assert_eq!(
        invoice["label"],
        serde_json::Value::Null,
        "a class with no declared label predicate must degrade to null, not \
         invent one: {invoice:?}"
    );
}
