//! Epic 18 Slice A against a real Postgres.

mod common;

use graph_owl_storage::{SignatureScheme, Storage, WebhookEndpoint};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn endpoint(path: &str) -> WebhookEndpoint {
    let now = chrono::Utc::now();
    WebhookEndpoint {
        id: Uuid::new_v4(),
        path: path.to_string(),
        source: "dbt-bot".to_string(),
        signature_scheme: SignatureScheme::HmacSha256 {
            header: "X-Signature".to_string(),
            prefix: "sha256=".to_string(),
        },
        mapping: "dbt-run-completed".to_string(),
        event_filter: vec!["run.completed".to_string()],
        enabled: true,
        has_secret: false,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn a_registered_endpoint_round_trips_without_its_secret() {
    let (storage, _db) = test_storage().await;

    let written = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"shared-secret"))
        .await
        .expect("register");

    assert!(written.has_secret);
    let fetched = storage
        .get_webhook_endpoint(written.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched, written);
    assert_eq!(
        fetched.signature_scheme,
        SignatureScheme::HmacSha256 {
            header: "X-Signature".to_string(),
            prefix: "sha256=".to_string(),
        }
    );
}

#[tokio::test]
async fn the_secret_is_never_in_a_get_or_list_response() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"shared-secret"))
        .await
        .expect("register");

    let listed = storage.list_webhook_endpoints().await.expect("list");
    // A real, non-empty result naming the registered endpoint — not just
    // "the secret string does not appear", which an empty list would also
    // satisfy for the wrong reason.
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, written.id);
    assert_eq!(listed[0].path, "dbt");

    // `WebhookEndpoint` structurally has no field for it — this proves the
    // *storage* read paths agree, not just the type. Serialized JSON must
    // not contain the raw secret bytes.
    let json = serde_json::to_string(&listed).expect("serialize");
    assert!(!json.contains("shared-secret"));
}

#[tokio::test]
async fn the_secret_is_readable_only_through_its_own_method() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"shared-secret"))
        .await
        .expect("register");

    let secret = storage
        .webhook_secret(written.id)
        .await
        .expect("read secret")
        .expect("must exist");
    assert_eq!(secret, b"shared-secret");
}

#[tokio::test]
async fn a_second_endpoint_cannot_reuse_a_registered_path() {
    let (storage, _db) = test_storage().await;
    storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"secret-a"))
        .await
        .expect("first registration");

    let result = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"secret-b"))
        .await;

    assert!(matches!(
        result,
        Err(graph_owl_storage::StorageError::Conflict {
            kind: graph_owl_storage::ConflictKind::WebhookPathExists,
            ..
        })
    ));
}

#[tokio::test]
async fn updating_an_endpoint_without_a_new_secret_leaves_the_old_one_in_place() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"original-secret"))
        .await
        .expect("register");

    let mut update = written.clone();
    update.enabled = false;
    let updated = storage
        .upsert_webhook_endpoint(update, None)
        .await
        .expect("update without a new secret");

    assert!(!updated.enabled);
    assert!(updated.has_secret);
    let secret = storage
        .webhook_secret(written.id)
        .await
        .expect("read secret")
        .expect("must still exist");
    assert_eq!(secret, b"original-secret");
}

#[tokio::test]
async fn found_by_path_is_how_a_receiver_looks_up_its_endpoint() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_webhook_endpoint(endpoint("dbt"), Some(b"secret"))
        .await
        .expect("register");

    let found = storage
        .get_webhook_endpoint_by_path("dbt")
        .await
        .expect("lookup")
        .expect("must exist");
    assert_eq!(found.id, written.id);

    let missing = storage
        .get_webhook_endpoint_by_path("does-not-exist")
        .await
        .expect("lookup");
    assert_eq!(missing, None);
}

#[tokio::test]
async fn a_non_unique_violation_is_not_reported_as_a_path_conflict() {
    let (storage, _db) = test_storage().await;
    // Violates `CHECK (path <> '')` — a check violation (`23514`), not a
    // unique violation (`23505`). Proves the error-mapping guard actually
    // discriminates by code rather than treating every database error as
    // "the path is taken".
    let result = storage
        .upsert_webhook_endpoint(endpoint(""), Some(b"secret"))
        .await;

    assert!(matches!(
        result,
        Err(graph_owl_storage::StorageError::Unexpected(_))
    ));
}

#[tokio::test]
async fn an_ed25519_endpoint_round_trips_with_no_prefix() {
    let (storage, _db) = test_storage().await;
    let mut ed = endpoint("airflow");
    ed.signature_scheme = SignatureScheme::Ed25519 {
        header: "X-Signature-Ed25519".to_string(),
    };

    let written = storage
        .upsert_webhook_endpoint(ed, Some(b"public-key-bytes-not-secret"))
        .await
        .expect("register");

    assert_eq!(
        written.signature_scheme,
        SignatureScheme::Ed25519 {
            header: "X-Signature-Ed25519".to_string(),
        }
    );
}
