//! Plan 123 Slice H — the six checks nothing else in `packs/gst` performs.
//!
//! Each runs the pack's **real** query text against facts imported the way a
//! pack actually lands them. A rule that passes against a hand-shaped fixture
//! and fails against the pack's own SPARQL has proved nothing about the pack.
//!
//! Every rule gets a positive **and** a negative: this project's own mutation
//! record says every survivor so far has been a missing negative, because the
//! mutated code still produced the right answer for the positive input.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use tower::ServiceExt;

fn query(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs/gst/queries")
            .join(format!("{name}.sparql")),
    )
    .unwrap_or_else(|e| panic!("read packs/gst/queries/{name}.sparql: {e}"))
}

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
                .expect("request builds"),
        )
        .await
        .expect("handled");
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

async fn setup(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace");

    for name in [
        "supplierGstin",
        "invoiceNumber",
        "taxAmount",
        "period",
        "noteType",
        "originalInvoiceNumber",
        "imsStatus",
        "reverseCharge",
        "claimDeadline",
        "exemptTurnover",
        "totalTurnover",
    ] {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 1, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }
    for name in ["recordedIn", "reflectedIn", "issuedBy", "belongsToPeriod"] {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 0, "many": true}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }
}

async fn import(app: &axum::Router, source: &str, turtle: &str) {
    let (status, body) = call(
        app,
        "POST",
        &format!("/graph/import/rdf?source={source}&format=turtle"),
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import {source}: {body}");
    assert_eq!(
        body["rejected"].as_array().map(Vec::len).unwrap_or(0),
        0,
        "import {source} rejected subjects: {body}"
    );
}

async fn run(app: &axum::Router, name: &str) -> Vec<serde_json::Value> {
    let (status, body) = json(
        app,
        "POST",
        "/sparql",
        serde_json::json!({ "query": query(name) }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{name}: {body}");
    body["rows"].as_array().cloned().unwrap_or_default()
}

fn value(row: &serde_json::Value, var: &str) -> String {
    row[var]
        .as_str()
        .unwrap_or_else(|| panic!("no binding {var} in {row}"))
        .trim_matches(|c| c == '"' || c == '<' || c == '>')
        .to_string()
}

/// The supplier declaration every books fixture prepends.
///
/// **It must land in the same named graph as the invoices**, because every
/// query in this pack joins `?invoice gst:issuedBy ?supplier` and
/// `?supplier gst:supplierGstin ?gstin` inside one `GRAPH ?register { }`
/// block — and `rows_to_turtle` emits both in a single document, so that is
/// what real data looks like. Importing the supplier separately puts it in
/// its own graph and every one of these rules silently matches nothing.
const SUPPLIER_TRIPLES: &str = r#"
    gst:supplier-A rdf:type gst:Supplier ; gst:supplierGstin "29AACCG0527D1Z8" .
"#;

/// One books document: prefixes, the supplier, then the caller's triples.
fn books(triples: &str) -> String {
    format!(
        "@prefix gst: <https://graph-owl.dev/packs/gst#> .\n\
         @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
         {SUPPLIER_TRIPLES}\n{triples}"
    )
}

// ---------------------------------------------------------------- duplicates

#[tokio::test]
async fn one_invoice_entered_twice_is_reported_once_not_twice() {
    // Reported once: `?a != ?b` alone would surface each pair in both orders,
    // and a reviewer would chase the same duplicate twice.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ; gst:taxAmount "18000" .
        gst:books-2 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ; gst:taxAmount "18000" .
        "#,
        ),
    )
    .await;

    let rows = run(&app, "duplicate-claim").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "number"), "INV-9");
}

#[tokio::test]
async fn two_different_invoices_from_one_supplier_are_not_duplicates() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ; gst:taxAmount "18000" .
        gst:books-2 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-10" ; gst:taxAmount "18000" .
        "#,
        ),
    )
    .await;

    assert!(run(&app, "duplicate-claim").await.is_empty());
}

// ------------------------------------------------------------------ IMS

#[tokio::test]
async fn an_unactioned_ims_record_is_reported_before_it_is_deemed_accepted() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:imsStatus "Pending" .
        "#,
        ),
    )
    .await;

    let rows = run(&app, "ims-not-actioned").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
}

#[tokio::test]
async fn an_accepted_or_rejected_ims_record_is_not_reported() {
    // The negative that matters: a rule reporting *every* IMS record would
    // pass the positive above and bury a preparer in resolved items.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:imsStatus "Accepted" .
        gst:books-2 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-10" ;
            gst:taxAmount "9000" ; gst:imsStatus "Rejected" .
        "#,
        ),
    )
    .await;

    assert!(run(&app, "ims-not-actioned").await.is_empty());
}

