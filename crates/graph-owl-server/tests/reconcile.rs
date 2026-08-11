//! `POST /packs/{pack}/finding-rules` and `POST /packs/{pack}/reconcile` —
//! Epic 105 P5b (`plans/105b-native-reconcile-engine.md`), the platform
//! doc's P5 finding runtime, native at last.
//!
//! **The whole point of this file**: `findings_from_rows`'s unit tests in
//! `graph-owl-api` prove the filter logic against hand-built rows; this
//! proves the part only HTTP + a real database can — that a rule registered
//! over the wire, evaluated against facts landed the same way a pack
//! actually lands them (`POST /graph/import/rdf`, not a hand-seeded flake),
//! produces a finding a reviewer can see through `GET /findings`. The real
//! `missing-in-gstr2b.sparql` query text is used verbatim, not a stand-in —
//! this is the parity proof the plan's own acceptance criteria ask for.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture, test_app, test_catalog, token};
use graph_owl_core::{Principal, PrincipalKind};
use tower::ServiceExt;

const MISSING_IN_GSTR2B: &str = r"
PREFIX gst: <https://graph-owl.dev/packs/gst#>

SELECT ?invoice ?number ?gstin ?taxAmount
WHERE {
  GRAPH ?register {
    ?invoice a gst:PurchaseInvoice ;
             gst:supplierGstin ?gstin ;
             gst:invoiceNumber ?number ;
             gst:taxAmount ?taxAmount .
  }
  OPTIONAL {
    GRAPH ?authority {
      ?filed a gst:Gstr2bInvoice ;
             gst:supplierGstin ?gstin ;
             gst:invoiceNumber ?number .
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

/// The real pipeline a pack goes through, minimized to one invoice claimed
/// in the purchase register and never filed in GSTR-2B — INV-1003's own
/// shape from `packs/gst/fixtures/purchase-register.ttl`.
async fn seed_gst_vocabulary_and_one_unmatched_invoice(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    for name in ["supplierGstin", "invoiceNumber", "taxAmount"] {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 1, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }

    let turtle = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:p-INV-1003 rdf:type gst:PurchaseInvoice ;
            gst:supplierGstin "29AACCG0527D1Z8" ;
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

async fn register_missing_in_gstr2b_rule(app: &axum::Router) {
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
}

#[tokio::test]
async fn a_registered_rule_evaluated_against_real_imported_facts_produces_a_finding() {
    let (app, _db, _url) = test_app().await;
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    let (status, outcome) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["pack"], "gst");
    assert_eq!(outcome["evaluated"], 1, "{outcome}");
    assert_eq!(outcome["found"], 1, "{outcome}");
    assert_eq!(outcome["opened"], 1, "{outcome}");
    assert_eq!(outcome["alreadyOpen"], 0, "{outcome}");

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
    assert_eq!(findings[0]["label"], "gst:PotentialMismatch");
    assert!(
        findings[0]["subject"]
            .as_str()
            .unwrap()
            .contains("p-INV-1003"),
        "the subject must be the bare IRI, not <wrapped>: {findings:?}"
    );
    assert_eq!(findings[0]["governedBy"], "gst:Section16-2-aa");
    let evidence = findings[0]["evidence"].as_array().expect("evidence array");
    assert!(
        evidence
            .iter()
            .any(|e| e["predicate"] == "gst:invoiceNumber" && e["value"] == "INV-1003"),
        "evidence value must be the bare literal, not \"quoted\": {evidence:?}"
    );
}

/// **Epic 105 P10's `reconcile()` tool, against the real adapter.** Proves
/// two things the HTTP route's own tests above never distinguish, because
/// every call there resolves to `Principal::system()` (open mode, always
/// admin): `CatalogContext::reconcile` really does call through to
/// `Catalog::reconcile_pack` for a real admin, *and* really does refuse a
/// non-admin — the property the trait doc comment states but no earlier
/// test in this codebase exercises, since nothing else drives
/// `CatalogContext` with a hand-built non-admin `Principal`.
#[tokio::test]
async fn reconcile_admits_an_admin_and_refuses_a_non_admin_through_the_real_adapter() {
    let (catalog, _db, _url) = test_catalog().await;
    let app = graph_owl_server::app(catalog.clone());
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    let admin = Principal::system();
    let admin_reads = graph_owl_mcp::catalog::CatalogContext::new(catalog.clone(), admin.clone());
    let outcome = graph_owl_mcp::ContextSource::reconcile(&admin_reads, &admin.id, "gst")
        .await
        .expect("no source error")
        .expect("an admin may reconcile");
    assert_eq!(outcome.pack, "gst");
    assert_eq!(outcome.evaluated, 1, "{outcome:?}");
    assert_eq!(outcome.found, 1, "{outcome:?}");
    assert_eq!(outcome.opened, 1, "{outcome:?}");

    let contractor = Principal {
        id: "contractor".to_string(),
        name: "contractor".to_string(),
        kind: PrincipalKind::User,
        roles: Vec::new(),
        is_admin: false,
    };
    let contractor_reads = graph_owl_mcp::catalog::CatalogContext::new(catalog, contractor.clone());
    let refused = graph_owl_mcp::ContextSource::reconcile(&contractor_reads, &contractor.id, "gst")
        .await
        .expect("no source error");
    assert!(refused.is_none(), "a non-admin must be refused: {refused:?}");
}

/// A second run over the same unmatched invoice must not double the queue —
/// the same idempotence `record_findings` already gives `POST /findings`,
/// now exercised through the whole reconcile path rather than a hand-posted
/// batch.
#[tokio::test]
async fn reconciling_twice_does_not_duplicate_a_still_pending_finding() {
    let (app, _db, _url) = test_app().await;
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    let (_, first) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(first["opened"], 1, "{first}");

    let (_, second) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(second["found"], 1, "the rule still matches: {second}");
    assert_eq!(second["opened"], 0, "nothing new: {second}");
    assert_eq!(
        second["alreadyOpen"], 1,
        "the same finding, recognized: {second}"
    );

    let (_, findings) = call(
        &app,
        "GET",
        "/findings?pack=gst",
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(
        findings.as_array().expect("array").len(),
        1,
        "two runs, one finding: {findings:?}"
    );
}

/// A pack with rules registered but nothing matching them is a legitimate,
/// clean reconciliation — not an error, and not a no-op the caller cannot
/// distinguish from a run that never happened.
#[tokio::test]
async fn a_pack_whose_invoice_was_actually_filed_produces_no_finding() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = json(
        &app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for name in ["supplierGstin", "invoiceNumber", "taxAmount"] {
        let (status, _) = json(
            &app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 1, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // Both sides present under the same GSTIN + invoice number — a real
    // match, matching `Gstr2bInvoice` rather than only `PurchaseInvoice`.
    let turtle = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:p-INV-1001 rdf:type gst:PurchaseInvoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-1001" ;
            gst:taxAmount "18000.00" .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-purchase-register&format=turtle",
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let filed = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:2b-INV-1001 rdf:type gst:Gstr2bInvoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
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

    register_missing_in_gstr2b_rule(&app).await;

    let (status, outcome) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["evaluated"], 1, "{outcome}");
    assert_eq!(
        outcome["found"], 0,
        "the invoice matches, so no finding: {outcome}"
    );
}

#[tokio::test]
async fn reconcile_is_admin_gated() {
    // `authorization_fixture` rather than `test_app`, and the difference is
    // load-bearing: `test_app` runs every caller as an admin (see
    // `graph_import.rs`'s own `a_non_admin_cannot_import`, which records the
    // same lesson). `asha` is the real provisioned non-admin every other
    // admin-gate test in this crate uses.
    let (app, _db, _catalog) = authorization_fixture().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/gst/reconcile")
                .header("authorization", format!("Bearer {}", token("asha")))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "refused as not-found rather than forbidden, matching every other admin route"
    );
}
