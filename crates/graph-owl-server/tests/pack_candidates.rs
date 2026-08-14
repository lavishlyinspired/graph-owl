//! Plan 111 Slice D — `POST /packs/{pack}/candidates`, the route that finally
//! runs a pack's own `[[matching.blocking]]`.
//!
//! **`graph_owl_core::blocking_strategy` is 963 lines and 38 tests with, until
//! this slice, no callers anywhere in the workspace.** Both shipped packs have
//! declared blocking strategies since Epic 105 and nothing read them — so the
//! strategies that exist precisely to see through a typo were configuration
//! nobody executed.
//!
//! **Domain-neutral, and this file proves it rather than asserting it.** The
//! pack under test declares its own prefix, its own namespace and its own
//! field names; the handler resolves the prefix and the facade sees rendered
//! identifiers. Nothing in the route knows what the fields mean.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A pack whose vocabulary is invented for this test. If the mechanism only
/// worked for the shipped packs, that is exactly the failure the neutrality
/// rule exists to catch.
const PACK: &str = "planetest";
const NAMESPACE: &str = "https://graph-owl.dev/packs/planetest#";
const PREFIX: &str = "pt";

fn manifest() -> String {
    format!(
        r#"
[pack]
id = "{PACK}"
namespace = "{NAMESPACE}"
prefix = "{PREFIX}"
description = "A pack that exists to prove blocking is not GST-shaped."

[[matching.blocking]]
strategy = "normalized"
fields = ["{PREFIX}:partyName"]

[[matching.blocking]]
strategy = "ngram"
fields = ["{PREFIX}:registrationId"]
n = 3
"#
    )
}

/// Writes the pack manifest into a scratch directory and points the server's
/// pack loader at it for the duration of the process.
fn install_manifest() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("graph-owl-plan111-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join(PACK)).expect("pack directory");
    std::fs::write(base.join(PACK).join("pack.toml"), manifest()).expect("manifest");
    // SAFETY: the test binary is single-threaded at this point (each test sets
    // it before its first request) and every test in this file uses the same
    // value, so no test can observe another's.
    unsafe { std::env::set_var("GRAPH_OWL_PACKS_DIR", &base) };
    base
}

async fn declare_namespace(app: &axum::Router) -> u16 {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/namespaces")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "iri": NAMESPACE, "declaredBy": PACK }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    // `200` on a re-declare, `201` the first time — declaring a namespace is
    // idempotent so a pack can be reloaded, and this test does not care which
    // of the two it got.
    assert!(
        response.status().is_success(),
        "declare the namespace: {}",
        response.status()
    );
    u16::try_from(
        json_body(response).await["code"]
            .as_u64()
            .expect("a namespace code"),
    )
    .expect("a u16 code")
}

/// The pack's own predicates, registered before anything is asserted against
/// them — the graph refuses an undefined predicate, which is the check that
/// stops a typo becoming a new field nobody declared.
async fn declare_predicates(app: &axum::Router, namespace: u16) {
    for name in ["partyName", "registrationId"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/predicates")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "namespace": namespace, "name": name, "valueType": 1, "many": false })
                            .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert!(response.status().is_success(), "declare `{name}`");
    }
}

/// Turtle rather than a hand-built flake list: this is how a pack's data
/// actually arrives, and a fixture that bypasses the importer would prove the
/// blocking works on data no deployment ever has.
async fn import(app: &axum::Router, turtle: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/graph/import/rdf?source={PACK}&format=turtle"))
                .header("content-type", "text/turtle")
                .body(Body::from(turtle.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "import the fixture");
    // **The import reports per-subject rejections in a `200`.** Asserting only
    // the status would have let this test run against an empty graph and
    // report "no candidates" as a pass — which is exactly what it did until
    // the response body was read.
    assert!(
        body["rejected"].as_array().expect("rejected").is_empty(),
        "the fixture did not land: {body}",
    );
}

async fn candidates(app: &axum::Router, subject: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/packs/{PACK}/candidates"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "subject": subject }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

fn fixture() -> String {
    format!(
        r#"
@prefix pt: <{NAMESPACE}> .

pt:row-a pt:partyName "ACME  Ltd" ; pt:registrationId "27AAACR5055K1ZM" .
pt:row-b pt:partyName "acme ltd" ; pt:registrationId "09ZZZZZ0000Z9ZZ" .
pt:row-c pt:partyName "Contoso"  ; pt:registrationId "27AAACR5055K1MZ" .
pt:row-d pt:partyName "Fabrikam" ; pt:registrationId "44QQQQQ1111Q1QQ" .
"#
    )
}

/// **The whole slice in one assertion.** `row-b` shares a normalized party
/// name with `row-a`; `row-c` shares nothing but a transposed registration id
/// — the classic data-entry error — and only an n-gram key finds it. `row-d`
/// shares neither and must not appear.
#[tokio::test]
async fn a_packs_own_strategies_find_the_near_misses_and_nothing_else() {
    let _base = install_manifest();
    let (app, _container, _connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    declare_predicates(&app, code).await;
    import(&app, &fixture()).await;

    let (status, body) = candidates(&app, &format!("{code}:row-a")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let found = body["candidates"].as_array().expect("candidates array");
    let by_subject: std::collections::BTreeMap<String, Vec<String>> = found
        .iter()
        .map(|candidate| {
            (
                candidate["subject"].as_str().expect("subject").to_string(),
                candidate["by"]
                    .as_array()
                    .expect("by")
                    .iter()
                    .map(|s| s.as_str().expect("strategy").to_string())
                    .collect(),
            )
        })
        .collect();

    assert_eq!(
        by_subject.get(&format!("{code}:row-b")),
        Some(&vec!["normalized".to_string()]),
        "`ACME  Ltd` and `acme ltd` are one party: {body}",
    );
    assert_eq!(
        by_subject.get(&format!("{code}:row-c")),
        Some(&vec!["ngram".to_string()]),
        "a transposed registration id is exactly what the n-gram key is for: {body}",
    );
    assert!(
        !by_subject.contains_key(&format!("{code}:row-d")),
        "an unrelated record must not be a candidate — without this the test \
         above passes on a search that returns everything: {body}",
    );
    assert_eq!(body["truncated"], false);
}

/// A pack that does not exist declares no strategies, which is an empty answer
/// rather than an error — the same "absent rather than broken" posture every
/// other pack-reading surface in this server already takes.
#[tokio::test]
async fn an_unknown_pack_answers_empty_rather_than_failing() {
    let _base = install_manifest();
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/nosuchpack/candidates")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "subject": "1:anything" }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(
        body["candidates"].as_array().expect("array").is_empty(),
        "{body}"
    );
}

/// A body with no subject is a `400` naming the field, in the RFC 9457 shape
/// every other route here uses.
#[tokio::test]
async fn a_request_with_no_subject_is_rejected_by_name() {
    let _base = install_manifest();
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/packs/{PACK}/candidates"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "limit": 5 }).to_string()))
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
            .any(|e| e["field"] == "subject"),
        "{body}"
    );
}
