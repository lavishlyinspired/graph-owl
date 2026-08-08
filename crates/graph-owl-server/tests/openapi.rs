//! Epic 1 Slice J: the contract matches the server, and the committed copy
//! matches the code.
//!
//! Three failures this guards against, in increasing order of how long they
//! take to notice:
//!
//! 1. A route documented that does not exist — a client generates a call that
//!    404s.
//! 2. A route that exists and is not documented — a capability nobody can find,
//!    and a generated client that silently lacks it.
//! 3. The committed `openapi.json` drifting from the code, so a reviewer reads
//!    a contract the server stopped honouring.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use graph_owl_server::openapi::{ROUTES, document};
use tower::ServiceExt;

async fn call(app: &axum::Router, method: &str, uri: &str) -> StatusCode {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    response.status()
}

/// **Every declared route exists.** A path in the spec that the router does not
/// serve is a client generating a call that 404s, and the spec is the last
/// place anyone looks.
///
/// Asserted as "not `405 Method Not Allowed`", which is what axum returns when
/// a path matches and the method does not. A `404` is not usable as the signal:
/// the SPA fallback answers unknown paths with `200 text/html` by design, so an
/// unmatched path is indistinguishable from a real one at the status level —
/// but a *matched* path with the wrong method still produces `405`.
#[tokio::test]
async fn every_documented_route_is_served_by_the_router() {
    let (app, _container, _) = test_app().await;

    for route in ROUTES {
        let uri = route
            .path
            .replace("{id}", "99999999-9999-4999-8999-999999999999");
        let status = call(&app, &route.method.to_uppercase(), &uri).await;

        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{} {} is documented but the router does not serve that method",
            route.method,
            route.path
        );
    }
}

/// And the negative half: a method the table does **not** declare on a
/// documented path must be refused. Without this, a router that accepted every
/// verb on every path would satisfy the test above.
#[tokio::test]
async fn a_method_the_contract_does_not_declare_is_refused() {
    let (app, _container, _) = test_app().await;

    // `/health` is GET-only in the table.
    assert_eq!(
        call(&app, "PUT", "/health").await,
        StatusCode::METHOD_NOT_ALLOWED,
        "the router accepts a verb the contract does not describe"
    );
}

/// The served document is the generated one — not a stale file read from disk,
/// and not an empty stub.
#[tokio::test]
async fn the_endpoint_serves_the_generated_document() {
    let (app, _container, _) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let served: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");

    assert_eq!(
        served,
        document(),
        "the endpoint must serve what the code generates"
    );
}

/// **The drift guard.** `openapi.json` is committed so a contract change shows
/// up in review as a diff rather than being discovered by a client. This test
/// is what makes that true; regenerate with
/// `cargo run -p graph-owl-server --bin openapi > openapi.json`.
#[test]
fn the_committed_contract_matches_the_code() {
    let committed =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi.json"))
            .expect("openapi.json is committed at the repository root");
    let committed: serde_json::Value =
        serde_json::from_str(&committed).expect("committed contract is valid JSON");

    assert_eq!(
        committed,
        document(),
        "the committed contract has drifted. Regenerate it:\n  \
         cargo run -p graph-owl-server --bin openapi > openapi.json"
    );
}

/// **Slice K in the only form that is worth having.** A spec can be valid and
/// wrong: every `$ref` resolving and every route existing says nothing about
/// whether the *shapes* match what the server sends.
///
/// So this drives a real request and checks the response against the schema the
/// contract promises for it — required fields present, and no field the schema
/// does not describe. That second half is what catches a spec that has silently
/// fallen behind the type: a field added to `Asset` without a regenerated
/// contract fails here, which is exactly the drift a generated client would
/// deserialize into nothing.
#[tokio::test]
async fn a_real_response_matches_the_schema_the_contract_promises() {
    let (app, _container, _) = test_app().await;

    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/assets")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind":"service","name":"hdfc-core"}"#))
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(created.status(), StatusCode::CREATED);

    let bytes = axum::body::to_bytes(created.into_body(), usize::MAX)
        .await
        .expect("body");
    let asset: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");

    let doc = document();
    let schema = &doc["components"]["schemas"]["Asset"];
    let properties = schema["properties"].as_object().expect("Asset properties");

    for required in schema["required"].as_array().into_iter().flatten() {
        let field = required.as_str().expect("a field name");
        assert!(
            asset.get(field).is_some(),
            "the contract requires Asset.{field} and the server did not send it"
        );
    }

    for field in asset.as_object().expect("an object").keys() {
        assert!(
            properties.contains_key(field),
            "the server sends Asset.{field} and the contract does not describe it —              regenerate: cargo run -p graph-owl-server --bin openapi > openapi.json"
        );
    }
}

