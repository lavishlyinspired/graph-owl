//! `POST /graph/context` — Plan 113 Slice A.
//!
//! **The whole point: a subject that is not a catalog asset.** Every fixture
//! here is a bare pack-vocabulary subject, no `POST /assets` call anywhere —
//! the shape a GST invoice actually has once imported via
//! `POST /graph/import/rdf`, not a table with a UUID. If the mechanism only
//! worked for catalog assets, `/assets/{id}/graph` already covers it and this
//! route would exist for nothing.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

const NAMESPACE: &str = "https://graph-owl.dev/packs/planetest113#";

async fn declare_namespace(app: &axum::Router) -> u16 {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/namespaces")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "iri": NAMESPACE }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert!(response.status().is_success(), "declare the namespace");
    u16::try_from(
        json_body(response).await["code"]
            .as_u64()
            .expect("a namespace code"),
    )
    .expect("a u16 code")
}

async fn declare_predicates(app: &axum::Router, namespace: u16) {
    for name in ["issuedBy", "supplierGstin", "adjustedBy"] {
        let (predicate_type, many) = if name == "issuedBy" || name == "adjustedBy" {
            (0, false)
        } else {
            (1, false)
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/predicates")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "namespace": namespace, "name": name, "valueType": predicate_type, "many": many })
                            .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert!(response.status().is_success(), "declare `{name}`");
    }
}

async fn import(app: &axum::Router, turtle: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graph/import/rdf?source=planetest113&format=turtle")
                .header("content-type", "text/turtle")
                .body(Body::from(turtle.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "import the fixture: {body}");
    assert!(
        body["rejected"].as_array().expect("rejected").is_empty(),
        "the fixture did not land: {body}"
    );
}

fn fixture() -> String {
    format!(
        r#"
@prefix pt: <{NAMESPACE}> .

pt:invoice-1 pt:issuedBy pt:supplier-1 ; pt:adjustedBy pt:creditnote-1 .
pt:supplier-1 pt:supplierGstin "27AAACR5055K1ZM" .
"#
    )
}

async fn context(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graph/context")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// **A bare pack subject — not an asset, no UUID — still walks and reports
/// real provenance.** This is the entire reason the route exists.
#[tokio::test]
async fn a_subject_with_no_catalog_asset_row_walks_and_carries_provenance() {
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (status, body) = context(
        &app,
        json!({ "seed": format!("{code}:invoice-1"), "direction": "outgoing", "hops": 2 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes = body["nodes"].as_array().expect("nodes array");
    let ids: Vec<&str> = nodes
        .iter()
        .map(|n| n["id"].as_str().expect("id"))
        .collect();
    assert!(ids.contains(&"invoice-1"), "{body}");
    assert!(ids.contains(&"supplier-1"), "{body}");
    let supplier = nodes
        .iter()
        .find(|n| n["id"] == "supplier-1")
        .expect("the supplier node");
    assert!(
        supplier["sources"]
            .as_array()
            .expect("sources")
            .iter()
            .any(|s| s == "planetest113"),
        "provenance resolved for a bare subject too: {supplier}",
    );
}

/// The relationship filter narrows a bare subject's neighbourhood exactly as
/// it narrows a catalog asset's (Plan 112 Slice A) — one mechanism, proven
/// for the case that previously had no route to reach it at all.
#[tokio::test]
async fn the_relationship_filter_narrows_a_bare_subjects_neighbourhood() {
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (_, unfiltered) = context(
        &app,
        json!({ "seed": format!("{code}:invoice-1"), "direction": "outgoing", "hops": 1 }),
    )
    .await;
    assert_eq!(
        unfiltered["edges"].as_array().expect("edges").len(),
        2,
        "{unfiltered}"
    );

    let (status, filtered) = context(
        &app,
        json!({
            "seed": format!("{code}:invoice-1"), "direction": "outgoing", "hops": 1,
            "relationshipTypes": ["issuedBy"]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    let edges = filtered["edges"].as_array().expect("edges");
    assert_eq!(edges.len(), 1, "{filtered}");
    assert_eq!(edges[0]["relationship"], "issuedBy", "{filtered}");
}

/// A subject nothing points to and that points nowhere is `200` with an empty
/// picture — the same posture every walk in this product takes.
#[tokio::test]
async fn an_unconnected_subject_is_an_empty_picture_not_a_failure() {
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (status, body) = context(
        &app,
        json!({ "seed": format!("{code}:nothing-here"), "direction": "both", "hops": 2 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes = body["nodes"].as_array().expect("nodes");
    assert!(nodes.len() <= 1, "{body}");
    assert!(
        body["edges"].as_array().expect("edges").is_empty(),
        "{body}"
    );
    assert_eq!(body["truncated"], false);
}

/// A missing seed is a `400` naming the field, matching every other route.
#[tokio::test]
async fn a_request_with_no_seed_is_rejected_by_name() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graph/context")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "hops": 2 }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert!(
        body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|e| e["field"] == "seed"),
        "{body}"
    );
}

async fn analytics(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graph/context/analytics")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// `POST /graph/context/analytics` — Plan 113 Slice B, over HTTP.
///
/// **Same envelope as `/assets/{id}/analytics`, over a subject that has no
/// asset row.** A GST invoice's `issuedBy` is one direct triple, exactly the
/// shape the real Postgres-backed traversal and analytics both read from —
/// unlike the in-memory double, no reified-relationship workaround is needed
/// here, which is itself evidence this is the realistic case, not a
/// contrived one.
#[tokio::test]
async fn connectivity_for_a_bare_subject_matches_the_asset_envelope() {
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (status, body) = analytics(
        &app,
        json!({ "seed": format!("{code}:invoice-1"), "direction": "outgoing", "hops": 1 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes = body["nodes"].as_array().expect("nodes array");
    assert_eq!(
        nodes.len(),
        body["outDegree"].as_array().expect("outDegree").len(),
        "{body}"
    );
    let invoice_index = nodes
        .iter()
        .position(|n| n.as_str() == Some(&format!("{code}:invoice-1")))
        .expect("the seed is one of the reported nodes");
    let out_degree = body["outDegree"][invoice_index].as_f64().expect("a number");
    assert!(
        (out_degree - 2.0).abs() < f64::EPSILON,
        "the invoice points at its supplier and its credit note: {body}",
    );
    assert_eq!(body["truncated"], false);
}

/// The relationship filter narrows analytics exactly as it narrows the
/// picture — the fix `graph_context_analytics_for`'s own unit tests found,
/// now proven end to end.
#[tokio::test]
async fn the_filter_narrows_connectivity_for_a_bare_subject_too() {
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (status, body) = analytics(
        &app,
        json!({
            "seed": format!("{code}:invoice-1"), "direction": "outgoing", "hops": 1,
            "relationshipTypes": ["issuedBy"]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes = body["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 2, "{body}");
    assert!(
        !nodes
            .iter()
            .any(|n| n.as_str() == Some(&format!("{code}:creditnote-1"))),
        "the adjustedBy-only neighbour is excluded from the filtered analytics too: {body}",
    );
}

/// Plan 121 Slice 2 — the same `[console.labels]` resolution Slice 1 wired
/// into the evidence-graph route reaches `/graph/context` too, since
/// `SubjectExplorer` (Explore's graph view) walks *any* subject through this
/// route, not only a finding's own subject.
///
/// **Points `GRAPH_OWL_PACKS_DIR` at this repo's real `packs/` directory**,
/// the same pattern `pack_install.rs`'s own tests use — a synthetic
/// namespace (every other test in this file) has no `[console.labels]` to
/// resolve against, so proving the mechanism here needs the real, shipped
/// `gst` manifest and its real `declaredBy: "pack:gst"` provenance.
mod real_gst_pack {
    use std::path::PathBuf;

    use crate::common::{json_body, test_app};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use tower::ServiceExt;

    fn real_packs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
    }

    /// Every test in this module writes the identical value, so a
    /// concurrent write from another test in this same process can only
    /// ever race to the same outcome — the same reasoning `pack_install.rs`
    /// already documents for its own copy of this helper.
    fn set_packs_dir_to_the_real_one() {
        unsafe {
            std::env::set_var("GRAPH_OWL_PACKS_DIR", real_packs_dir());
        }
    }

    async fn seed_a_named_supplier_two_hops_from_an_invoice(app: &axum::Router) {
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

        for (name, value_type) in [("issuedBy", 0), ("supplierGstin", 1), ("supplierName", 1)] {
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

            gst:invoice-1 gst:issuedBy gst:supplier-1 .
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
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "import: {body}");
    }

    #[tokio::test]
    async fn a_walked_neighbour_shows_its_declared_label_not_its_bare_id() {
        set_packs_dir_to_the_real_one();
        let (app, _container, _connection_string) = test_app().await;
        seed_a_named_supplier_two_hops_from_an_invoice(&app).await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/graph/context")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "seed": "1024:invoice-1", "direction": "outgoing", "hops": 1,
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let nodes = body["nodes"].as_array().expect("nodes array");
        let supplier = nodes
            .iter()
            .find(|n| n["id"] == "supplier-1")
            .unwrap_or_else(|| panic!("supplier node present: {nodes:?}"));
        assert_eq!(
            supplier["label"],
            json!("Nimbus Freight Logistics"),
            "a walked neighbour must resolve its label the same way the evidence \
             graph does, not just the seed a caller already knew the name of: \
             {supplier:?}"
        );

        // The negative case: the invoice's own class has no [console.labels]
        // entry, so it degrades to null rather than fabricating one.
        let invoice = nodes
            .iter()
            .find(|n| n["id"] == "invoice-1")
            .unwrap_or_else(|| panic!("invoice node present: {nodes:?}"));
        assert_eq!(
            invoice["label"],
            serde_json::Value::Null,
            "an untyped subject must degrade to null: {invoice:?}"
        );
    }
}
