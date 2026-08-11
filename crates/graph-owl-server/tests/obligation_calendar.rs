//! `GET /packs/{pack}/obligations` — Epic 105 P8's first real slice
//! (`plans/105h-obligation-calendar.md`).
//!
//! **What this proves that `graph-owl-api`'s own `obligations_from_rows_tests`
//! cannot**: those hand-build rows; this runs the real `payment-overdue.sparql`
//! query text (verbatim, matching `reconcile.rs`'s own precedent) against data
//! landed the way a pack actually lands it, and proves an unpaid invoice's due
//! date is computed and reachable over HTTP.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, test_catalog, token};
use graph_owl_core::Principal;
use tower::ServiceExt;

const PAYMENT_OVERDUE: &str = r"
PREFIX gst: <https://graph-owl.dev/packs/gst#>

SELECT ?purchase ?number ?gstin ?purchasedAt ?paidAt
WHERE {
  GRAPH ?register {
    ?invoice a gst:PurchaseInvoice ;
             gst:issuedBy ?supplier ;
             gst:invoiceNumber ?number .
    ?supplier gst:supplierGstin ?gstin .
  }
  GRAPH ?events {
    ?purchase a gst:PurchaseEvent ;
              gst:onInvoice ?invoice ;
              gst:atTime    ?purchasedAt .
  }
  OPTIONAL {
    GRAPH ?paidIn {
      ?payment a gst:PaymentEvent ;
               gst:onInvoice ?invoice ;
               gst:atTime    ?paidAt .
    }
  }
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

/// One purchase, unpaid — `packs/gst/fixtures`' own INV-1003 shape, minus
/// the payment event.
async fn seed_one_unpaid_purchase(app: &axum::Router) {
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
        ("issuedBy", 0),
        ("onInvoice", 0),
        ("atTime", 1),
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

    let register = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier ;
            gst:supplierGstin "27AABCU9603R1ZM" .

        gst:p-INV-1003 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-27AABCU9603R1ZM ;
            gst:invoiceNumber "INV-1003" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-purchase-register&format=turtle",
        "text/turtle",
        register.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import register: {body}");

    let events = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:purchase-INV-1003 rdf:type gst:PurchaseEvent ;
            gst:onInvoice gst:p-INV-1003 ;
            gst:atTime "2026-01-01" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=gst-events&format=turtle",
        "text/turtle",
        events.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import events: {body}");
}

async fn register_payment_overdue_rule(app: &axum::Router) {
    let (status, body) = json(
        app,
        "POST",
        "/packs/gst/finding-rules",
        serde_json::json!({
            "rules": [{
                "label": "gst:PaymentOverdue",
                "summary": "Credit taken on an invoice not paid within 180 days of its date",
                "governedBy": "gst:Section16-2-d",
                "query": PAYMENT_OVERDUE,
                "subjectVar": "purchase",
                "evidence": [
                    {"predicate": "gst:invoiceNumber", "var": "number"},
                    {"predicate": "gst:atTime", "var": "purchasedAt"},
                ],
                "span": {
                    "from": "purchasedAt",
                    "to": "paidAt",
                    "exceedsDays": 180,
                    "whenMissing": "elapsed",
                    "asOf": "2026-08-01",
                },
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register rule: {body}");
}

#[tokio::test]
async fn an_unpaid_purchase_appears_with_its_computed_due_date() {
    let (app, _db, _url) = test_app().await;
    seed_one_unpaid_purchase(&app).await;
    register_payment_overdue_rule(&app).await;

    let (status, obligations) = call(
        &app,
        "GET",
        "/packs/gst/obligations",
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{obligations}");

    let obligations = obligations.as_array().expect("array");
    assert_eq!(obligations.len(), 1, "{obligations:?}");
    // A full IRI, matching Finding.subject's own established convention —
    // `bare_term` strips SPARQL term wrapping (`<>`/`""`), not the
    // namespace, the same way `findings_from_rows` leaves it.
    assert_eq!(
        obligations[0]["subject"],
        "https://graph-owl.dev/packs/gst#purchase-INV-1003"
    );
    assert_eq!(obligations[0]["label"], "gst:PaymentOverdue");
    assert_eq!(obligations[0]["anchor"], "2026-01-01");
    assert_eq!(
        obligations[0]["due"], "2026-06-30",
        "180 days after 2026-01-01: {obligations:?}"
    );
    let days_remaining = obligations[0]["daysRemaining"]
        .as_i64()
        .expect("daysRemaining is a number");
    assert!(
        days_remaining < 0,
        "as of the rule's own as_of (2026-08-01), this is already overdue: {days_remaining}"
    );
}

#[tokio::test]
async fn a_pack_with_no_span_configured_rules_reports_an_empty_calendar() {
    let (app, _db, _url) = test_app().await;

    let (status, obligations) = call(
        &app,
        "GET",
        "/packs/gst/obligations",
        "application/json",
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{obligations}");
    assert_eq!(obligations.as_array().expect("array").len(), 0);
}

/// **Epic 105 P10's `calculate_risk()` tool, against the real adapter.**
/// `CatalogContext::calculate_risk` really does call the pre-existing
/// `Catalog::calculate_risk`, which narrows `Catalog::obligation_calendar`
/// (the same computation `GET /packs/{pack}/obligations` above already
/// proves) to one subject — not the unit-level `Fixture` double in
/// `graph-owl-mcp`'s own tests, which never touches the real adapter.
#[tokio::test]
async fn calculate_risk_reports_one_subject_s_real_days_remaining() {
    let (catalog, _db, _url) = test_catalog().await;
    let app = graph_owl_server::app(catalog.clone());
    seed_one_unpaid_purchase(&app).await;
    register_payment_overdue_rule(&app).await;

    let principal = Principal::system();
    let reads = graph_owl_mcp::catalog::CatalogContext::new(catalog, principal.clone());

    let risk = graph_owl_mcp::ContextSource::calculate_risk(
        &reads,
        &principal.id,
        "gst",
        "https://graph-owl.dev/packs/gst#purchase-INV-1003",
    )
    .await
    .expect("no source error");

    assert_eq!(risk.len(), 1, "{risk:?}");
    assert_eq!(risk[0].label, "gst:PaymentOverdue");
    assert!(
        risk[0].days_remaining < 0,
        "as of the rule's own as_of, this is already overdue: {:?}",
        risk[0]
    );

    // A subject not seeded here gets a real, empty answer — proving the
    // filter, not a coincidence of there being only one obligation open.
    let unrelated = graph_owl_mcp::ContextSource::calculate_risk(
        &reads,
        &principal.id,
        "gst",
        "https://graph-owl.dev/packs/gst#no-such-purchase",
    )
    .await
    .expect("no source error");
    assert!(unrelated.is_empty(), "{unrelated:?}");
}