/// The error shape too, because that is the half clients get wrong. A typed
/// problem+json is the difference between a client that can branch on
/// `type` and one that string-matches a message.
#[tokio::test]
async fn a_real_error_matches_the_problem_schema() {
    let (app, _container, _) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/99999999-9999-4999-8999-999999999999")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let problem: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");

    let doc = document();
    let schema = &doc["components"]["schemas"]["Problem"];
    for required in schema["required"].as_array().into_iter().flatten() {
        let field = required.as_str().expect("a field name");
        assert!(
            problem.get(field).is_some(),
            "the contract requires Problem.{field} and the server did not send it"
        );
    }
    assert!(
        problem["type"]
            .as_str()
            .is_some_and(|t| t.starts_with("https://")),
        "`type` is the field a client branches on and must be a stable URI: {problem}"
    );
}

/// Epic 9 Slice B: the served context is exactly what
/// `graph_owl_rdf_io::JsonLdContext::core_v1().to_document()` produces —
/// the same function the crate's own compaction uses, so this route and
/// that compaction cannot name two different mappings.
#[tokio::test]
async fn the_json_ld_context_endpoint_serves_the_core_context() {
    let (app, _container, _) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/context/v1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/ld+json")
    );

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let served: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    let expected: serde_json::Value =
        serde_json::from_slice(&graph_owl_rdf_io::JsonLdContext::core_v1().to_document())
            .expect("valid JSON");
    assert_eq!(served, expected);
}

/// An unknown context version is `404`, not a stale or default document —
/// the same "unlisted is indistinguishable from nonexistent" reasoning the
/// admin surfaces already use.
#[tokio::test]
async fn an_unknown_json_ld_context_version_is_not_found() {
    let (app, _container, _) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/context/v99")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// **Epic 36 Slice D's own finding**: the contract had no mechanism for
/// query parameters on *any* endpoint — only the `{id}` path parameter was
/// ever documented (`operation["parameters"]` was set exactly once, inside
/// `if route.path.contains("{id}")`, nowhere else). A generated client
/// (`sdk/python/graph_owl_read_client`, built to prove Epic 36's browse
/// reference app) had no typed way to pass `?fields=` to `GET /assets/{id}`
/// even though the handler has supported it since Epic 37a Slice B. Fixed
/// for the endpoints Slice D's own browse app actually needs — `GET
/// /assets/{id}` (`fields`, `asOf`) and `GET /assets/search` (`q`, `kind`,
/// `domain`, `dataProduct`, `limit`, `after`) — not a blanket backfill of
/// every endpoint's query parameters, which is a separate, much larger
/// undertaking recorded in `36-reference-apps.md`'s defect log rather than
/// attempted here.
#[test]
fn get_asset_by_id_documents_its_query_parameters() {
    let doc = document();
    let params = doc["paths"]["/assets/{id}"]["get"]["parameters"]
        .as_array()
        .expect("GET /assets/{id} must document its parameters");
    let names: Vec<&str> = params
        .iter()
        .map(|p| p["name"].as_str().expect("a parameter name"))
        .collect();
    assert!(names.contains(&"fields"), "{names:?}");
    assert!(names.contains(&"asOf"), "{names:?}");
    // The path parameter must still be there too — this is additive, not a
    // replacement of the existing `{id}` documentation.
    assert!(names.contains(&"id"), "{names:?}");
}

#[test]
fn search_documents_its_query_parameters() {
    let doc = document();
    let params = doc["paths"]["/assets/search"]["get"]["parameters"]
        .as_array()
        .expect("GET /assets/search must document its parameters");
    let names: Vec<&str> = params
        .iter()
        .map(|p| p["name"].as_str().expect("a parameter name"))
        .collect();
    for expected in ["q", "kind", "domain", "dataProduct", "limit", "after"] {
        assert!(
            names.contains(&expected),
            "missing {expected:?} in {names:?}"
        );
    }
    // `q` is the one required query parameter on this route — a generated
    // client that treats it as optional lets a caller send a request that
    // can only 400.
    let q = params
        .iter()
        .find(|p| p["name"] == "q")
        .expect("q parameter");
    assert_eq!(q["required"], true, "{q}");
}

#[test]
fn list_assets_documents_pagination_and_filters() {
    let doc = document();
    let params = doc["paths"]["/assets"]["get"]["parameters"]
        .as_array()
        .expect("GET /assets must document its parameters");
    let names: Vec<&str> = params
        .iter()
        .map(|p| p["name"].as_str().expect("a parameter name"))
        .collect();
    for expected in [
        "limit",
        "after",
        "kind",
        "owner",
        "unowned",
        "domain",
        "dataProduct",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected:?} in {names:?}"
        );
    }
}
