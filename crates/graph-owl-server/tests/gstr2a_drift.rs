//! Plan 123 Slice C — GSTR-2A alongside GSTR-2B, and drift between them.
//!
//! **The plan's own stated RED, verbatim**: "a supplier filing in month N+1
//! for a month-N invoice produces a `FiledLateInGstr2a` finding against month
//! N and **no** duplicate claim in month N+1."
//!
//! Both rules are run as their real `packs/gst` query text, not a stand-in,
//! against facts imported the way a pack actually lands them — the same parity
//! discipline `reconcile.rs` established. A rule that passes against a
//! hand-shaped fixture and fails against the pack's own SPARQL has proved
//! nothing about the pack.

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

async fn declare_vocabulary(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    for name in [
        "supplierGstin",
        "invoiceNumber",
        "taxAmount",
        "period",
        "pulledOn",
        "reverseCharge",
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
    for name in [
        "recordedIn",
        "reflectedIn",
        "observedIn",
        "seenIn",
        "issuedBy",
    ] {
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
    // A 200 with every subject rejected is the failure mode this project has
    // already shipped once: the import "succeeds" and the store stays empty.
    assert_eq!(
        body["rejected"].as_array().map(Vec::len).unwrap_or(0),
        0,
        "import {source} rejected subjects: {body}"
    );
}

/// One invoice in the books, and a supplier subject carrying the GSTIN — the
/// real shape `graphowl_client.py` emits, where the GSTIN lives on the
/// Supplier and is reached through `gst:issuedBy`, never as a literal on the
/// invoice. A fixture that puts it on the invoice would pass a rule the real
/// data fails, which is the exact class of bug this project has already
/// shipped once.
const BOOKS: &str = r#"
    @prefix gst: <https://graph-owl.dev/packs/gst#> .
    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

    gst:supplier-29AACCG0527D1Z8 rdf:type gst:Supplier ;
        gst:supplierGstin "29AACCG0527D1Z8" .

    gst:books-INV-2001 rdf:type gst:PurchaseInvoice ;
        gst:issuedBy gst:supplier-29AACCG0527D1Z8 ;
        gst:invoiceNumber "INV-2001" ;
        gst:taxAmount "45000.00" .

    gst:canonical-INV-2001 gst:recordedIn gst:books-INV-2001 .
"#;

/// The supplier filed in month N+1 for a month-N invoice: the portal shows it
/// in a 2A pulled in May, and no 2B carries it at all.
const GSTR2A_LATE: &str = r#"
    @prefix gst: <https://graph-owl.dev/packs/gst#> .
    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

    gst:gstr2a-snapshot-2026-03-2026-05-02 rdf:type gst:Gstr2aSnapshot ;
        gst:period "2026-03" ;
        gst:pulledOn "2026-05-02" .

    gst:gstr2a-INV-2001 rdf:type gst:Gstr2aInvoice ;
        gst:invoiceNumber "INV-2001" ;
        gst:taxAmount "45000.00" ;
        gst:seenIn gst:gstr2a-snapshot-2026-03-2026-05-02 .

    gst:canonical-INV-2001 gst:observedIn gst:gstr2a-INV-2001 .
"#;

/// The same invoice, now carried by a 2B — the month-N+1 claim. Imported as
/// its own source so the "no duplicate once claimed" half can be tested by
/// adding it rather than by rewriting the fixture.
const GSTR2B_CLAIMED: &str = r#"
    @prefix gst: <https://graph-owl.dev/packs/gst#> .
    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

    gst:gstr2b-INV-2001 rdf:type gst:Gstr2bInvoice ;
        gst:invoiceNumber "INV-2001" ;
        gst:taxAmount "45000.00" .

    gst:canonical-INV-2001 gst:reflectedIn gst:gstr2b-INV-2001 .
"#;

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

/// `/sparql` returns each binding in its RDF surface form — a literal as
/// `"INV-2001"` (quotes included), an IRI as `<...>`. Comparing against the
/// bare value is what a reader means, so strip the syntax once here rather
/// than quoting every expectation.
fn value(row: &serde_json::Value, var: &str) -> String {
    row[var]
        .as_str()
        .unwrap_or_else(|| panic!("no binding for {var} in {row}"))
        .trim_matches(|c| c == '"' || c == '<' || c == '>')
        .to_string()
}

#[tokio::test]
async fn a_supplier_who_filed_after_the_2b_froze_is_reported_against_the_period_they_filed_for() {
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;
    import(&app, "gst-2a-may-pull", GSTR2A_LATE).await;

    let rows = run(&app, "filed-late-in-gstr2a").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "number"), "INV-2001");
    assert_eq!(value(&rows[0], "gstin"), "29AACCG0527D1Z8");
    // The period is the one the supplier filed *for*, not the one the pull
    // happened in. Reporting May would send a reviewer to the wrong return.
    assert_eq!(value(&rows[0], "observedPeriod"), "2026-03");
    assert_eq!(value(&rows[0], "pulledOn"), "2026-05-02");
}

