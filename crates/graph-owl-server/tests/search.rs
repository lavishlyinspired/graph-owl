//! Epic 8 Slice A: search is full-text and ranked, not a substring scan.
//!
//! Every test here has a negative half. A search that matches everything and a
//! search that matches nothing both look like a working search from the
//! positive assertion alone — the first because the target is present, the
//! second only when the target happens to be absent.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

async fn create(app: &axum::Router, body: Value) -> Value {
    let (status, created) = send(app, "POST", "/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created
}

/// A service, a database, a schema, and three tables — enough for relevance to
/// have something to order.
async fn estate(app: &axum::Router) {
    let service = create(app, json!({ "kind": "service", "name": "hdfc-core" })).await;
    let database = create(
        app,
        json!({ "kind": "database", "name": "retail", "parentId": service["id"] }),
    )
    .await;
    let schema = create(
        app,
        json!({ "kind": "schema", "name": "payments", "parentId": database["id"] }),
    )
    .await;

    for (name, description) in [
        (
            "upi_transactions",
            "Every UPI payment settled through NPCI.",
        ),
        ("card_settlements", "Nightly card settlement batches."),
        (
            "audit_log",
            "Who changed which transactions, and when they did it.",
        ),
    ] {
        create(
            app,
            json!({
                "kind": "table",
                "name": name,
                "parentId": schema["id"],
                "description": description
            }),
        )
        .await;
    }
}

fn names(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .map(|a| a["name"].as_str().expect("a name").to_string())
        .collect()
}

async fn search(app: &axum::Router, q: &str) -> Vec<String> {
    let (status, page) = send(app, "GET", &format!("/assets/search?q={q}&limit=50"), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    names(&page)
}

/// The point of the epic: a word in a description is findable. `LIKE` over name
/// and FQN could not do this at all.
#[tokio::test]
async fn a_table_is_findable_by_a_word_only_its_description_contains() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    let hits = search(&app, "NPCI").await;

    assert!(
        hits.contains(&"upi_transactions".to_string()),
        "described tables must be findable: {hits:?}"
    );
    // And the negative: a word in *no* description finds nothing. Without this,
    // a query matching every row would satisfy the assertion above.
    assert!(
        search(&app, "swift").await.is_empty(),
        "a term nothing mentions must return nothing"
    );
}

/// Prefix matching, which is what makes a search box usable while typing.
#[tokio::test]
async fn a_partial_word_finds_the_table_before_it_is_fully_typed() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    assert!(
        search(&app, "transa")
            .await
            .contains(&"upi_transactions".to_string())
    );
    // The negative: prefix matching is a *prefix*, not a substring. Matching
    // mid-word would make every three-letter query return most of the estate.
    assert!(
        !search(&app, "ansactions")
            .await
            .contains(&"upi_transactions".to_string()),
        "a suffix is not a prefix"
    );
}

/// Identifiers are split on the separators identifiers actually use, so the
/// index holds the words a person would type.
#[tokio::test]
async fn an_identifier_is_findable_by_either_of_its_parts() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    for term in ["upi", "transactions"] {
        assert!(
            search(&app, term)
                .await
                .contains(&"upi_transactions".to_string()),
            "{term} should find upi_transactions"
        );
    }
}

/// Every term narrows. Typing a second word that no single asset satisfies must
/// return nothing rather than the union — otherwise the search box rewards
/// vagueness and a longer query returns more results than a shorter one.
#[tokio::test]
async fn terms_are_anded_so_a_second_word_narrows_rather_than_widens() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    let one = search(&app, "settlement").await;
    assert!(one.contains(&"card_settlements".to_string()), "{one:?}");

    let two = search(&app, "settlement%20upi").await;
    assert!(
        two.is_empty(),
        "no asset carries both terms, so the conjunction is empty: {two:?}"
    );
}

/// **Relevance, asserted as an order and not as membership.** A name match
/// outranks a description match; both are returned, so only the ordering can
/// distinguish a ranked search from an unranked one.
#[tokio::test]
async fn a_name_match_outranks_a_description_match() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    let hits = search(&app, "transactions").await;

    assert!(
        hits.len() >= 2,
        "both the named table and the described one must be present, \
         or the ordering assertion below proves nothing: {hits:?}"
    );
    assert_eq!(
        hits.first().map(String::as_str),
        Some("upi_transactions"),
        "the table *called* transactions outranks the one merely mentioning them: {hits:?}"
    );
    assert!(
        hits.contains(&"audit_log".to_string()),
        "the description match is still a hit, just a lower one: {hits:?}"
    );
}

/// An unusable query is an empty result, not a 500. `to_tsquery('english', '')`
/// raises a syntax error, so an all-punctuation search has to be answered
/// without asking Postgres at all.
#[tokio::test]
async fn a_query_with_no_searchable_terms_is_an_empty_result_not_an_error() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    let (status, page) = send(&app, "GET", "/assets/search?q=%21%21%21&limit=50", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(names(&page).is_empty(), "{page}");
}

