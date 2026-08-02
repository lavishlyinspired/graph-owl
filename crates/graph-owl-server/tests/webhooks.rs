//! Epic 18 Slice A at the wire.
//!
//! The facade tests (`graph-owl-api`) prove `Catalog::receive_webhook`
//! verifies before recording. What can only be checked here is the boundary
//! those tests assume: that the HTTP layer hands the *raw* bytes and the
//! *configured* header value through unchanged — no JSON parsing, no
//! re-serialization — because a body that never gets deserialized on a bad
//! signature is a claim about this crate's extractors, not about the facade.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app, test_app_with_secret};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

const SECRET: &str = "webhook-test-signing-secret";

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

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// Raw bytes with an arbitrary header — the shape an outside sender uses,
/// never JSON on the way in.
async fn deliver(
    app: &axum::Router,
    path: &str,
    header: Option<(&str, &str)>,
    body: &'static [u8],
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/webhooks/receive/{path}"));
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    let request = request
        .body(Body::from(body))
        .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("handled");
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

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

async fn text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

fn hmac_sign(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    let hex: String = bytes.iter().fold(String::new(), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    });
    format!("sha256={hex}")
}

async fn register(app: &axum::Router, path: &str, secret: &str) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": path,
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "eventFilter": ["run.completed"],
            "secret": secret,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

async fn register_rate_limited(
    app: &axum::Router,
    path: &str,
    secret: &str,
    rate_limit_per_minute: u32,
) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": path,
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "eventFilter": ["run.completed"],
            "secret": secret,
            "rateLimitPerMinute": rate_limit_per_minute,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// **The registration secret never appears in any response** — same claim
/// `connector_configs.rs` makes for connector credentials, at the same
/// surface.
#[tokio::test]
async fn a_registered_endpoint_never_exposes_its_secret() {
    let (app, _database, _) = test_app().await;

    let created = register(&app, "dbt", "shared-secret-do-not-leak").await;
    assert_eq!(created["hasSecret"], true, "{created}");
    assert!(created.get("secret").is_none(), "{created}");
    let created_string = created.to_string();
    assert!(
        !created_string.contains("shared-secret-do-not-leak"),
        "the secret came back in the response that stored it: {created_string}"
    );

    let (status, listed) = send(&app, "GET", "/webhooks/endpoints", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed_string = listed.to_string();
    assert!(
        !listed_string.contains("shared-secret-do-not-leak"),
        "the secret came back in a later read: {listed_string}"
    );
    assert!(
        listed_string.contains("\"path\":\"dbt\""),
        "{listed_string}"
    );
}

/// **A correctly signed delivery is recorded.** The important claim: the
/// signature is checked against exactly these bytes, sent with exactly this
/// header — not a re-serialized or re-parsed form of them.
#[tokio::test]
async fn a_correctly_signed_delivery_is_recorded() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);

    let (status, recorded) = deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
    assert_eq!(status, StatusCode::CREATED, "{recorded}");
    assert_eq!(recorded["state"], "received", "{recorded}");
}

/// **A bad signature is `401`.** Slice A's central claim, at the wire: a
/// payload that would fail to parse as JSON must still 401 rather than 400 —
/// proving verification happens before any attempt to read the body as
/// anything but bytes.
#[tokio::test]
async fn a_bad_signature_is_unauthenticated_even_for_a_body_that_is_not_json() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "the-real-secret").await;

    let body: &'static [u8] = b"this is not json at all {{{";
    let wrong_signature = hmac_sign("a-guessed-secret", body);

    let (status, problem) =
        deliver(&app, "dbt", Some(("X-Signature", &wrong_signature)), body).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a malformed body must still fail verification before any parse is attempted: {problem}"
    );
}

