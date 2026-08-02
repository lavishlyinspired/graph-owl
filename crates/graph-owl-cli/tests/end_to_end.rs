//! Epic 20 end to end: declarations → plan → apply → **a real catalog**.
//!
//! **What this catches that nothing else can.** Every other test in this
//! crate runs against a recording double, which is right for the questions
//! they ask (did we send the correct *decision*?) and structurally unable to
//! answer this one: a double accepts any payload shape forever. It would
//! have gone on accepting `parentFqn` indefinitely while every real request
//! for a child was refused — which is not hypothetical, it is exactly what
//! writing this test surfaced. The server's `UpsertAsset` takes `parentId`
//! as a UUID.
//!
//! One test, at the epic's end. It stands up the real router over a real
//! Postgres, so it is expensive; the value is not coverage, it is contact
//! with something that can say no.

use graph_owl_cli::apply::{ParentIds, in_dependency_order};
use graph_owl_cli::client::{Catalog, ClientError, UpsertRequest};
use graph_owl_cli::plan::{Change, LiveEntity, compute};
use graph_owl_cli::validate::validate_directory;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// A `Catalog` backed by the real axum router — the same code path an HTTP
/// request takes, minus the socket. `oneshot` gives real routing, real
/// extractors, real validation and the real serde boundary, which is where
/// a shape mismatch actually bites.
struct LiveCatalog {
    app: axum::Router,
    runtime: tokio::runtime::Handle,
}

impl LiveCatalog {
    fn call(&self, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let app = self.app.clone();
        tokio::task::block_in_place(|| {
            self.runtime.block_on(async move {
                let response = app.oneshot(request).await.expect("handled");
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body");
                let value = if bytes.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
                };
                (status, value)
            })
        })
    }
}

impl Catalog for LiveCatalog {
    fn live_within(&self, scope_prefixes: &[String]) -> Result<Vec<LiveEntity>, ClientError> {
        let (status, body) = self.call(
            Request::builder()
                .uri("/assets?limit=200")
                .body(Body::empty())
                .expect("request"),
        );
        if !status.is_success() {
            return Err(ClientError::Refused {
                status: status.as_u16(),
                detail: body.to_string(),
            });
        }
        Ok(body["data"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(|asset| {
                let fqn = asset["fullyQualifiedName"].as_str()?.to_string();
                let in_scope = scope_prefixes
                    .iter()
                    .any(|p| fqn == *p || fqn.starts_with(&format!("{p}.")));
                if !in_scope {
                    return None;
                }
                Some(LiveEntity {
                    id: asset["id"].as_str()?.to_string(),
                    fully_qualified_name: fqn,
                    kind: asset["kind"].as_str()?.to_string(),
                    description: asset["description"].as_str().map(ToString::to_string),
                })
            })
            .collect())
    }

    fn upsert(&self, entity: &UpsertRequest) -> Result<String, ClientError> {
        // **Built field by field, omitting what is not declared.** Decision
        // 4 is enforced on the wire here: an undeclared description is an
        // absent key, not a null — and only a real server can confirm the
        // difference is respected rather than merely intended.
        let mut payload = serde_json::Map::new();
        payload.insert("kind".into(), entity.kind.clone().into());
        payload.insert("name".into(), entity.name.clone().into());
        if let Some(parent_id) = &entity.parent_id {
            payload.insert("parentId".into(), parent_id.clone().into());
        }
        if let Some(description) = &entity.description {
            payload.insert("description".into(), description.clone().into());
        }

        let (status, body) = self.call(
            Request::builder()
                .method("POST")
                .uri("/assets")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::Value::Object(payload).to_string()))
                .expect("request"),
        );
        if !status.is_success() {
            return Err(ClientError::Refused {
                status: status.as_u16(),
                detail: body.to_string(),
            });
        }
        Ok(body["id"]
            .as_str()
            .expect("the catalog returns an id")
            .to_string())
    }

    fn tombstone(&self, _fully_qualified_name: &str) -> Result<(), ClientError> {
        unreachable!("apply never prunes; that is Slice D's separate, guarded path")
    }
}

