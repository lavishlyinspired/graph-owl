//! `GET /findings` and `POST /findings/{id}/decision` — Epic 105 P5.
//!
//! **One queue, every domain.** The storage tests prove the table's
//! invariants and the `graph-owl-api` unit tests prove the layer's decisions;
//! these prove the half only HTTP can — that the wire shape is what a console
//! can render, that the deciding principal is the authenticated one rather
//! than a field anybody can set, and that a bad status is a `400` a caller can
//! act on rather than a `500`.
//!
//! There is deliberately no `/gst/findings`. A per-domain route is the
//! hardcoding `plans/105-domain-neutrality.md` exists to prevent, and the
//! first one would make the second inevitable.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use graph_owl_core::finding::{Evidence, Finding};
use graph_owl_storage::FindingStore;
use graph_owl_storage_postgres::PostgresStorage;
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", token(subject)))
        .header("content-type", "application/json");
    let request = match body {
        Some(value) => request.body(Body::from(value.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request should build");

    let response = app
        .clone()
        .oneshot(request)
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

fn finding(pack: &str, label: &str, subject: &str) -> Finding {
    Finding::new(
        pack,
        label,
        subject,
        "Claimed in the register, never filed by the supplier",
        "gst:Section16",
        vec![Evidence {
            subject: subject.to_string(),
            predicate: "1025:taxAmount".to_string(),
            value: "45000.00".to_string(),
            var: None,
        }],
    )
    .expect("a complete finding")
}

/// Seed through the store, so the read tests below exercise the read path
/// alone. The HTTP write path is admin-gated and has its own tests at the
/// bottom of this file.
async fn seed(connection_string: &str, findings: &[Finding]) {
    let store = PostgresStorage::connect(connection_string)
        .await
        .expect("connect");
    for f in findings {
        store.record_finding(f).await.expect("record");
    }
}

#[tokio::test]
async fn the_queue_carries_the_citation_and_the_evidence_a_reviewer_needs() {
    let (app, _db, url) = test_app().await;
    let one = finding("gst", "gst:MissingInGstr2b", "1025:inv-1");
    seed(&url, std::slice::from_ref(&one)).await;

    let (status, body) = send(&app, "GET", "/findings", "system", None).await;

    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("an array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["label"], "gst:MissingInGstr2b");
    assert_eq!(
        rows[0]["governedBy"], "gst:Section16",
        "camelCase on the wire, and never absent — a finding with no citation \
         is an accusation"
    );
    assert_eq!(rows[0]["status"], "pending");
    assert_eq!(
        rows[0]["evidence"][0]["predicate"], "1025:taxAmount",
        "the evidence is what lets a reviewer follow the conclusion back into \
         the graph instead of trusting a summary of it"
    );
}

#[tokio::test]
async fn one_route_serves_every_pack() {
    // The property that makes findings a runtime rather than a GST feature.
    let (app, _db, url) = test_app().await;
    seed(
        &url,
        &[
            finding("gst", "gst:MissingInGstr2b", "1025:inv-1"),
            finding("hospitality", "hosp:DuplicateGuest", "1024:guest-1"),
        ],
    )
    .await;

    let (_, all) = send(&app, "GET", "/findings", "system", None).await;
    let (_, gst) = send(&app, "GET", "/findings?pack=gst", "system", None).await;
    let (_, hosp) = send(&app, "GET", "/findings?pack=hospitality", "system", None).await;

    assert_eq!(all.as_array().expect("array").len(), 2);
    assert_eq!(gst.as_array().expect("array").len(), 1);
    assert_eq!(hosp.as_array().expect("array").len(), 1);
    assert_eq!(hosp[0]["label"], "hosp:DuplicateGuest");
}

#[tokio::test]
async fn a_decision_is_attributed_to_the_caller_not_to_the_body() {
    // **The reason `actor` is not a field.** A body-supplied name would let
    // any caller attribute their dismissal to somebody else, which is worse
    // than no attribution at all — it is a false record.
    let (app, _db, url) = test_app().await;
    let one = finding("gst", "gst:MissingInGstr2b", "1025:inv-1");
    seed(&url, std::slice::from_ref(&one)).await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/findings/{}/decision", one.id),
        "system",
        Some(serde_json::json!({ "status": "accepted" })),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(&app, "GET", "/findings?status=accepted", "system", None).await;
    assert_eq!(body[0]["decidedBy"], "system");
}

#[tokio::test]
async fn dismissing_without_a_reason_is_refused_with_a_message_a_reviewer_can_act_on() {
    let (app, _db, url) = test_app().await;
    let one = finding("gst", "gst:MissingInGstr2b", "1025:inv-1");
    seed(&url, std::slice::from_ref(&one)).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/findings/{}/decision", one.id),
        "system",
        Some(serde_json::json!({ "status": "rejected" })),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the reviewer's mistake reaches them as a 400 naming the field, not \
         as a constraint violation from the database"
    );
    assert!(
        body.to_string().contains("reason"),
        "and the body says which field: {body}"
    );
}

#[tokio::test]
async fn dismissing_with_a_reason_records_it() {
    let (app, _db, url) = test_app().await;
    let one = finding("gst", "gst:MissingInGstr2b", "1025:inv-1");
    seed(&url, std::slice::from_ref(&one)).await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/findings/{}/decision", one.id),
        "system",
        Some(serde_json::json!({
            "status": "rejected",
            "reason": "the supplier filed in the next period"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(&app, "GET", "/findings?status=rejected", "system", None).await;
    assert_eq!(body[0]["reason"], "the supplier filed in the next period");
    assert_eq!(
        body.as_array().expect("array").len(),
        1,
        "and it is out of the pending queue"
    );
}

#[tokio::test]
async fn a_status_nobody_defined_is_a_400_rather_than_an_empty_list() {
    // An unknown status silently returning nothing is the worst outcome: the
    // reviewer sees an empty queue and concludes there is no work.
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(&app, "GET", "/findings?status=dismissed", "system", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("status"), "body: {body}");
}

#[tokio::test]
async fn deciding_a_finding_that_is_gone_is_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/findings/{}/decision", uuid::Uuid::new_v4()),
        "system",
        Some(serde_json::json!({ "status": "accepted" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_finding_cannot_be_pushed_back_to_pending() {
    // `pending` is the absence of a decision. Allowing it as one would let a
    // reviewer erase an accountable record by "deciding" undecided.
    let (app, _db, url) = test_app().await;
    let one = finding("gst", "gst:MissingInGstr2b", "1025:inv-1");
    seed(&url, std::slice::from_ref(&one)).await;
    send(
        &app,
        "POST",
        &format!("/findings/{}/decision", one.id),
        "system",
        Some(serde_json::json!({ "status": "accepted" })),
    )
    .await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/findings/{}/decision", one.id),
        "system",
        Some(serde_json::json!({ "status": "pending" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_reconciliation_run_reports_new_work_separately_from_old() {
    // **The number an operator is allowed to see each morning.** A scheduled
    // run over a corpus that has not changed finds the same problems; a single
    // count would report them as new every day until nobody read it.
    let (app, _db, _url) = test_app().await;
    let batch = serde_json::json!({
        "findings": [{
            "pack": "gst",
            "label": "gst:MissingInGstr2b",
            "subject": "1025:inv-1",
            "summary": "Claimed in the register, never filed by the supplier",
            "governedBy": "gst:Section16",
            "evidence": [{
                "subject": "1025:inv-1",
                "predicate": "1025:taxAmount",
                "value": "45000.00"
            }]
        }]
    });

    let (first_status, first) =
        send(&app, "POST", "/findings", "system", Some(batch.clone())).await;
    let (_, second) = send(&app, "POST", "/findings", "system", Some(batch)).await;

    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(first["opened"], 1);
    assert_eq!(first["alreadyOpen"], 0);
    assert_eq!(
        second["opened"], 0,
        "a re-run over unchanged data opens nothing"
    );
    assert_eq!(second["alreadyOpen"], 1);
}

#[tokio::test]
async fn a_finding_with_no_evidence_is_refused_and_says_which_one() {
    // A batch of two hundred that fails on one is otherwise a reconciliation
    // nobody can debug — and a finding with no evidence is an accusation with
    // nothing behind it, which is exactly what this system must never queue.
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/findings",
        "system",
        Some(serde_json::json!({
            "findings": [
                {
                    "pack": "gst", "label": "gst:A", "subject": "1025:inv-1",
                    "summary": "s", "governedBy": "gst:Section16",
                    "evidence": [{"subject": "1025:inv-1", "predicate": "p", "value": "v"}]
                },
                {
                    "pack": "gst", "label": "gst:B", "subject": "1025:inv-2",
                    "summary": "s", "governedBy": "gst:Section16",
                    "evidence": []
                }
            ]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string().contains("findings[1]"),
        "the failing one is named by index: {body}"
    );

    let (_, queued) = send(&app, "GET", "/findings", "system", None).await;
    assert_eq!(
        queued.as_array().expect("array").len(),
        0,
        "and nothing from a refused batch lands — a half-written \
         reconciliation is worse than a failed one, because it looks complete"
    );
}

/// Plan 121 Slice 3 — the same `[console.labels]` resolution the evidence
/// graph (Slice 1) and `/graph/context` (Slice 2) already apply reaches the
/// findings queue's own subject column too, since a reviewer scans this list
/// before opening any single finding.
mod real_gst_pack {
    use std::path::PathBuf;

    use crate::common::{test_app, token};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use graph_owl_core::finding::{Evidence, Finding};
    use graph_owl_storage::FindingStore;
    use graph_owl_storage_postgres::PostgresStorage;
    use serde_json::json;
    use tower::ServiceExt;

    fn real_packs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
    }

    /// Every test in this module writes the identical value — the same
    /// reasoning `pack_install.rs`'s own copy of this helper documents.
    fn set_packs_dir_to_the_real_one() {
        unsafe {
            std::env::set_var("GRAPH_OWL_PACKS_DIR", real_packs_dir());
        }
    }

    async fn seed_a_named_supplier(app: &axum::Router) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/namespaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "iri": "https://graph-owl.dev/packs/gst#",
                            "declaredBy": "pack:gst",
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert!(response.status().is_success(), "declare the gst namespace");

        for (name, value_type) in [("supplierGstin", 1), ("supplierName", 1)] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/predicates")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "namespace": 1024, "name": name,
                                "valueType": value_type, "many": false,
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await
                .expect("request should be handled");
            assert!(response.status().is_success(), "declare {name}");
        }

        let turtle = r#"
            @prefix gst: <https://graph-owl.dev/packs/gst#> .
            @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

            gst:supplier-1 rdf:type gst:Supplier ;
                gst:supplierGstin "29AACCG0527D1Z8" ;
                gst:supplierName "Nimbus Freight Logistics" .
        "#;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graph/import/rdf?source=gst-purchase-register&format=turtle")
                    .header("content-type", "text/turtle")
                    .body(Body::from(turtle.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::OK, "import the supplier");
    }

    #[tokio::test]
    async fn a_finding_s_subject_shows_its_declared_label_not_its_bare_id() {
        set_packs_dir_to_the_real_one();
        let (app, _db, url) = test_app().await;
        seed_a_named_supplier(&app).await;

        let finding = Finding::new(
            "gst",
            "gst:SupplierNotFiled",
            "https://graph-owl.dev/packs/gst#supplier-1",
            "Claimed in the register, never filed by the supplier",
            "gst:Section16",
            vec![Evidence {
                subject: "https://graph-owl.dev/packs/gst#supplier-1".to_string(),
                predicate: "1024:supplierGstin".to_string(),
                value: "29AACCG0527D1Z8".to_string(),
                var: None,
            }],
        )
        .expect("a complete finding");
        let store = PostgresStorage::connect(&url).await.expect("connect");
        store.record_finding(&finding).await.expect("record");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/findings")
                    .header("authorization", format!("Bearer {}", token("system")))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(status, StatusCode::OK, "{body}");

        let rows = body.as_array().expect("array");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            rows[0]["subjectLabel"],
            json!("Nimbus Freight Logistics"),
            "the queue row's own subject must resolve the same [console.labels] \
             entry the evidence graph and /graph/context already do: {:?}",
            rows[0]
        );
        // Every other field a reviewer relies on must still be there —
        // adding subjectLabel must not have replaced the finding's shape.
        assert_eq!(rows[0]["label"], "gst:SupplierNotFiled");
        assert_eq!(rows[0]["governedBy"], "gst:Section16");
    }

    #[tokio::test]
    async fn a_finding_whose_subject_has_no_declared_label_degrades_to_null() {
        set_packs_dir_to_the_real_one();
        let (app, _db, url) = test_app().await;
        seed_a_named_supplier(&app).await;

        // The invoice class (unlike Supplier) has no [console.labels] entry —
        // a finding about it must degrade rather than fabricate a name.
        let finding = Finding::new(
            "gst",
            "gst:SomeInvoiceFinding",
            "https://graph-owl.dev/packs/gst#invoice-never-imported",
            "s",
            "gst:Section16",
            vec![Evidence {
                subject: "https://graph-owl.dev/packs/gst#invoice-never-imported".to_string(),
                predicate: "1024:supplierGstin".to_string(),
                value: "x".to_string(),
                var: None,
            }],
        )
        .expect("a complete finding");
        let store = PostgresStorage::connect(&url).await.expect("connect");
        store.record_finding(&finding).await.expect("record");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/findings")
                    .header("authorization", format!("Bearer {}", token("system")))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

        let rows = body.as_array().expect("array");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            rows[0]["subjectLabel"],
            serde_json::Value::Null,
            "an unresolvable subject must degrade to null: {:?}",
            rows[0]
        );
    }
}

/// Plan 120 Slice C / Plan 121 Slice 4 — the domain-neutrality proof.
/// Every earlier slice in this mechanism used GST fixtures; this proves the
/// same, unmodified server code resolves a label for a completely unrelated
/// domain from nothing but a `pack.toml` declaration — hospitality's own
/// stated acceptance criterion (`packs/hospitality/pack.toml`'s own header:
/// "zero changes to any `.rs` file and zero changes to any `.tsx` file").
mod real_hospitality_pack {
    use std::path::PathBuf;

    use crate::common::{test_app, token};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use graph_owl_core::finding::{Evidence, Finding};
    use graph_owl_storage::FindingStore;
    use graph_owl_storage_postgres::PostgresStorage;
    use serde_json::json;
    use tower::ServiceExt;

    fn real_packs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
    }

    fn set_packs_dir_to_the_real_one() {
        unsafe {
            std::env::set_var("GRAPH_OWL_PACKS_DIR", real_packs_dir());
        }
    }

    /// The real, shipped fixture — not a hand-copied subset — so this proves
    /// the mechanism against the same data `packs/hospitality/pack.toml`
    /// itself declares under `[[documents]] source = "hospitality-estate"`.
    fn real_estate_fixture() -> String {
        std::fs::read_to_string(real_packs_dir().join("hospitality/fixtures/estate.ttl"))
            .expect("read the real hospitality fixture")
    }

    async fn seed_the_real_estate_fixture(app: &axum::Router) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/namespaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "iri": "https://graph-owl.dev/packs/hospitality#",
                            "declaredBy": "pack:hospitality",
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert!(
            response.status().is_success(),
            "declare the hospitality namespace"
        );

        for (name, value_type) in [
            ("label", 1),
            ("name", 1),
            ("guestPhone", 1),
            ("guestSurname", 1),
            ("stayedAt", 0),
            ("bookedBy", 0),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/predicates")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "namespace": 1024, "name": name,
                                "valueType": value_type, "many": false,
                            })
                            .to_string(),
                        ))
                        .expect("request should build"),
                )
                .await
                .expect("request should be handled");
            assert!(response.status().is_success(), "declare {name}");
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graph/import/rdf?source=hospitality-estate&format=turtle")
                    .header("content-type", "text/turtle")
                    .body(Body::from(real_estate_fixture()))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "import the real hospitality fixture"
        );
    }

    #[tokio::test]
    async fn a_duplicate_guest_findings_evidence_graph_shows_the_guests_own_surname() {
        set_packs_dir_to_the_real_one();
        let (app, _db, url) = test_app().await;
        seed_the_real_estate_fixture(&app).await;

        let finding = Finding::new(
            "hospitality",
            "hosp:DuplicateGuest",
            "https://graph-owl.dev/packs/hospitality#guest-1",
            "Two guest records that appear to be the same person",
            "hosp:GuestRecordPolicy",
            vec![Evidence {
                subject: "https://graph-owl.dev/packs/hospitality#guest-1".to_string(),
                predicate: "1024:guestSurname".to_string(),
                value: "Smith".to_string(),
                var: None,
            }],
        )
        .expect("a complete finding");
        let store = PostgresStorage::connect(&url).await.expect("connect");
        store.record_finding(&finding).await.expect("record");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/findings/{}/evidence-graph", finding.id))
                    .header("authorization", format!("Bearer {}", token("system")))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(status, StatusCode::OK, "{body}");

        let nodes = body["nodes"].as_array().expect("nodes array");
        let guest = nodes
            .iter()
            .find(|n| n["id"] == "guest-1")
            .unwrap_or_else(|| panic!("guest-1 node present: {nodes:?}"));
        assert_eq!(
            guest["label"],
            json!("Smith"),
            "the same, unmodified server code must resolve a hospitality \
             guest's own surname from nothing but packs/hospitality/pack.toml's \
             [console.labels] — no GST-specific assumption anywhere in the \
             mechanism: {guest:?}"
        );
    }
}
