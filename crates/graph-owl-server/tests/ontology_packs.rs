//! Epic 33 at the wire — domain ontology packs.
//!
//! The SKOS fixture is a small, synthetic document (three concepts, one
//! `broader` edge) rather than any real vendor vocabulary — packs are
//! explicitly not vendored into this repo (`plans/33-ontology-packs.md`
//! decision 1), and that applies to test fixtures too.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(ct) = content_type {
        request = request.header("content-type", ct);
    }
    let request = request
        .body(Body::from(body))
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
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, parsed)
}

async fn json_send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    match body {
        Some(v) => {
            send(
                app,
                method,
                uri,
                Some("application/json"),
                v.to_string().into_bytes(),
            )
            .await
        }
        None => send(app, method, uri, None, Vec::new()).await,
    }
}

fn fixture_v1() -> Vec<u8> {
    r#"
    @prefix skos: <http://www.w3.org/2004/02/skos/core#> .
    <http://ex.org/fin#Asset> skos:prefLabel "Asset" .
    <http://ex.org/fin#FinancialInstrument> skos:prefLabel "Financial Instrument" ;
        skos:broader <http://ex.org/fin#Asset> .
    <http://ex.org/fin#Loan> skos:prefLabel "Loan" ;
        skos:definition "A sum of money lent" ;
        skos:altLabel "Credit" ;
        skos:broader <http://ex.org/fin#FinancialInstrument> .
    "#
    .as_bytes()
    .to_vec()
}

/// Adds `Bond` and drops `Asset`, relative to [`fixture_v1`] — an add, a
/// removal, and (via `Loan`'s untouched content) an unchanged concept, all
/// in one document.
fn fixture_v2() -> Vec<u8> {
    r#"
    @prefix skos: <http://www.w3.org/2004/02/skos/core#> .
    <http://ex.org/fin#FinancialInstrument> skos:prefLabel "Financial Instrument" .
    <http://ex.org/fin#Loan> skos:prefLabel "Loan" ;
        skos:definition "A sum of money lent" ;
        skos:altLabel "Credit" ;
        skos:broader <http://ex.org/fin#FinancialInstrument> .
    <http://ex.org/fin#Bond> skos:prefLabel "Bond" ;
        skos:broader <http://ex.org/fin#FinancialInstrument> .
    "#
    .as_bytes()
    .to_vec()
}

fn import_uri(pack_id: &str, version: &str, licence_kind: &str, extra: &str) -> String {
    format!(
        "/ontology-packs?packId={pack_id}&version={version}&sourceUrl=http://ex.org/source\
         &licenceKind={licence_kind}&licenceName=Test{extra}"
    )
}

async fn import(
    app: &axum::Router,
    pack_id: &str,
    version: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &import_uri(pack_id, version, "permissive", ""),
        Some("text/turtle"),
        body,
    )
    .await
}