/// **The round trip that matters**: a directory of YAML becomes real
/// entities, and running it again changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn declarations_apply_to_a_real_catalog_and_a_second_apply_is_a_no_op() {
    let (app, _db) = common::test_app().await;
    let catalog = LiveCatalog {
        app,
        runtime: tokio::runtime::Handle::current(),
    };

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid");
    let declarations = validate_directory(&root).expect("the fixture is valid");
    let scope = vec!["snowflake_prod".to_string()];

    // ── first apply: everything is created ──────────────────────────────
    let live = catalog.live_within(&scope).expect("read");
    assert!(live.is_empty(), "the catalog starts empty");

    let plan = compute(&declarations, &live);
    assert_eq!(
        plan.counts().create,
        3,
        "{}",
        graph_owl_cli::plan::render(&plan)
    );

    let mut parents = ParentIds::from_live(&live);
    for entity in in_dependency_order(&plan) {
        let (_, declaration) = &declarations.by_fqn[&entity.fully_qualified_name];
        let parent_id = declaration
            .metadata
            .parent
            .as_deref()
            .and_then(|fqn| parents.get(fqn))
            .map(ToString::to_string);
        let id = catalog
            .upsert(&UpsertRequest {
                kind: declaration.kind.clone(),
                name: declaration.metadata.name.clone(),
                parent_id,
                description: declaration.metadata.description.clone(),
            })
            .unwrap_or_else(|e| panic!("the catalog refused {}: {e}", entity.fully_qualified_name));
        parents.learn(&entity.fully_qualified_name, id);
    }

    // ── second plan: nothing left to do ─────────────────────────────────
    let live_after = catalog.live_within(&scope).expect("read");
    assert_eq!(
        live_after.len(),
        3,
        "all three declared entities exist, with the FQNs the catalog derived"
    );

    let second = compute(&declarations, &live_after);
    assert!(
        !second.has_changes(),
        "a second apply must be a no-op — zero versions, zero events:\n{}",
        graph_owl_cli::plan::render(&second)
    );
    assert_eq!(second.counts().no_change, 3);
}

/// **Decision 4 against a real server.** A description curated outside the
/// declarations survives an apply — the failure this prevents is silently
/// resetting every hand-written description, and it can only be *proven*
/// where a real write happens.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_edited_undeclared_field_survives_an_apply() {
    let (app, _db) = common::test_app().await;
    let catalog = LiveCatalog {
        app,
        runtime: tokio::runtime::Handle::current(),
    };

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid");
    let declarations = validate_directory(&root).expect("valid");
    let scope = vec!["snowflake_prod".to_string()];

    // Apply once so the entities exist.
    let live = catalog.live_within(&scope).expect("read");
    let plan = compute(&declarations, &live);
    let mut parents = ParentIds::from_live(&live);
    for entity in in_dependency_order(&plan) {
        let (_, declaration) = &declarations.by_fqn[&entity.fully_qualified_name];
        let parent_id = declaration
            .metadata
            .parent
            .as_deref()
            .and_then(|fqn| parents.get(fqn))
            .map(ToString::to_string);
        let id = catalog
            .upsert(&UpsertRequest {
                kind: declaration.kind.clone(),
                name: declaration.metadata.name.clone(),
                parent_id,
                description: declaration.metadata.description.clone(),
            })
            .expect("apply");
        parents.learn(&entity.fully_qualified_name, id);
    }

    // `snowflake_prod.analytics` declares no description. Someone writes one
    // by hand — the exact situation decision 4 protects.
    let live = catalog.live_within(&scope).expect("read");
    let analytics = live
        .iter()
        .find(|e| e.fully_qualified_name == "snowflake_prod.analytics")
        .expect("exists");
    let (status, _) = catalog.call(
        Request::builder()
            .method("PATCH")
            .uri(format!("/assets/{}", analytics.id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"description": "written by a human at 2am"}).to_string(),
            ))
            .expect("request"),
    );
    assert!(status.is_success(), "the hand edit should land: {status}");

    // Re-plan. The declaration still says nothing about the description, so
    // there must be nothing to do.
    let live = catalog.live_within(&scope).expect("read");
    let plan = compute(&declarations, &live);
    assert!(
        !plan.has_changes(),
        "an undeclared field must not be planned as a change:\n{}",
        graph_owl_cli::plan::render(&plan)
    );
    assert!(
        plan.entities.iter().all(|e| e.change == Change::NoChange),
        "nothing at all should be pending"
    );

    // And it is still there.
    let live = catalog.live_within(&scope).expect("read");
    let analytics = live
        .iter()
        .find(|e| e.fully_qualified_name == "snowflake_prod.analytics")
        .expect("exists");
    assert_eq!(
        analytics.description.as_deref(),
        Some("written by a human at 2am"),
        "the hand-written description must survive"
    );
}

mod common;