/// Operators typed into the search box are separators, never operators. `a & b`
/// as a raw tsquery is valid and means something else; `!x` inverts the user's
/// intent; `x:` is a syntax error surfacing as a broken search.
#[tokio::test]
async fn tsquery_operators_typed_by_a_user_do_not_reach_the_query_language() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    for typed in ["%21upi", "upi%20%26%20transactions", "%28upi%29"] {
        let (status, page) = send(
            &app,
            "GET",
            &format!("/assets/search?q={typed}&limit=50"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{typed} produced {page}");
        assert!(
            names(&page).contains(&"upi_transactions".to_string()),
            "{typed} should still find the table: {page}"
        );
    }
}

/// A relevance-ordered page has to paginate by relevance. Paging with the FQN
/// cursor of an alphabetical query would silently skip and repeat rows.
#[tokio::test]
async fn a_ranked_result_pages_without_skipping_or_repeating() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    // `core` reaches every asset through its FQN, so the corpus is larger than
    // one page. A stopword would not: `to_tsquery('english', 'a:*')` is an
    // empty query, and the test would pass vacuously on an empty result.
    let (_, first) = send(&app, "GET", "/assets/search?q=core&limit=2", None).await;
    let (_, everything) = send(&app, "GET", "/assets/search?q=core&limit=50", None).await;
    let all = names(&everything);
    assert!(all.len() > 2, "the corpus must exceed one page: {all:?}");

    let Some(cursor) = first["paging"]["after"].as_str() else {
        panic!("a corpus larger than the page must offer a cursor: {first}");
    };
    let (status, second) = send(
        &app,
        "GET",
        &format!("/assets/search?q=core&limit=2&after={cursor}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");

    let paged: Vec<String> = names(&first).into_iter().chain(names(&second)).collect();
    assert_eq!(
        paged,
        all.iter().take(paged.len()).cloned().collect::<Vec<_>>(),
        "paging must reproduce the unpaged order exactly"
    );
}

/// **Epic 34 Slice F's own search RED test.** The `kind` facet is computed
/// from `asset.kind.as_str()` over whatever the visible page contains — no
/// per-kind branch, so it never needed to be taught about the five families
/// this epic added. One search across six root-level kinds, from five
/// different families plus the original `service`, proves the facet counts
/// them all correctly rather than only the kinds it was written against.
#[tokio::test]
async fn search_facets_by_kind_span_every_family() {
    let (app, _container, _) = test_app().await;
    let roots = [
        ("service", "acme-warehouse-root"),
        ("dashboardService", "acme-looker-root"),
        ("messagingService", "acme-kafka-root"),
        ("pipelineService", "acme-airflow-root"),
        ("mlModelService", "acme-sagemaker-root"),
        ("storageService", "acme-s3-root"),
    ];
    for (kind, name) in roots {
        create(&app, json!({ "kind": kind, "name": name })).await;
    }

    let (status, result) = send(&app, "GET", "/assets/search?q=acme&limit=50", None).await;
    assert_eq!(status, StatusCode::OK, "{result}");

    let data = result["data"].as_array().expect("a page");
    assert_eq!(data.len(), roots.len(), "{result}");

    let by_kind = result["facets"]["kind"].as_array().expect("a kind facet");
    for (kind, _) in roots {
        let count = by_kind
            .iter()
            .find(|entry| entry["value"] == kind)
            .and_then(|entry| entry["count"].as_u64());
        assert_eq!(
            count,
            Some(1),
            "missing or wrong count for `{kind}`: {result}"
        );
    }
}

// ── Snippets — Phase 2.4 of plans/EPIC-COMPLETION-PLAN.md ──────────────────

async fn search_data(app: &axum::Router, q: &str) -> Value {
    let (status, page) = send(app, "GET", &format!("/assets/search?q={q}&limit=50"), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["data"].clone()
}

fn hit_named<'a>(data: &'a Value, name: &str) -> &'a Value {
    data.as_array()
        .expect("an array")
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or_else(|| panic!("`{name}` must be in the results: {data}"))
}

/// **The point of the epic's own snippet criterion**: the excerpt names why
/// this row matched, not just that it did — a bare hit list forces a reader
/// to open every row to find the word they searched for.
#[tokio::test]
async fn a_snippet_is_returned_around_the_matched_word() {
    let (app, _container, _) = test_app().await;
    estate(&app).await;

    let data = search_data(&app, "NPCI").await;

    let hit = hit_named(&data, "upi_transactions");
    let snippet = hit["snippet"].as_str().expect("a snippet");
    assert!(
        snippet.contains("NPCI"),
        "the matched word must appear in its own snippet: {snippet}"
    );
}

/// And the negative: a match on `name` alone, with nothing in `description`
/// for `ts_headline` to excerpt, must report `null` rather than an empty or
/// misleading string — the same "absent means absent" rule
/// `CertificationStatus::None` already follows for a different field.
#[tokio::test]
async fn a_name_only_match_has_no_snippet() {
    let (app, _container, _) = test_app().await;
    create(
        &app,
        json!({ "kind": "service", "name": "nps-unique-root" }),
    )
    .await;

    let data = search_data(&app, "nps").await;

    let hit = hit_named(&data, "nps-unique-root");
    assert!(
        hit["snippet"].is_null(),
        "nothing to excerpt from an empty description: {hit}"
    );
}
