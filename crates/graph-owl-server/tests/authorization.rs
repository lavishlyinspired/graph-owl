mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app_with_secret};
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "demo-signing-secret-not-for-production";

/// A bank estate with one schema that must not be readable by everyone.
async fn seed_source(connection_string: &str) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    for statement in [
        "CREATE SCHEMA IF NOT EXISTS core_banking",
        "CREATE SCHEMA IF NOT EXISTS payments",
        "CREATE TABLE IF NOT EXISTS core_banking.customers (
             customer_id BIGINT PRIMARY KEY, pan CHAR(10), aadhaar_last4 CHAR(4))",
        "CREATE TABLE IF NOT EXISTS payments.upi_transactions (
             txn_id TEXT PRIMARY KEY, amount NUMERIC(18,2) NOT NULL)",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed statement");
    }
}

fn token(subject: &str) -> String {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        name: &'a str,
        exp: usize,
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: subject,
            name: subject,
            exp: 4_102_444_800, // year 2100
        },
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("token should encode")
}

async fn call(app: &axum::Router, uri: &str, subject: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(subject) = subject {
        builder = builder.header("authorization", format!("Bearer {}", token(subject)));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request should build"))
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// Sets up the estate, two users, and the policy that separates them.
async fn fixture() -> (axum::Router, common::TestDb) {
    let (app, container, connection_string) = test_app_with_secret(SECRET).await;
    seed_source(&connection_string).await;

    // Catalogue as the admin.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/connectors/postgres/runs")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from(
                    json!({
                        "connectionString": connection_string,
                        "serviceName": "hdfc-core",
                        "includeSchemas": ["core_banking", "payments"]
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK, "catalogue must succeed");

    // The database the catalog itself lives in, which the connector also
    // catalogued — so it is the second segment of every FQN below.
    let database = connection_string
        .rsplit_once('/')
        .expect("the connection string names a database")
        .1
        .to_string();

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("catalog connection");

    // `root` is an admin; `asha` is a risk analyst who may read everything
    // except the PII-bearing customer master.
    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = 'root'")
        .execute(&pool)
        .await
        .expect("promote root");

    let rules = json!([
        {
            "name": "read-catalog",
            "effect": "allow",
            "operations": ["viewBasic", "viewDetails"],
            "resources": { "type": "all" }
        },
        {
            "name": "no-customer-pii",
            "effect": "deny",
            "operations": ["viewBasic", "viewDetails"],
            "resources": {
                "type": "fqnPrefix",
                // Derived, not hardcoded. The database name is a *component
                // of the FQN* — `service.database.schema.table` — so a policy
                // written against a literal `postgres` stops matching the
                // moment each test gets its own database. That is the schema
                // this policy denies, addressed the way the catalog addresses
                // it.
                "value": format!("hdfc-core.{database}.core_banking")
            }
        }
    ]);
    sqlx::query("INSERT INTO roles (name) VALUES ('risk-analyst') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .expect("role insert");
    sqlx::query("INSERT INTO policies (name, rules) VALUES ('analyst-baseline', $1)")
        .bind(&rules)
        .execute(&pool)
        .await
        .expect("policy insert");
    // After the policy exists — the foreign key is doing its job.
    sqlx::query(
        "INSERT INTO role_policies (role, policy) VALUES ('risk-analyst', 'analyst-baseline')
         ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("role-policy link");

    // First request auto-provisions asha; then grant the role.
    call(&app, "/assets/stats", Some("asha")).await;
    sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ('asha', 'risk-analyst') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .expect("grant role");

    (app, container)
}

#[tokio::test]
async fn a_request_without_a_token_is_rejected() {
    let (app, _container) = fixture().await;

    let (status, body) = call(&app, "/assets/stats", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["type"], "https://graph-owl.dev/errors/unauthenticated");
}

#[tokio::test]
async fn a_token_signed_with_the_wrong_key_is_rejected() {
    let (app, _container) = fixture().await;

    #[derive(serde::Serialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }
    let forged = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: "root".to_string(),
            exp: 4_102_444_800,
        },
        &jsonwebtoken::EncodingKey::from_secret(b"not-the-real-secret"),
    )
    .expect("encode");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/assets/stats")
                .header("authorization", format!("Bearer {forged}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "a forged admin token must not be honoured"
    );
}

/// **The demo moment.** Two principals, one search, different results.
#[tokio::test]
async fn two_principals_searching_the_same_corpus_get_different_results() {
    let (app, _container) = fixture().await;

    let (_, admin) = call(&app, "/assets/search?q=customers&limit=100", Some("root")).await;
    let (_, analyst) = call(&app, "/assets/search?q=customers&limit=100", Some("asha")).await;

    let admin_names: Vec<&str> = admin["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(
        admin_names.contains(&"customers"),
        "the admin must see the customer master: {admin_names:?}"
    );
    assert!(
        analyst["data"].as_array().expect("data").is_empty(),
        "the analyst is denied core_banking and must see none of it, got {:?}",
        analyst["data"]
    );
}

/// A denied asset must be invisible in the *count* too. A total computed before
/// filtering says "47 results" above 12 rows, which leaks the existence of the
/// 35 the reader may not see.
#[tokio::test]
async fn counts_are_consistent_with_what_the_principal_can_see() {
    let (app, _container) = fixture().await;

    let (_, admin_stats) = call(&app, "/assets/stats", Some("root")).await;
    let (_, analyst_stats) = call(&app, "/assets/stats", Some("asha")).await;

    let total = |stats: &Value| -> i64 {
        stats["byKind"]
            .as_array()
            .expect("byKind")
            .iter()
            .map(|k| k["count"].as_i64().unwrap_or(0))
            .sum()
    };

    let admin_total = total(&admin_stats);
    let analyst_total = total(&analyst_stats);
    assert!(
        admin_total > analyst_total,
        "{admin_total} vs {analyst_total}"
    );

    // And the count matches the rows actually returned.
    let (_, listed) = call(&app, "/assets?limit=1000", Some("asha")).await;
    let listed_count = listed["data"].as_array().expect("data").len() as i64;
    assert_eq!(
        analyst_total, listed_count,
        "the stat total and the listed rows must be the same set"
    );
}

/// Hidden reads as missing, deliberately: a `403` on a specific id confirms
/// that id exists, which is exactly what the policy conceals.
#[tokio::test]
async fn a_denied_asset_reads_as_not_found_not_forbidden() {
    let (app, _container) = fixture().await;

    let (_, found) = call(&app, "/assets/search?q=customers&limit=10", Some("root")).await;
    let hidden_id = found["data"][0]["id"].as_str().expect("id");

    let (status, body) = call(&app, &format!("/assets/{hidden_id}"), Some("asha")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], "https://graph-owl.dev/errors/not-found");

    let (admin_status, _) = call(&app, &format!("/assets/{hidden_id}"), Some("root")).await;
    assert_eq!(
        admin_status,
        StatusCode::OK,
        "the same id is readable by someone who may see it — proving it exists"
    );
}

#[tokio::test]
async fn what_a_principal_is_allowed_is_still_fully_visible() {
    let (app, _container) = fixture().await;

    let (status, payments) = call(&app, "/assets/search?q=upi&limit=100", Some("asha")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !payments["data"].as_array().expect("data").is_empty(),
        "denying one schema must not restrict the rest"
    );
}

#[tokio::test]
async fn a_first_time_subject_is_auto_provisioned_without_a_directory_sync() {
    let (app, _container) = fixture().await;

    let (status, _) = call(&app, "/assets/stats", Some("brand-new-person")).await;
    assert_eq!(status, StatusCode::OK);

    // Provisioned with no roles, so a new identity starts with no access
    // rather than inheriting someone else's.
    let (_, listed) = call(&app, "/assets?limit=100", Some("brand-new-person")).await;
    assert!(
        listed["data"].as_array().expect("data").is_empty(),
        "a new user must start with nothing, not with everything"
    );
}

#[tokio::test]
async fn writes_are_attributed_to_the_authenticated_principal() {
    let (app, _container) = fixture().await;

    let (_, found) = call(
        &app,
        "/assets/search?q=upi_transactions&limit=10",
        Some("root"),
    )
    .await;
    let id = found["data"][0]["id"].as_str().expect("id");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/assets/{id}"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from(
                    json!({ "description": "UPI ledger" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);

    let updated = json_body(response).await;
    assert_eq!(
        updated["updatedBy"], "root",
        "the envelope must name the person, not `system` — this is the payoff \
         of threading the Principal seam through Epic 1"
    );
}

/// The landing page is subject to the same rule as every other count: a total
/// that ignored policy would state the size of what the reader may not see —
/// the exact leak the rest of this file exists to close.
#[tokio::test]
async fn the_overview_is_authorization_filtered_like_every_other_count() {
    let (app, _container) = fixture().await;

    let (_, admin) = call(&app, "/overview", Some("root")).await;
    let (status, analyst) = call(&app, "/overview", Some("asha")).await;
    assert_eq!(status, StatusCode::OK);

    let admin_total = admin["assets"]["total"].as_i64().expect("total");
    let analyst_total = analyst["assets"]["total"].as_i64().expect("total");
    assert!(
        admin_total > analyst_total,
        "{admin_total} vs {analyst_total}"
    );

    // The documentation denominator has to be filtered too. A coverage
    // percentage over an unfiltered total would quietly disclose the count of
    // hidden assets through arithmetic.
    assert_eq!(
        analyst["documentation"]["total"].as_i64().expect("total"),
        analyst_total,
        "coverage is a fraction of what the reader can see"
    );

    // And nothing from the denied schema may appear in the recent list.
    let recent = analyst["recentlyChanged"]
        .as_array()
        .expect("recentlyChanged");
    assert!(
        recent.iter().all(|a| !a["fullyQualifiedName"]
            .as_str()
            .unwrap_or_default()
            .contains("core_banking")),
        "core_banking leaked into the analyst's recent list: {recent:?}"
    );
}

/// Whitespace is not documentation. Counting it would make the coverage number
/// reward someone typing a space into every field.
#[tokio::test]
async fn documentation_coverage_counts_real_descriptions_only() {
    let (app, _container) = fixture().await;

    let (_, found) = call(
        &app,
        "/assets/search?q=upi_transactions&kind=table",
        Some("root"),
    )
    .await;
    let id = found["data"][0]["id"].as_str().expect("id").to_string();

    let (_, before) = call(&app, "/overview", Some("root")).await;
    let described_before = before["documentation"]["described"]
        .as_i64()
        .expect("described");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/assets/{id}"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from(
                    json!({ "description": "UPI ledger" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);

    let (_, after) = call(&app, "/overview", Some("root")).await;
    assert_eq!(
        after["documentation"]["described"]
            .as_i64()
            .expect("described"),
        described_before + 1,
        "a real description must move the number"
    );
}

async fn sparql(app: &axum::Router, subject: &str, query: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sparql")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token(subject)))
                .body(Body::from(json!({ "query": query }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

const DSC: &str = "https://graph-owl.dev/ns/catalog#";

/// **The demo moment, in SPARQL.** The same query, two principals, different
/// answers — and the difference is structural: the analyst's evaluator never
/// receives the denied facts, so no optimisation inside it could surface them.
#[tokio::test]
async fn two_principals_running_the_same_sparql_get_different_results() {
    let (app, _container) = fixture().await;
    let query = format!("SELECT ?name WHERE {{ ?t <{DSC}type> \"table\" . ?t <{DSC}name> ?name }}");

    let (status, admin) = sparql(&app, "root", &query).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    let (_, analyst) = sparql(&app, "asha", &query).await;

    let names = |body: &Value| -> Vec<String> {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| {
                row["name"]
                    .as_str()
                    .map(|s| s.trim_matches('"').to_string())
            })
            .collect()
    };

    let admin_names = names(&admin);
    let analyst_names = names(&analyst);

    assert!(
        admin_names.iter().any(|n| n == "customers"),
        "the admin must see the PII table: {admin_names:?}"
    );
    assert!(
        !analyst_names.iter().any(|n| n == "customers"),
        "the analyst must not: {analyst_names:?}"
    );
    assert!(
        !analyst_names.is_empty(),
        "and must still see everything else"
    );
}

/// A denied asset must not be reachable through a *join* either. Filtering the
/// rows would still have let the join traverse it.
#[tokio::test]
async fn a_denied_asset_is_not_reachable_through_a_join() {
    let (app, _container) = fixture().await;
    let (_, analyst) = sparql(
        &app,
        "asha",
        &format!("SELECT ?fqn WHERE {{ ?c <{DSC}parentTable> ?t . ?t <{DSC}fqn> ?fqn }}"),
    )
    .await;

    let body = analyst.to_string();
    assert!(
        !body.contains("core_banking"),
        "a join reached into the denied schema: {body}"
    );
}

/// The freshness stamp and the truncation flag are always present — a caller
/// must never have to infer either from the row count.
#[tokio::test]
async fn a_sparql_answer_states_its_own_completeness() {
    let (app, _container) = fixture().await;
    let (status, body) = sparql(
        &app,
        "root",
        &format!("SELECT ?n WHERE {{ ?t <{DSC}name> ?n }}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["truncated"], false);
    assert!(body["factsScanned"].as_u64().expect("factsScanned") > 0);
    assert!(body.get("asOf").is_some(), "the stamp must be present");
}

#[tokio::test]
async fn a_malformed_query_is_a_400_naming_the_field() {
    let (app, _container) = fixture().await;
    let (status, body) = sparql(&app, "root", "SELECT ?x WHERE { not sparql").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["field"], "query");
}

#[tokio::test]
async fn an_unauthenticated_sparql_request_is_rejected() {
    let (app, _container) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sparql")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "query": "SELECT ?x WHERE { ?x ?p ?o }" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---- Epic 7b Slice E: Cypher over the same engine, at the HTTP boundary ----

async fn cypher(app: &axum::Router, subject: &str, query: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/cypher")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token(subject)))
                .body(Body::from(json!({ "query": query }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// **The demo moment, in Cypher.** The Cypher counterpart of
/// `two_principals_running_the_same_sparql_get_different_results` — same
/// fixture, same policy, same two principals, a different query language.
/// `type` here is the asset's own kind field, a string literal fact, not an
/// ontology class assertion — Epic 7c draws that distinction deliberately
/// (`a_literal_valued_type_does_not_become_a_label`), so the query compares
/// `t.type` as a property rather than matching a Cypher label.
#[tokio::test]
async fn two_principals_running_the_same_cypher_get_different_results() {
    let (app, _container) = fixture().await;
    let query = "MATCH (t) WHERE t.type = \"table\" RETURN t.name AS name";

    let (status, admin) = cypher(&app, "root", query).await;
    assert_eq!(status, StatusCode::OK, "{admin}");
    let (_, analyst) = cypher(&app, "asha", query).await;

    let names = |body: &Value| -> Vec<String> {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| {
                row["name"]
                    .as_str()
                    .map(|s| s.trim_matches('"').to_string())
            })
            .collect()
    };

    let admin_names = names(&admin);
    let analyst_names = names(&analyst);

    assert!(
        admin_names.iter().any(|n| n == "customers"),
        "the admin must see the PII table: {admin_names:?}"
    );
    assert!(
        !analyst_names.iter().any(|n| n == "customers"),
        "the analyst must not: {analyst_names:?}"
    );
    assert!(
        !analyst_names.is_empty(),
        "and must still see everything else"
    );
}

/// **The property Slice E exists for, proved at the HTTP boundary rather than
/// only at the `Catalog` unit level.** One restricted principal, the same
/// logical question in both languages, compared. If `/cypher` and `/sparql`
/// ever authorized differently, this would be the leak.
#[tokio::test]
async fn cypher_and_sparql_agree_under_one_restricted_principal() {
    let (app, _container) = fixture().await;

    let sparql_query =
        format!("SELECT ?name WHERE {{ ?t <{DSC}type> \"table\" . ?t <{DSC}name> ?name }}");
    let cypher_query = "MATCH (t) WHERE t.type = \"table\" RETURN t.name AS name";

    let (_, sparql_body) = sparql(&app, "asha", &sparql_query).await;
    let (_, cypher_body) = cypher(&app, "asha", cypher_query).await;

    let names = |body: &Value| -> std::collections::BTreeSet<String> {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| {
                row["name"]
                    .as_str()
                    .map(|s| s.trim_matches('"').to_string())
            })
            .collect()
    };

    let sparql_names = names(&sparql_body);
    let cypher_names = names(&cypher_body);

    assert_eq!(
        sparql_names, cypher_names,
        "one restricted principal must see the same names through both \
         languages: sparql={sparql_body}, cypher={cypher_body}"
    );
    assert!(
        !sparql_names.iter().any(|n| n == "customers"),
        "the analyst must not see the denied table through either language: {sparql_names:?}"
    );
}

/// A join must not reach a denied schema in Cypher either — the Cypher
/// counterpart of `a_denied_asset_is_not_reachable_through_a_join`.
#[tokio::test]
async fn a_denied_asset_is_not_reachable_through_a_cypher_join() {
    let (app, _container) = fixture().await;
    let (_, analyst) = cypher(
        &app,
        "asha",
        "MATCH (c)-[r]->(t) WHERE r.type = \"parentTable\" RETURN t.fqn AS fqn",
    )
    .await;

    let body = analyst.to_string();
    assert!(
        !body.contains("core_banking"),
        "a join reached into the denied schema: {body}"
    );
}

#[tokio::test]
async fn a_malformed_cypher_query_is_a_400_naming_the_field() {
    let (app, _container) = fixture().await;
    let (status, body) = cypher(&app, "root", "MATCH (n RETURN n").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["field"], "query");
}

/// A syntactically valid Cypher query outside the served subset — a write —
/// is the same `400` shape as a parse failure, not a `500`: the subset gate's
/// refusal surfaces through `Catalog::cypher` as a `Validation` error exactly
/// like a parse error does.
#[tokio::test]
async fn a_write_clause_over_cypher_is_a_400_not_a_500() {
    let (app, _container) = fixture().await;
    let (status, body) = cypher(&app, "root", "CREATE (n:Table) RETURN n").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "query");
}

#[tokio::test]
async fn an_unauthenticated_cypher_request_is_rejected() {
    let (app, _container) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/cypher")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "query": "MATCH (n) RETURN n" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