#[tokio::test]
async fn once_a_2b_carries_the_invoice_the_late_filing_finding_stops_firing() {
    // The second half of the plan's own RED: "and **no** duplicate claim in
    // month N+1". A carry-forward is not a failure, and a rule that keeps
    // firing after the credit became claimable manufactures work.
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;
    import(&app, "gst-2a-may-pull", GSTR2A_LATE).await;
    import(&app, "gst-2b-april", GSTR2B_CLAIMED).await;

    let rows = run(&app, "filed-late-in-gstr2a").await;

    assert!(rows.is_empty(), "{rows:?}");
}

#[tokio::test]
async fn an_invoice_the_portal_has_never_shown_produces_no_late_filing_finding() {
    // The negative that keeps the rule honest: with no 2A loaded at all it
    // must report nothing, not everything. `requires` is what turns this into
    // "not evaluated" rather than "clean" at the outcome level.
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;

    let rows = run(&app, "filed-late-in-gstr2a").await;

    assert!(rows.is_empty(), "{rows:?}");
}

#[tokio::test]
async fn a_reverse_charge_invoice_is_not_reported_as_filed_late() {
    // The recipient self-assesses, so there is no supplier line to arrive
    // late. Same exclusion `missing-in-gstr1.sparql` already makes.
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(
        &app,
        "gst-books",
        &BOOKS.replace(
            r#"gst:invoiceNumber "INV-2001" ;"#,
            r#"gst:invoiceNumber "INV-2001" ;
        gst:reverseCharge "Y" ;"#,
        ),
    )
    .await;
    import(&app, "gst-2a-may-pull", GSTR2A_LATE).await;

    let rows = run(&app, "filed-late-in-gstr2a").await;

    assert!(rows.is_empty(), "{rows:?}");
}

#[tokio::test]
async fn a_2a_value_that_differs_from_the_claimed_2b_is_reported_as_an_amendment() {
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;
    import(&app, "gst-2b-april", GSTR2B_CLAIMED).await;
    // The supplier amended: the portal now shows 41,000 where the frozen 2B
    // said 45,000.
    import(
        &app,
        "gst-2a-may-pull",
        &GSTR2A_LATE.replace(r#"gst:taxAmount "45000.00""#, r#"gst:taxAmount "41000.00""#),
    )
    .await;

    let rows = run(&app, "amended-after-claim").await;

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(value(&rows[0], "number"), "INV-2001");
    assert_eq!(value(&rows[0], "claimedAmount"), "45000.00");
    assert_eq!(value(&rows[0], "observedAmount"), "41000.00");
}

#[tokio::test]
async fn a_2a_agreeing_with_the_claimed_2b_is_not_an_amendment() {
    // The negative the mutation lesson demands: without it, a rule that
    // reported *every* matched invoice would pass the positive test above.
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;
    import(&app, "gst-2b-april", GSTR2B_CLAIMED).await;
    import(&app, "gst-2a-may-pull", GSTR2A_LATE).await;

    let rows = run(&app, "amended-after-claim").await;

    assert!(rows.is_empty(), "{rows:?}");
}

#[tokio::test]
async fn an_invoice_with_no_2b_at_all_is_late_filing_never_an_amendment() {
    // The two rules partition the same join: one fires exactly where the 2B
    // side is absent, the other exactly where it is present. If both fired on
    // one invoice a reviewer would be told two incompatible stories.
    let (app, _db, _url) = test_app().await;
    declare_vocabulary(&app).await;
    import(&app, "gst-books", BOOKS).await;
    import(&app, "gst-2a-may-pull", GSTR2A_LATE).await;

    let late = run(&app, "filed-late-in-gstr2a").await;
    let amended = run(&app, "amended-after-claim").await;

    assert_eq!(late.len(), 1, "{late:?}");
    assert!(amended.is_empty(), "{amended:?}");
}
