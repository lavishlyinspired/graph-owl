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
    assert!(
        refused.is_none(),
        "a non-admin must be refused: {refused:?}"
    );
}

/// **Epic 105 P10's `run_rule()` tool, against the real adapter.** The
/// single-rule counterpart to the test above, proving the same two
/// properties for the narrower call: `CatalogContext::run_rule` reaches
/// `Catalog::run_rule` for a real admin, and refuses a real non-admin —
/// plus the one property `run_rule` adds that `reconcile` has no way to
/// express, an unknown label coming back exactly like a denial.
#[tokio::test]
async fn run_rule_admits_an_admin_refuses_a_non_admin_and_reports_an_unknown_rule() {
    let (catalog, _db, _url) = test_catalog().await;
    let app = graph_owl_server::app(catalog.clone());
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    let admin = Principal::system();
    let admin_reads = graph_owl_mcp::catalog::CatalogContext::new(catalog.clone(), admin.clone());
    let outcome = graph_owl_mcp::ContextSource::run_rule(
        &admin_reads,
        &admin.id,
        "gst",
        "gst:PotentialMismatch",
    )
    .await
    .expect("no source error")
    .expect("an admin may run the rule");
    assert_eq!(outcome.pack, "gst");
    assert_eq!(
        outcome.evaluated, 1,
        "one rule, not the whole pack: {outcome:?}"
    );
    assert_eq!(outcome.found, 1, "{outcome:?}");
    assert_eq!(outcome.opened, 1, "{outcome:?}");

    let contractor = Principal {
        id: "contractor".to_string(),
        name: "contractor".to_string(),
        kind: PrincipalKind::User,
        roles: Vec::new(),
        is_admin: false,
    };
    let contractor_reads =
        graph_owl_mcp::catalog::CatalogContext::new(catalog.clone(), contractor.clone());
    let refused = graph_owl_mcp::ContextSource::run_rule(
        &contractor_reads,
        &contractor.id,
        "gst",
        "gst:PotentialMismatch",
    )
    .await
    .expect("no source error");
    assert!(
        refused.is_none(),
        "a non-admin must be refused: {refused:?}"
    );

    let unknown =
        graph_owl_mcp::ContextSource::run_rule(&admin_reads, &admin.id, "gst", "gst:NoSuchRule")
            .await
            .expect("no source error");
    assert!(
        unknown.is_none(),
        "an unknown rule must read the same as a denial: {unknown:?}"
    );
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

// ---- Plan 109 Slice 2: the canonical gst:Invoice, against a real graph ----
//
// Both tests below seed a minimal, self-contained shape rather than loading
// the real pack — the same pattern every other test in this file already
// uses — and prove the *traversal*, not a finding rule. Each import lands in
// its own named graph exactly as a real pack load does, so a canonical
// subject's edges to its per-source records are only findable if every
// pattern below sits inside its own `GRAPH` block — the same discipline
// `packs/gst/queries/*.sparql` document at length.

async fn sparql(app: &axum::Router, query: &str) -> serde_json::Value {
    let (status, body) = json(
        app,
        "POST",
        "/sparql",
        serde_json::json!({ "query": query }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn declare_gst_namespace_and_predicates(app: &axum::Router, string_predicates: &[&str]) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    for name in string_predicates {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 1, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }
    for name in ["issuedBy", "recordedIn", "appearsIn", "reflectedIn"] {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 0, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }
}

/// **The user's own words: "the most important end-to-end test in the whole
/// plan."** A Books record, a GSTR-1 record and a GSTR-2B record
/// representing the same real invoice resolve to exactly one canonical
/// `gst:Invoice`, and all three source records remain independently
/// reachable — and queryable in their own named graphs — from it.
#[tokio::test]
async fn a_books_gstr1_and_gstr2b_record_for_one_invoice_converge_on_one_canonical_subject() {
    let (app, _db, _url) = test_app().await;
    declare_gst_namespace_and_predicates(&app, &["supplierGstin", "invoiceNumber"]).await;

    let books = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:pr-INV-9001 rdf:type gst:PurchaseInvoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-9001" .

        gst:invoice-27AABCU9603R1ZM-INV9001 rdf:type gst:Invoice ;
            gst:recordedIn gst:pr-INV-9001 .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-purchase-register&format=turtle",
        "text/turtle",
        books.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "books import: {body}");

    let gstr1 = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:g1-INV-9001 rdf:type gst:Gstr1Invoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-9001" .

        gst:invoice-27AABCU9603R1ZM-INV9001 rdf:type gst:Invoice ;
            gst:appearsIn gst:g1-INV-9001 .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-gstr1&format=turtle",
        "text/turtle",
        gstr1.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "gstr1 import: {body}");

    let gstr2b = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:2b-INV-9001 rdf:type gst:Gstr2bInvoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-9001" .

        gst:invoice-27AABCU9603R1ZM-INV9001 rdf:type gst:Invoice ;
            gst:reflectedIn gst:2b-INV-9001 .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-gstr2b&format=turtle",
        "text/turtle",
        gstr2b.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "gstr2b import: {body}");

    // One traversal, from the one canonical subject, to all three
    // independently-imported, independently-graphed per-source records —
    // never a value join on a shared key.
    let result = sparql(
        &app,
        r"
            PREFIX gst: <https://graph-owl.dev/packs/gst#>
            SELECT ?canonical ?purchase ?declared ?filed
            WHERE {
              GRAPH ?g1 { ?canonical a gst:Invoice ; gst:recordedIn ?purchase . }
              GRAPH ?g2 { ?canonical gst:appearsIn ?declared . }
              GRAPH ?g3 { ?canonical gst:reflectedIn ?filed . }
            }
        ",
    )
    .await;

    let rows = result["rows"].as_array().expect("rows array");
    assert_eq!(
        rows.len(),
        1,
        "one canonical subject must reach all three records exactly once: {result}"
    );
    let row = &rows[0];
    let field = |key: &str| {
        row[key]
            .as_str()
            .unwrap_or("")
            .trim_matches(['"', '<', '>'])
    };
    assert!(
        field("canonical").contains("invoice-27AABCU9603R1ZM-INV9001"),
        "{row}"
    );
    assert!(field("purchase").contains("pr-INV-9001"), "{row}");
    assert!(field("declared").contains("g1-INV-9001"), "{row}");
    assert!(field("filed").contains("2b-INV-9001"), "{row}");
}