#[tokio::test]
async fn importing_a_pack_creates_terms_with_hierarchy_intact() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    assert_eq!(status, StatusCode::CREATED, "{pack}");
    assert_eq!(pack["packId"], "fin");
    assert_eq!(pack["termCount"], json!(3));

    let pack_id = pack["id"].as_str().expect("an id").to_string();
    let (status, terms) = json_send(
        &app,
        "GET",
        &format!("/ontology-packs/{pack_id}/terms"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{terms}");
    let terms = terms.as_array().expect("array");
    assert_eq!(terms.len(), 3);

    let loan = terms
        .iter()
        .find(|t| t["sourceIri"] == "http://ex.org/fin#Loan")
        .expect("loan term");
    assert_eq!(loan["term"]["definition"], "A sum of money lent");
    assert_eq!(loan["term"]["synonyms"], json!(["Credit"]));
    assert_eq!(loan["term"]["status"], "approved");
    assert_eq!(loan["effective"]["overridden"], false);
}

#[tokio::test]
async fn reimporting_the_same_version_is_a_no_op() {
    let (app, _db, _connection_string) = test_app().await;

    let (_, first) = import(&app, "fin", "1.0", fixture_v1()).await;
    let (status, second) = import(&app, "fin", "1.0", fixture_v1()).await;

    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(first["id"], second["id"], "must return the original pack");

    let (_, terms) = json_send(
        &app,
        "GET",
        &format!("/ontology-packs/{}/terms", first["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(
        terms.as_array().unwrap().len(),
        3,
        "a re-import must not duplicate terms"
    );
}

#[tokio::test]
async fn malformed_turtle_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &import_uri("fin", "1.0", "permissive", ""),
        Some("text/turtle"),
        b"this is not turtle at all {{{".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_licence_required_pack_without_acknowledgement_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &import_uri(
            "fin",
            "1.0",
            "licenceRequired",
            "&licenceContact=licensing@ex.org",
        ),
        Some("text/turtle"),
        fixture_v1(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_licence_required_pack_with_acknowledgement_succeeds() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &import_uri(
            "fin",
            "1.0",
            "licenceRequired",
            "&licenceContact=licensing@ex.org&acknowledgeLicence=true",
        ),
        Some("text/turtle"),
        fixture_v1(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn creating_and_removing_an_override_changes_the_effective_term() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    let pack_id = pack["id"].as_str().unwrap().to_string();

    let (status, override_) = json_send(
        &app,
        "POST",
        &format!("/ontology-packs/{pack_id}/overrides"),
        Some(json!({
            "termPath": "http://ex.org/fin#Loan",
            "kind": "redefine",
            "payload": { "definition": "our house definition" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{override_}");
    let override_id = override_["id"].as_str().unwrap().to_string();

    let (_, terms) = json_send(
        &app,
        "GET",
        &format!("/ontology-packs/{pack_id}/terms"),
        None,
    )
    .await;
    let loan = terms
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["sourceIri"] == "http://ex.org/fin#Loan")
        .unwrap();
    assert_eq!(loan["effective"]["definition"], "our house definition");
    assert_eq!(loan["effective"]["overridden"], true);
    // The pack's own stored content must be untouched — only the *effective*
    // projection changes.
    assert_eq!(loan["term"]["definition"], "A sum of money lent");

    let (status, _) = json_send(
        &app,
        "DELETE",
        &format!("/ontology-packs/{pack_id}/overrides/{override_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, terms) = json_send(
        &app,
        "GET",
        &format!("/ontology-packs/{pack_id}/terms"),
        None,
    )
    .await;
    let loan = terms
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["sourceIri"] == "http://ex.org/fin#Loan")
        .unwrap();
    assert_eq!(
        loan["effective"]["definition"], "A sum of money lent",
        "removing the override must restore the pack's own value"
    );
    assert_eq!(loan["effective"]["overridden"], false);
}

#[tokio::test]
async fn a_dry_run_upgrade_reports_without_writing_anything() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    let pack_id = pack["id"].as_str().unwrap().to_string();

    let (status, result) = send(
        &app,
        "POST",
        &format!("/ontology-packs/{pack_id}/upgrade?version=2.0&dryRun=true"),
        Some("text/turtle"),
        fixture_v2(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["applied"], false);
    assert_eq!(result["report"]["added"], json!(["http://ex.org/fin#Bond"]));
    assert_eq!(
        result["report"]["removed"],
        json!(["http://ex.org/fin#Asset"])
    );

    let (_, refetched) = json_send(&app, "GET", &format!("/ontology-packs/{pack_id}"), None).await;
    assert_eq!(refetched["version"], "1.0", "a dry run must not write");
}

#[tokio::test]
async fn upgrading_applies_the_diff() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    let pack_id = pack["id"].as_str().unwrap().to_string();

    let (status, result) = send(
        &app,
        "POST",
        &format!("/ontology-packs/{pack_id}/upgrade?version=2.0"),
        Some("text/turtle"),
        fixture_v2(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["applied"], true);

    let (_, refetched) = json_send(&app, "GET", &format!("/ontology-packs/{pack_id}"), None).await;
    assert_eq!(refetched["version"], "2.0");
    assert_eq!(
        refetched["termCount"],
        json!(3),
        "Bond was added on top of Asset,FinancialInstrument,Loan minus the removed Asset"
    );

    let (_, terms) = json_send(
        &app,
        "GET",
        &format!("/ontology-packs/{pack_id}/terms"),
        None,
    )
    .await;
    let terms = terms.as_array().unwrap();
    let bond = terms
        .iter()
        .find(|t| t["sourceIri"] == "http://ex.org/fin#Bond")
        .expect("bond must have been added");
    assert_eq!(bond["term"]["status"], "approved");

    let asset = terms
        .iter()
        .find(|t| t["sourceIri"] == "http://ex.org/fin#Asset")
        .expect("asset must still be present, deprecated rather than deleted");
    assert_eq!(asset["term"]["status"], "deprecated");
}

#[tokio::test]
async fn removal_without_force_reports_and_leaves_the_pack_in_place() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    let pack_id = pack["id"].as_str().unwrap().to_string();

    let (status, report) =
        json_send(&app, "DELETE", &format!("/ontology-packs/{pack_id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["termCount"], json!(3));

    let (status, _) = json_send(&app, "GET", &format!("/ontology-packs/{pack_id}"), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the pack must still exist without force"
    );
}

#[tokio::test]
async fn removal_with_force_deletes_the_pack() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack) = import(&app, "fin", "1.0", fixture_v1()).await;
    let pack_id = pack["id"].as_str().unwrap().to_string();

    let (status, _) = json_send(
        &app,
        "DELETE",
        &format!("/ontology-packs/{pack_id}?force=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_send(&app, "GET", &format!("/ontology-packs/{pack_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn removal_is_blocked_when_another_pack_exact_matches_a_term_in_it() {
    let (app, _db, _connection_string) = test_app().await;
    let (_, pack_a) = import(&app, "fin-a", "1.0", fixture_v1()).await;

    let pack_b_doc = r#"
    @prefix skos: <http://www.w3.org/2004/02/skos/core#> .
    <http://ex.org/finb#Loan> skos:prefLabel "Loan (B)" ;
        skos:exactMatch <http://ex.org/fin#Loan> .
    "#
    .as_bytes()
    .to_vec();
    let (status, _pack_b) = import(&app, "fin-b", "1.0", pack_b_doc).await;
    assert_eq!(status, StatusCode::CREATED);

    let pack_a_id = pack_a["id"].as_str().unwrap().to_string();
    let (status, body) = json_send(
        &app,
        "DELETE",
        &format!("/ontology-packs/{pack_a_id}?force=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, _) = json_send(&app, "GET", &format!("/ontology-packs/{pack_a_id}"), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a refused removal must not have deleted anything"
    );
}