/// A missing signature header is `401`, not a `400` for a missing field —
/// this is an authentication failure, not a validation one.
#[tokio::test]
async fn a_missing_signature_header_is_unauthenticated() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "secret").await;

    let (status, _) = deliver(&app, "dbt", None, br#"{"event":"run.completed"}"#).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A tampered body does not verify, even under the right header value for a
/// different payload — the signature covers these exact bytes.
#[tokio::test]
async fn a_tampered_body_is_unauthenticated() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let signed_body: &'static [u8] = br#"{"amount":10}"#;
    let signature = hmac_sign("shared-secret", signed_body);
    let tampered: &'static [u8] = br#"{"amount":99999}"#;

    let (status, _) = deliver(&app, "dbt", Some(("X-Signature", &signature)), tampered).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// An unregistered path is `404` — there is nothing to verify against.
#[tokio::test]
async fn an_unregistered_path_is_not_found() {
    let (app, _database, _) = test_app().await;

    let (status, _) = deliver(
        &app,
        "no-such-endpoint",
        Some(("X-Signature", "sha256=whatever")),
        br#"{"event":"run.completed"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A disabled endpoint is `404`, not `403` — an existence signal is
/// unnecessary, pulled forward from Slice E's own reasoning.
#[tokio::test]
async fn a_disabled_endpoint_is_not_found() {
    let (app, _database, _) = test_app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "dbt",
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "enabled": false,
            "secret": "shared-secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let signed_body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", signed_body);
    let (status, _) = deliver(&app, "dbt", Some(("X-Signature", &signature)), signed_body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// One endpoint per path — a second registration under the same path would
/// make "which secret does this URL verify against" unanswerable.
#[tokio::test]
async fn registering_a_taken_path_is_a_conflict() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "secret-a").await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "dbt",
            "source": "another-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "secret": "secret-b",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// An empty `path`/`source`/`mapping` is refused with `400` rather than
/// stored and left to fail later against the database's own `CHECK`
/// constraints.
#[tokio::test]
async fn a_blank_path_is_refused() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "",
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "secret": "secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Omitting `enabled` registers a usable endpoint — a freshly registered one
/// has no prior state for a client to be preserving, so absent means "on".
#[tokio::test]
async fn enabled_defaults_to_true_when_omitted() {
    let (app, _database, _) = test_app().await;
    let created = register(&app, "dbt", "secret").await;
    assert_eq!(created["enabled"], true, "{created}");
}

/// Admin-only, same reasoning as connector configs: a webhook endpoint holds
/// a secret and decides what an external system may write into the catalog.
#[tokio::test]
async fn a_non_admin_cannot_register_or_list_webhook_endpoints() {
    let (app, _database, _) = test_app_with_secret(SECRET).await;

    let register_request = Request::builder()
        .method("POST")
        .uri("/webhooks/endpoints")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(
            json!({
                "path": "dbt",
                "source": "dbt-bot",
                "signatureScheme": {
                    "kind": "hmacSha256",
                    "header": "X-Signature",
                    "prefix": "sha256=",
                },
                "mapping": "dbt-run-completed",
                "secret": "secret",
            })
            .to_string(),
        ))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(register_request)
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let list_request = Request::builder()
        .method("GET")
        .uri("/webhooks/endpoints")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    let response = app.clone().oneshot(list_request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Epic 18 Slice E: a verified delivery with an unparseable body is `400`
/// synchronously, not `201` followed by a silent async dead-letter — the
/// criterion names a synchronous response, and it is only checkable at the
/// wire, since the facade test for the same behavior reads the recorded
/// event directly rather than an HTTP status.
#[tokio::test]
async fn a_verified_but_unparseable_body_is_bad_request() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let body: &'static [u8] = b"this is not json at all {{{";
    let signature = hmac_sign("shared-secret", body);

    let (status, problem) = deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
}

/// Epic 18 Slice E: **one flooding sender must not cost every other sender
/// its traffic.** The limit is per-endpoint configuration (`None` means
/// unlimited, the default every other test in this file relies on) — this
/// is the one endpoint that opts in, and only its own deliveries are
/// throttled.
#[tokio::test]
async fn a_burst_past_the_configured_limit_is_rate_limited() {
    let (app, _database, _) = test_app().await;
    register_rate_limited(&app, "dbt", "shared-secret", 1).await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);

    let (first_status, first_body) =
        deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
    assert_eq!(first_status, StatusCode::CREATED, "{first_body}");

    let (second_status, second_body) =
        deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
    assert_eq!(
        second_status,
        StatusCode::TOO_MANY_REQUESTS,
        "{second_body}"
    );
}

/// The refusal names how long to wait — a client that ignores `Retry-After`
/// and retries immediately would just be refused again.
#[tokio::test]
async fn a_rate_limited_response_names_retry_after() {
    let (app, _database, _) = test_app().await;
    register_rate_limited(&app, "dbt", "shared-secret", 1).await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);
    deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;

    let request = Request::builder()
        .method("POST")
        .uri("/webhooks/receive/dbt")
        .header("X-Signature", &signature)
        .body(Body::from(body))
        .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after = response
        .headers()
        .get(axum::http::header::RETRY_AFTER)
        .expect("Retry-After header must be present");
    let seconds: u64 = retry_after
        .to_str()
        .expect("header should be ascii")
        .parse()
        .expect("header should be a number of seconds");
    assert!(seconds >= 1, "Retry-After must never read as zero");
}

/// Isolation between endpoints: saturating one endpoint's own limit does not
/// cost a *different* endpoint its traffic — the whole reason this is
/// per-endpoint rather than one shared counter.
#[tokio::test]
async fn a_saturated_endpoint_does_not_rate_limit_a_different_one() {
    let (app, _database, _) = test_app().await;
    register_rate_limited(&app, "noisy", "noisy-secret", 1).await;
    register(&app, "quiet", "quiet-secret").await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let noisy_signature = hmac_sign("noisy-secret", body);
    deliver(&app, "noisy", Some(("X-Signature", &noisy_signature)), body).await;
    let (saturated_status, _) =
        deliver(&app, "noisy", Some(("X-Signature", &noisy_signature)), body).await;
    assert_eq!(saturated_status, StatusCode::TOO_MANY_REQUESTS);

    let quiet_signature = hmac_sign("quiet-secret", body);
    let (quiet_status, quiet_body) =
        deliver(&app, "quiet", Some(("X-Signature", &quiet_signature)), body).await;
    assert_eq!(
        quiet_status,
        StatusCode::CREATED,
        "a different endpoint must have its own budget: {quiet_body}"
    );
}

/// Epic 18 Slice E's payload-size-cap criterion, met by **adopting** axum's
/// own default rather than inventing a number of our own: `Bytes` (and
/// anything built on it) already refuses a body over 2MB with `413`, which
/// is exactly the status this criterion names. This test exists to lock
/// that behavior in — if a future change swaps the extractor for something
/// that reads the body unbounded, this is what catches it.
#[tokio::test]
async fn an_oversized_delivery_is_payload_too_large() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let oversized: &'static [u8] = Box::leak(vec![b'a'; 2 * 1024 * 1024 + 1].into_boxed_slice());
    let signature = hmac_sign("shared-secret", oversized);

    let (status, _) = deliver(&app, "dbt", Some(("X-Signature", &signature)), oversized).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

/// Epic 18 Slice E's per-endpoint metrics criterion. Only the two states
/// `receive_webhook` decides *synchronously* are checked here — `applied`
/// and `duplicate` are decided by the detached `process_inbound_event` task,
/// and a test that polled for it would be a sleep-based flake with a
/// schedule, the same reasoning `admission`'s own tests already document.
#[tokio::test]
async fn a_delivery_is_counted_under_its_endpoint_and_state() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);
    deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;

    let malformed: &'static [u8] = b"not json";
    let malformed_signature = hmac_sign("shared-secret", malformed);
    deliver(
        &app,
        "dbt",
        Some(("X-Signature", &malformed_signature)),
        malformed,
    )
    .await;

    let scrape = text(get(&app, "/metrics").await).await;
    assert!(
        scrape.contains("graph_owl_webhook_events_total"),
        "no webhook events counter in the scrape:\n{scrape}"
    );
    assert!(
        scrape.contains(r#"endpoint="dbt""#),
        "the endpoint label is what makes the counter actionable:\n{scrape}"
    );
    assert!(
        scrape.contains(r#"state="received""#),
        "the accepted delivery must be counted as received:\n{scrape}"
    );
    assert!(
        scrape.contains(r#"state="failed""#),
        "the malformed delivery must be counted as failed:\n{scrape}"
    );
}

/// An endpoint with no configured limit is unlimited — the default every
/// other test in this file already relies on, asserted explicitly here so a
/// regression that starts refusing unconfigured endpoints fails loudly.
#[tokio::test]
async fn an_endpoint_with_no_configured_limit_is_never_rate_limited() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);
    for _ in 0..10 {
        let (status, response_body) =
            deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
        assert_eq!(status, StatusCode::CREATED, "{response_body}");
    }
}