/// **Proving Filing/Statement's period-scoping does the temporal job it
/// exists for.** An invoice declared by the supplier in July and reflected
/// in *August's* GSTR-2B (a late filing carrying forward — GST's own
/// published guidance for exactly this) queries as reflected in the later
/// period, never the invoice's own, earlier one — traversing `Invoice
/// --reflectedIn--> Gstr2bInvoice --reflectedIn--> Gstr2bStatement
/// --period-->`.
#[tokio::test]
async fn an_invoice_carried_forward_into_a_later_periods_2b_reports_the_later_period() {
    let (app, _db, _url) = test_app().await;
    declare_gst_namespace_and_predicates(
        &app,
        &["supplierGstin", "invoiceNumber", "invoiceDate", "period"],
    )
    .await;
    for name in ["generatedFor"] {
        let (status, _) = json(
            &app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 0, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "predicate {name}");
    }

    let gstr1 = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:g1-INV-9002 rdf:type gst:Gstr1Invoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-9002" ;
            gst:invoiceDate "2026-07-15" .

        gst:invoice-27AABCU9603R1ZM-INV9002 rdf:type gst:Invoice ;
            gst:appearsIn gst:g1-INV-9002 .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-gstr1&format=turtle",
        "text/turtle",
        gstr1.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "gstr1 import: {body}");

    // Filed on time — declared for July — but the July 2B was generated
    // before this line arrived, so it never reflected it. It only surfaces
    // once August's 2B is generated, one period later.
    let gstr2b_august = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:recipient-self rdf:type gst:Recipient .

        gst:g2bstatement-2026-08 rdf:type gst:Gstr2bStatement ;
            gst:period "2026-08" ;
            gst:generatedFor gst:recipient-self .

        gst:2b-INV-9002 rdf:type gst:Gstr2bInvoice ;
            gst:supplierGstin "27AABCU9603R1ZM" ;
            gst:invoiceNumber "INV-9002" ;
            gst:reflectedIn gst:g2bstatement-2026-08 .

        gst:invoice-27AABCU9603R1ZM-INV9002 rdf:type gst:Invoice ;
            gst:reflectedIn gst:2b-INV-9002 .
    "#;
    let (status, body) = call(
        &app,
        "POST",
        "/graph/import/rdf?source=gst-gstr2b-2026-08&format=turtle",
        "text/turtle",
        gstr2b_august.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "gstr2b import: {body}");

    let result = sparql(
        &app,
        r"
            PREFIX gst: <https://graph-owl.dev/packs/gst#>
            SELECT ?period
            WHERE {
              GRAPH ?g1 { ?canonical a gst:Invoice ; gst:appearsIn ?declared . }
              GRAPH ?g2 {
                ?canonical gst:reflectedIn ?filed .
                ?filed gst:reflectedIn ?statement .
                ?statement gst:period ?period .
              }
            }
        ",
    )
    .await;

    let rows = result["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1, "{result}");
    let period = rows[0]["period"].as_str().unwrap_or("").trim_matches('"');
    assert_eq!(
        period, "2026-08",
        "must report the period the 2B actually reflected it in, not the invoice's own July date: {result}"
    );
}

