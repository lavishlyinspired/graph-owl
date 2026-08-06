//! Epic 42 Slice F's "Bolt endpoint status and active sessions... read-only"
//! (attributed there to Epic 7d) — `GET /admin/bolt/status`.
//!
//! Shares a catalog between a live Bolt listener and the HTTP app on
//! purpose: the point of this test is that a session created over the wire
//! protocol shows up in an HTTP read, not that the two surfaces merely
//! compile against the same types.

#![cfg(feature = "bolt")]

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "bolt-status-demo-signing-secret-not-for-production";

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

async fn send_as(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", token(subject)))
        .body(Body::empty())
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

/// `UPDATE ... WHERE id = $1` matches zero rows for a subject that has never
/// authenticated — users are auto-provisioned on first request (Epic 12
/// Slice A), not created by any admin endpoint — so `subject` signs in once
/// first via a throwaway request before the promotion.
async fn promote_to_admin(app: &axum::Router, connection_string: &str, subject: &str) {
    send_as(app, "GET", "/assets/stats", subject).await;
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("db connection");
    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = $1")
        .bind(subject)
        .execute(&pool)
        .await
        .expect("promote subject to admin");
}

struct BoltClient {
    stream: tokio::net::TcpStream,
    decoder: graph_owl_bolt::chunking::Decoder,
}

impl BoltClient {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let mut bytes = graph_owl_bolt::handshake::MAGIC.to_vec();
        bytes.extend_from_slice(&graph_owl_bolt::handshake::encode_version(5, 0));
        bytes.extend_from_slice(&graph_owl_bolt::handshake::NO_VERSION);
        bytes.extend_from_slice(&graph_owl_bolt::handshake::NO_VERSION);
        bytes.extend_from_slice(&graph_owl_bolt::handshake::NO_VERSION);
        stream.write_all(&bytes).await.expect("write handshake");
        let mut reply = [0u8; 4];
        stream
            .read_exact(&mut reply)
            .await
            .expect("handshake reply");
        Self {
            stream,
            decoder: graph_owl_bolt::chunking::Decoder::new(),
        }
    }

    async fn hello(&mut self, subject: &str) {
        use graph_owl_bolt::packstream::BoltValue;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let value = BoltValue::Structure {
            signature: graph_owl_bolt::messages::signature::HELLO,
            fields: vec![BoltValue::Dictionary(vec![
                (
                    "user_agent".to_string(),
                    BoltValue::String("bolt-status-test/1.0".to_string()),
                ),
                (
                    "scheme".to_string(),
                    BoltValue::String("bearer".to_string()),
                ),
                ("credentials".to_string(), BoltValue::String(token(subject))),
            ])],
        };
        let bytes = graph_owl_bolt::packstream::encode(&value);
        self.stream
            .write_all(&graph_owl_bolt::chunking::encode(&bytes))
            .await
            .expect("write hello");
        let mut buf = [0u8; 4096];
        loop {
            if let Some(message) = self
                .decoder
                .next_message(16 * 1024 * 1024)
                .expect("chunking")
            {
                let (value, _) = graph_owl_bolt::packstream::decode(&message, 16 * 1024 * 1024)
                    .expect("packstream decode")
                    .expect("a complete value");
                assert!(
                    matches!(&value, BoltValue::Structure { signature, .. }
                        if *signature == graph_owl_bolt::messages::signature::SUCCESS),
                    "HELLO must succeed: {value:?}"
                );
                return;
            }
            let n = self.stream.read(&mut buf).await.expect("socket read");
            assert!(n > 0, "connection closed while HELLO's reply was expected");
            self.decoder.feed(&buf[..n]);
        }
    }
}

#[tokio::test]
async fn an_admin_sees_a_live_bolt_session_over_http() {
    let (catalog, _db, connection_string) = common::test_catalog_with_secret(SECRET).await;
    let app = graph_owl_server::app(catalog.clone());
    promote_to_admin(&app, &connection_string, "root").await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let server = graph_owl_server::bolt::build_server(
        catalog,
        graph_owl_bolt::BoltLimits::default(),
        graph_owl_api::SparqlBudget::default(),
    );
    graph_owl_server::bolt::register(server.clone());
    tokio::spawn(async move {
        server.serve(listener, std::future::pending()).await;
    });

    let mut bolt_client = BoltClient::connect(addr).await;
    bolt_client.hello("carol").await;

    let (status, body) = send_as(&app, "GET", "/admin/bolt/status", "root").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], json!(true), "{body}");
    assert_eq!(body["activeConnections"], json!(1), "{body}");
    assert_eq!(body["sessions"][0]["principal"], "carol", "{body}");
    assert!(!body["sessions"][0]["connectedAt"].is_null(), "{body}");
}

#[tokio::test]
async fn a_non_admin_gets_404_not_403() {
    let (catalog, _db, _connection_string) = common::test_catalog_with_secret(SECRET).await;
    let app = graph_owl_server::app(catalog);

    let (status, body) = send_as(&app, "GET", "/admin/bolt/status", "mallory").await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn no_registered_server_reports_disabled_rather_than_a_stale_listener() {
    // No `register()` call in this test at all — nextest's one-process-per-test
    // isolation is what makes this assertion meaningful rather than an
    // accident of test ordering (a shared-process runner could see whichever
    // earlier test happened to register last).
    let (catalog, _db, connection_string) = common::test_catalog_with_secret(SECRET).await;
    let app = graph_owl_server::app(catalog);
    promote_to_admin(&app, &connection_string, "root").await;

    let (status, body) = send_as(&app, "GET", "/admin/bolt/status", "root").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], json!(false), "{body}");
}