// ------------------------------------------------------------ reverse charge

#[tokio::test]
async fn a_reverse_charge_invoice_the_supplier_also_declared_is_a_contradiction() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:reverseCharge "Y" .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;
    import(
        &app,
        "gst-2b",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:filed-1 rdf:type gst:Gstr2bInvoice ; gst:invoiceNumber "INV-9" .
        gst:canonical-1 gst:reflectedIn gst:filed-1 .
        "#,
    )
    .await;

    let rows = run(&app, "reverse-charge-claimed-as-forward").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
}

#[tokio::test]
async fn a_reverse_charge_invoice_absent_from_every_2b_is_ordinary_not_a_finding() {
    // Under RCM nothing is expected to arrive in a 2B, so absence is the
    // normal case. Reporting it would flag every RCM invoice a firm holds.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:reverseCharge "Y" .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;

    assert!(
        run(&app, "reverse-charge-claimed-as-forward")
            .await
            .is_empty()
    );
}

// ------------------------------------------------------------------ s.16(4)

#[tokio::test]
async fn unclaimed_credit_is_reported_with_the_deadline_the_period_declares() {
    // The deadline comes from the graph, never from the query — s.16(4)'s date
    // moves with the annual return's filing date.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-periods",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:period-2026-03 rdf:type gst:FilingPeriod ;
            gst:period "2026-03" ; gst:claimDeadline "2026-11-30" .
        "#,
    )
    .await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:belongsToPeriod gst:period-2026-03 .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;

    let rows = run(&app, "itc-time-bar-approaching").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "deadline"), "2026-11-30");
}

#[tokio::test]
async fn credit_already_reflected_in_a_2b_is_not_reported_as_time_barred() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-periods",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:period-2026-03 rdf:type gst:FilingPeriod ;
            gst:period "2026-03" ; gst:claimDeadline "2026-11-30" .
        "#,
    )
    .await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ;
            gst:taxAmount "18000" ; gst:belongsToPeriod gst:period-2026-03 .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;
    import(
        &app,
        "gst-2b",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:filed-1 rdf:type gst:Gstr2bInvoice ; gst:invoiceNumber "INV-9" .
        gst:canonical-1 gst:reflectedIn gst:filed-1 .
        "#,
    )
    .await;

    assert!(run(&app, "itc-time-bar-approaching").await.is_empty());
}

// ------------------------------------------------------------- Rule 42/43

#[tokio::test]
async fn a_period_declaring_exempt_turnover_is_reported_as_owing_a_reversal() {
    // The only rule in this pack whose subject is a period, because the
    // reversal is computed on the period's turnover split rather than on any
    // one invoice.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-periods",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:period-2026-03 rdf:type gst:FilingPeriod ;
            gst:period "2026-03" ;
            gst:exemptTurnover "250000" ; gst:totalTurnover "1000000" .
        "#,
    )
    .await;

    let rows = run(&app, "proportionate-reversal-due").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "exemptTurnover"), "250000");
}

#[tokio::test]
async fn a_period_with_no_exempt_turnover_owes_no_proportionate_reversal() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-periods",
        r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
        gst:period-2026-03 rdf:type gst:FilingPeriod ;
            gst:period "2026-03" ;
            gst:exemptTurnover "0" ; gst:totalTurnover "1000000" .
        "#,
    )
    .await;

    assert!(run(&app, "proportionate-reversal-due").await.is_empty());
}

// -------------------------------------------------------------- s.34 notes

#[tokio::test]
async fn a_credit_note_the_portal_does_not_carry_is_reported() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "CN-1" ;
            gst:taxAmount "-4500" ; gst:noteType "Credit" ;
            gst:originalInvoiceNumber "INV-9" .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;

    let rows = run(&app, "credit-note-not-in-portal").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "originalInvoice"), "INV-9");
}

#[tokio::test]
async fn an_ordinary_invoice_with_no_note_type_is_not_reported_as_a_note() {
    // The absence of `noteType` means an ordinary invoice. A rule binding it
    // optionally would report the entire register as unmatched credit notes.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(
        &app,
        "gst-books",
        &books(
            r#"gst:books-1 rdf:type gst:PurchaseInvoice ;
            gst:issuedBy gst:supplier-A ; gst:invoiceNumber "INV-9" ; gst:taxAmount "18000" .
        gst:canonical-1 gst:recordedIn gst:books-1 .
        "#,
        ),
    )
    .await;

    assert!(run(&app, "credit-note-not-in-portal").await.is_empty());
}