/// **Regression, 19 August 2026.** Plan 123 Slice C0 gave this route an
/// optional `{"graphs": [...]}` body and, in doing so, made a literal JSON
/// `null` body fail to deserialize — the route answered `422` to a request
/// that unambiguously means "run unscoped". It shipped because this crate's
/// own tests were not being run at the time; three of them had been failing
/// on it.
///
/// There are two ways a caller says "no scope" and both are real: the Python
/// pack client sends **no body**, and anything building a request from a
/// nullable value sends **`null`**. Neither may 422.
#[tokio::test]
async fn every_form_of_no_scope_runs_unscoped_rather_than_refusing() {
    let (app, _db, _url) = test_app().await;
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    // A literal JSON null.
    let (status, body) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "null body: {body}");
    assert_eq!(body["evaluated"], 1, "{body}");

    // An empty object — the same meaning, spelled the other way.
    let (status, body) = json(&app, "POST", "/packs/gst/reconcile", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK, "empty object: {body}");
    assert_eq!(body["evaluated"], 1, "{body}");

    // No body and **no content-type** — the real shape the Python pack client
    // sends, which only sets content-type when it has a body to describe
    // (`graph_owl_packs.loader._request`). Checked against that code rather
    // than assumed: an empty body that still claims `application/json` is
    // malformed JSON and is correctly refused, so asserting it here would have
    // pinned a shape no client produces.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/gst/reconcile")
                .header("authorization", format!("Bearer {}", token("system")))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::OK, "absent body");
}

/// The other half: a *malformed* scope must still be refused. Accepting `null`
/// must not become accepting anything — a mistyped key that silently widened a
/// run to the whole store is exactly the bug C0 exists to prevent.
#[tokio::test]
async fn a_misspelled_scope_key_is_refused_rather_than_silently_ignored() {
    let (app, _db, _url) = test_app().await;
    seed_gst_vocabulary_and_one_unmatched_invoice(&app).await;
    register_missing_in_gstr2b_rule(&app).await;

    let (status, _) = json(
        &app,
        "POST",
        "/packs/gst/reconcile",
        serde_json::json!({"graph": ["reco-abc-books"]}),
    )
    .await;

    assert_ne!(status, StatusCode::OK, "a mistyped scope key must not run");
}
