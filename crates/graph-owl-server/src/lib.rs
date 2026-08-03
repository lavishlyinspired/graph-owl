pub mod admission;
pub mod budget;
pub mod jwks;
pub mod observability;
pub mod openapi;
pub mod rate_limit;
pub mod streaming;

use axum::{
    Json, Router,
    extract::{
        FromRequest, FromRequestParts, Path, Query, Request, State, rejection::JsonRejection,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use std::sync::Arc;

use graph_owl_api::SparqlBudget;
use graph_owl_api::{
    Catalog, CatalogError, CreateRelationship, CreateTable, UpsertAsset,
    validation::{FieldError, FieldErrorCode, ValidateBody, require_non_empty_string},
};
use graph_owl_connectors::{Connector, DeletionPlan, RunScope, postgres::PostgresConnector};
use graph_owl_core::contract::{CompatibilityMode, ContractStatus};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Principal, Relationship, Table, TableUpdate,
    page::{Page, PageRequest, PageRequestError},
};
use graph_owl_storage::{ConflictKind, StorageError};
use serde::de::DeserializeOwned;
use serde_json::json;
use uuid::Uuid;

/// The router with default admission limits.
///
/// Kept as the one-argument function every test already calls. The composition
/// root uses [`app_with_admission`], because config is read once at startup and
/// an invalid value must refuse to start rather than be silently defaulted here.
pub fn app(catalog: Catalog) -> Router {
    app_with_admission(
        catalog,
        Arc::new(admission::Admission::with_limits(
            &[],
            admission::DEFAULT_RETRY_AFTER_SECONDS,
        )),
    )
}

pub fn app_with_admission(catalog: Catalog, admission: Arc<admission::Admission>) -> Router {
    // Installed when the app is built, not on the first scrape. The `metrics`
    // facade drops every measurement taken before a recorder exists, so a
    // lazily-installed one loses everything up to the first request Prometheus
    // happens to make — silently, and exactly during the startup window an
    // operator most wants to see.
    observability::metrics_handle();

    let router = Router::new()
        .route("/tables", post(create_table).get(list_tables))
        .route(
            "/tables/{id}",
            get(get_table).patch(update_table).delete(delete_table),
        )
        .route(
            "/tables/{id}/relationships",
            post(create_relationship).get(list_relationships_for_table),
        )
        .route("/relationships/{id}", delete(delete_relationship))
        .route("/assets", post(upsert_asset).get(list_assets))
        .route("/assets/search", get(search_assets))
        .route("/assets/roots", get(list_roots))
        .route("/assets/stats", get(asset_stats))
        .route("/overview", get(overview))
        .route("/graph/reconcile", post(reconcile_projection))
        .route("/sparql", post(sparql))
        .route("/connectors/postgres/runs", post(run_postgres_connector))
        .route("/connectors/runs", get(list_connector_runs))
        .route("/lineage", post(assert_lineage))
        .route("/lineage/{id}", delete(remove_lineage))
        .route("/lineage/asset/{id}", get(lineage_graph))
        // Epic 22. Definitions are the vocabulary; values ride on the entity
        // itself in `extension`, so there is no endpoint for a value.
        .route(
            "/custom-properties",
            get(list_custom_properties).post(define_custom_property),
        )
        .route(
            "/custom-properties/{id}",
            patch(update_custom_property).delete(delete_custom_property),
        )
        // Epic 30. graph-owl ingests and displays results; it does not run
        // tests, so there is no scheduler and no executor here.
        .route(
            "/test-definitions",
            get(list_test_definitions).post(create_test_definition),
        )
        .route(
            "/test-definitions/{id}/cadence",
            post(set_definition_cadence),
        )
        .route("/test-suites", post(create_test_suite))
        .route("/test-cases", get(list_test_cases).post(create_test_case))
        .route("/test-cases/{id}", delete(delete_test_case))
        .route(
            "/test-cases/{id}/results",
            get(list_test_results).post(record_test_results),
        )
        .route("/test-results/prune", post(prune_test_results))
        .route("/health/{fqn}", get(asset_health))
        // Epic 29 Slices D and E.
        .route(
            "/lineage/{id}/columns",
            get(get_column_mappings).put(set_column_mappings),
        )
        .route("/lineage/reconcile", post(reconcile_lineage))
        // Epic 27. A contract is an entity with parties, not an annotation —
        // so it gets entity routes, and the evaluation is a sub-resource of the
        // *asset* whose change triggered it.
        .route("/contracts", get(list_contracts).post(create_contract))
        .route("/contracts/{id}", get(get_contract))
        .route("/contracts/{id}/status", post(set_contract_status))
        .route("/contracts/{id}/breaches", delete(clear_contract_breaches))
        .route("/contracts/{id}/slas", get(evaluate_slas))
        .route("/assets/{fqn}/schema-change", post(evaluate_schema_change))
        // Epic 28. Usage is pushed, never read from an engine (decision 5).
        .route("/usage", post(record_usage))
        .route("/usage/{fqn}", get(asset_popularity))
        .route("/usage/{fqn}/rollups", get(asset_rollups))
        .route("/usage/prune", post(prune_usage))
        // Epic 25. Classifications are the operational vocabulary; labels are
        // its application, and they carry where they came from.
        .route(
            "/classifications",
            get(list_classifications).post(create_classification),
        )
        .route("/classifications/{id}", delete(delete_classification))
        .route("/classifications/{id}/tags", post(create_tag))
        .route("/tags", get(list_tags))
        .route("/tags/{fqn}", delete(delete_tag))
        .route("/tags/{fqn}/usage", get(tag_usage))
        // Labels hang off the *target*, not the tag: they are facts about the
        // entity, they version with it, and a client reading an asset wants
        // them beside everything else about it.
        .route("/labels/{targetFqn}", get(labels_on).post(apply_tag_label))
        .route("/labels/{targetFqn}/{tagFqn}", delete(remove_tag_label))
        .route("/labels/{targetFqn}/{tagFqn}/confirm", post(confirm_label))
        .route("/labels/{targetFqn}/{tagFqn}/reject", post(reject_label))
        .route(
            "/labels/{targetFqn}/{tagFqn}/propagate",
            post(propagate_label),
        )
        .route("/label-suggestions", get(label_suggestions))
        // Epic 26. Lifecycle and certification are orthogonal — an asset can be
        // Deprecated-certified, which is "still trustworthy, and going away".
        .route("/assets/{id}/lifecycle", post(set_lifecycle))
        .route("/assets/{fqn}/successor", get(terminal_successor))
        .route(
            "/certification-types",
            get(list_certification_types).post(create_certification_type),
        )
        .route(
            "/certifications/{targetFqn}",
            get(certifications_on).post(issue_certification),
        )
        .route("/recertification-queue", get(recertification_queue))
        // Epic 23. Domains are the accountability axis; data products are the
        // consumable one. Both are entities with envelopes, so both get
        // ordinary entity routes rather than a bespoke shape.
        // Epic 32. Grants are admin-only and human-only; the agent is named in
        // the path so an audit reads who changed whose capabilities.
        .route("/agents/grants", get(list_agent_grants))
        .route(
            "/agents/{agent_id}/grant",
            get(get_agent_grant)
                .put(set_agent_grant)
                .delete(revoke_agent_grant),
        )
        .route("/agents/{agent_id}/activity", get(get_agent_activity))
        .route("/proposals", get(list_proposals))
        .route("/proposals/{id}", get(get_proposal))
        .route("/proposals/{id}/accept", post(accept_proposal))
        .route("/proposals/{id}/reject", post(reject_proposal))
        .route("/domains", get(list_domains).post(create_domain))
        .route(
            "/domains/{id}",
            get(get_domain).patch(update_domain).delete(delete_domain),
        )
        .route("/domains/{id}/children", get(child_domains))
        .route("/domains/{id}/assets/count", get(count_domain_assets))
        // The assignment is a sub-resource of the *asset*, not of the domain:
        // it is a fact about the asset, it versions with the asset, and a
        // `POST /domains/{id}/assets` would read as though the domain owned the
        // list.
        .route(
            "/assets/{id}/domain",
            get(get_asset_domain)
                .post(assign_asset_domain)
                .delete(clear_asset_domain),
        )
        .route("/assets/{id}/data-products", get(get_asset_products))
        .route(
            "/data-products",
            get(list_data_products).post(create_data_product),
        )
        .route(
            "/data-products/{id}",
            get(get_data_product)
                .patch(update_data_product)
                .delete(delete_data_product),
        )
        .route("/data-products/{id}/assets", get(list_product_assets))
        .route(
            "/data-products/{id}/assets/{assetId}",
            put(add_product_asset).delete(remove_product_asset),
        )
        // Epic 21. `/extraction/runs` is the surface an out-of-process worker
        // submits to; the queue and the decision are what a human does with
        // what it proposed.
        .route("/extraction/runs", post(submit_extraction))
        .route("/extraction/runs/{id}", delete(delete_extraction_run))
        .route("/extraction/queue", get(extraction_queue))
        .route(
            "/extraction/claims/{id}/decision",
            post(decide_extraction_claim),
        )
        .route("/reasoning/runs", post(run_reasoning))
        .route("/reasoning/explain", get(explain_fact))
        .route("/reasoning/derived", get(derived_about))
        .route("/validation/runs", post(run_validation))
        .route("/validation/shapes/seed", post(seed_core_shapes))
        .route("/validation/report", get(validation_report))
        .route("/validation/waivers", post(waive_finding))
        .route("/validation/waivers/{id}", delete(revoke_waiver))
        .route("/validation/assignments", post(assign_finding))
        .route("/validation/assignments/{id}", delete(unassign_finding))
        .route("/policies/dry-run", post(dry_run_policy))
        .route("/users/{id}/roles", put(set_user_roles))
        .route("/teams", get(list_teams).post(upsert_team))
        .route("/teams/{id}/children", get(list_child_teams))
        .route("/teams/{id}", delete(delete_team))
        // Epic 24 Slice A: glossary and terms.
        .route("/glossaries", get(list_glossaries).post(create_glossary))
        .route(
            "/glossaries/{id}",
            get(get_glossary).delete(delete_glossary),
        )
        .route(
            "/glossaries/{id}/terms",
            get(list_glossary_terms).post(create_glossary_term),
        )
        .route("/glossary-terms/search", get(search_glossary_terms))
        .route(
            "/glossary-terms/{id}",
            get(get_glossary_term)
                .patch(update_glossary_term)
                .delete(delete_glossary_term),
        )
        .route(
            "/glossary-terms/{id}/relations",
            get(list_term_relations)
                .post(add_term_relation)
                .delete(delete_term_relation),
        )
        .route(
            "/glossary-terms/{id}/reviewers",
            get(list_term_reviewers).put(set_term_reviewers),
        )
        .route(
            "/glossary-terms/{id}/transitions",
            post(create_term_transition),
        )
        .route(
            "/glossary-terms/{id}/usage",
            get(term_usage).post(attach_term).delete(detach_term),
        )
        // `/business-metrics`, not `/metrics` — that path is already the
        // Prometheus exposition endpoint (Epic 10), and axum panics at
        // startup on a duplicate route rather than silently shadowing one.
        .route("/business-metrics", get(list_metrics).post(create_metric))
        .route("/business-metrics/search", get(search_metrics))
        .route(
            "/business-metrics/{id}",
            get(get_metric).patch(update_metric).delete(delete_metric),
        )
        .route("/business-metrics/{id}/sources", put(set_metric_sources))
        .route("/users/{id}", put(upsert_user).delete(delete_user))
        .route("/users/{id}/follows", get(list_follows))
        .route(
            "/assets/{id}/followers/{user_id}",
            put(follow_asset).delete(unfollow_asset),
        )
        // Epic 31. `/memories` for the record itself; the reads hang off the
        // asset, because "what do we know about this table" is the question, and
        // a client that has an asset id should not have to know a second noun to
        // ask it.
        .route("/memories", post(create_memory))
        .route("/memories/{id}", get(get_memory))
        .route("/memories/{id}/supersede", post(supersede_memory))
        // Epic 17 Slice G: mention resolution. `{id}` is the mention's
        // source.
        .route("/memories/{id}/mentions", post(resolve_mention))
        // Epic 17 Slice E: a merge is a record with a `split_at`, and this is
        // what sets it — never a delete of the `MergeRecord` itself.
        .route("/merges/{id}/split", post(split_merge))
        // Epic 17 Slice F: the review queue.
        .route("/resolution/queue", get(review_queue))
        .route("/resolution/queue/bulk", post(bulk_decide_review))
        .route("/resolution/queue/{id}/confirm", post(confirm_review))
        .route("/resolution/queue/{id}/reject", post(reject_review))
        // `PUT`, not `PATCH`: the body is the complete owner list, so the verb
        // that means "make it this" is the honest one. `PATCH` would imply a
        // delta, and a delta cannot express "this asset now has no owner" — which
        // is the operation the ownership-gap report depends on being reachable.
        .route(
            "/assets/{id}/owners",
            put(set_asset_owners).get(get_asset_owners),
        )
        .route("/assets/{id}/memories", get(recall_memories))
        .route("/assets/{id}/contradictions", get(list_contradictions))
        .route("/contradictions/reviews", post(review_contradiction))
        .route("/connectors/{connector}/schema", get(connector_schema))
        .route("/connectors/{connector}/test", post(test_connector))
        .route("/ingest", post(ingest))
        // Epic 16 Slice C. `202` and a handle, never a result: decision 2 says a
        // 500k-row file is a job, and the only honest synchronous answer to one
        // is "I have started".
        .route("/ingest/batch", post(ingest_batch))
        .route(
            "/ingest/jobs/{id}",
            get(ingest_job).delete(cancel_ingest_job),
        )
        .route(
            "/connectors/configs",
            get(list_connector_configs).post(save_connector_config),
        )
        // Epic 18 Slice A: registered webhook receivers, admin-gated the same
        // way connector configs are — both hold a credential.
        .route(
            "/webhooks/endpoints",
            get(list_webhook_endpoints).post(register_webhook_endpoint),
        )
        // A distinct prefix from `/webhooks/endpoints` rather than
        // `/webhooks/{path}`, so a registered path can never collide with the
        // literal segment `endpoints`. Unauthenticated by necessity: the
        // sender is not a graph-owl principal, and the endpoint's own
        // signature scheme is what verifies it instead.
        .route("/webhooks/receive/{path}", post(receive_webhook))
        // Epic 18 Slice C: versioned payload-to-draft mappings, admin-gated
        // for the same reason webhook endpoints are.
        .route("/webhooks/mappings", post(register_mapping))
        .route("/webhooks/mappings/{name}", get(get_mapping))
        .route(
            "/webhooks/mappings/{name}/versions",
            get(list_mapping_versions),
        )
        .route("/webhooks/mappings/{name}/dry-run", post(dry_run_mapping))
        // Epic 18 Slice D: dead-letter and replay, admin-gated.
        .route("/webhooks/events/{id}", get(inbound_event_status))
        .route(
            "/webhooks/dead-letters",
            get(dead_letter_queue).delete(purge_dead_letters),
        )
        .route("/webhooks/replay", post(replay_window))
        // Epic 19 Slice A: durable broker subscriptions, admin-gated the same
        // way webhook endpoints and connector configs are — all three hold a
        // credential and decide what an external system may write.
        .route(
            "/streaming/subscriptions",
            get(list_stream_subscriptions).post(register_stream_subscription),
        )
        // Epic 19 Slice E: replay a historical window, admin-gated.
        .route("/streaming/replay", post(replay_stream_window))
        // Epic 19 Slice D: poison-message quarantine, admin-gated.
        .route("/streaming/dead-letters", get(list_stream_dead_letters))
        .route(
            "/streaming/dead-letters/{id}/replay",
            post(replay_stream_dead_letter),
        )
        // Unauthenticated by necessity rather than by design: this is what a
        // client reads *before* it holds a token, so requiring one would be
        // circular.
        .route("/auth/config", get(auth_configuration_endpoint))
        // Unauthenticated by design: an orchestrator's probe must not depend
        // on the identity provider being reachable.
        .route("/health", get(health))
        .route("/ready", get(ready))
        // Unauthenticated for the same reason: a scrape must not depend on the
        // identity provider, or an auth outage blinds the monitoring that would
        // have shown it.
        .route("/metrics", get(observability::metrics_endpoint))
        // The contract, served so a client never has to find the file.
        //
        // **Kept as our own handler**, and Swagger UI is pointed at this URL
        // rather than handed a parsed document. `SwaggerUi::url()` takes
        // `utoipa::openapi::OpenApi`, and converting into it does not merely
        // risk losing detail — it *fails*: utoipa 8 serializes OpenAPI 3.1's
        // nullable form (`"type": ["string", "null"]`) which its own
        // deserializer cannot read back, on the very schemas its derive
        // produced. Handing the document through that round trip panics at
        // startup.
        //
        // Pointing the UI at the URL keeps one source of truth and keeps
        // `the_endpoint_serves_the_generated_document` true by construction.
        .route("/openapi.json", get(openapi::endpoint))
        .route(
            "/assets/{id}",
            get(get_asset).patch(update_asset).delete(delete_asset),
        )
        .route("/assets/{id}/versions", get(asset_versions))
        .route("/assets/{id}/restore", post(restore_asset))
        // Epic 17 Slices A, C, D: resolve this asset against its blocking-key
        // candidates. The only path that ever creates a `MergeRecord` — Slice
        // E's split has nothing to reverse until this has run.
        .route("/assets/{id}/resolve", post(resolve_asset))
        .route("/assets/{id}/children", get(list_asset_children))
        .route("/assets/{id}/graph", get(asset_graph))
        .route("/assets/{id}/ancestors", get(asset_ancestors))
        .with_state(catalog);

    // OIDC JWKS client — inserted early so the `Auth` extractor can find it in
    // request extensions. Only created when configured; without it the server
    // falls through to HS256 or open mode.
    let router = if let Some((issuer, audience)) = oidc_config() {
        let jwks_client = Arc::new(jwks::JwksClient::new(issuer, audience));
        router.layer(axum::Extension(jwks_client))
    } else {
        router
    };

    // One limiter for the whole process, same lifetime reasoning as the JWKS
    // client: per-endpoint state (`rate_limit::Window`) has to survive across
    // requests, so it is built once here rather than per-request.
    let router = router.layer(axum::Extension(Arc::new(rate_limit::RateLimiter::new())));

    router
        // **Inside** the observability layer, so a shed request is still
        // logged, still counted, and still gets its request id echoed back. A
        // rejection that no metric records is an overload an operator finds out
        // about from a customer.
        .layer(axum::middleware::from_fn_with_state(admission, admit))
        // `layer`, not `route_layer`: this must run after routing so
        // `MatchedPath` is in the extensions and the metric label is the route
        // template rather than the concrete path.
        .layer(axum::middleware::from_fn(observability::observe))
        // Interactive documentation over the same contract the API serves.
        //
        // **A plain path, with no wildcard.** The crate appends its own —
        // `/docs` redirects to `/docs/`, `/docs/` is the page, and
        // `/docs/{*rest}` serves its CSS and JS — so passing a wildcard here
        // makes it register a conflicting pair and panic inside `app()`, which
        // fails every test that builds a router rather than just this route.
        //
        // Configured with the *URL*, not with a parsed document. `SwaggerUi::url()`
        // wants `utoipa::openapi::OpenApi`, and converting into it does not
        // merely risk detail loss — it fails outright: utoipa serializes
        // OpenAPI 3.1's nullable form (`"type": ["string", "null"]`) and cannot
        // deserialize it, on the very schemas its own derive produced. Pointing
        // at the URL keeps one source of truth for the contract.
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/docs")
                .config(utoipa_swagger_ui::Config::new(["/openapi.json"])),
        )
        // Mounted LAST so the SPA fallback cannot swallow an unknown API path.
        // A fallback registered first turns every mistyped endpoint into a 200
        // text/html and the client sees a blank page instead of an error.
        .merge(graph_owl_ui::router())
}

/// Take a permit for the expensive paths, or refuse the request outright.
///
/// The permit is bound for the whole of `next.run` and dropped when this scope
/// ends — releasing it any earlier would let the semaphore admit a second
/// request while the first is still holding a connection, which is a limit that
/// counts *arrivals* rather than concurrency and therefore no limit at all.
///
/// The route **template** decides, not the path: reading the concrete URI would
/// mean `/assets/<uuid>/graph` matched nothing and the most expensive read in
/// the API went uncontrolled.
async fn admit(
    State(admission): State<Arc<admission::Admission>>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(axum::extract::MatchedPath::as_str)
        .map(ToString::to_string);

    let Some(class) = route.as_deref().and_then(admission::class_of) else {
        return next.run(request).await;
    };

    // `_permit`, not `_`. A binding named `_` drops at the end of the
    // statement, which would release the permit before the handler had even
    // started — a limit on arrivals rather than on concurrency, and one that
    // still passes a naive test because the first request is always admitted.
    // A leading underscore keeps it bound to the end of this scope.
    let Some(_permit) = admission.try_admit(class) else {
        return AppError::Overloaded {
            class: class.label(),
            retry_after_seconds: admission.retry_after_seconds(),
        }
        .into_response();
    };

    next.run(request).await
}

async fn create_table(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateTable>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, String); 1],
        Json<Table>,
    ),
    AppError,
> {
    let table = catalog.create_table(&principal, payload).await?;
    // Built from the returned id, never reassembled from the request — a client
    // following the header must land on the thing that was actually created.
    let location = format!("/tables/{}", table.id);
    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(table),
    ))
}

/// `deny_unknown_fields` so a typo'd filter fails loudly. `GET /tables?ownr=x`
/// silently returning the unfiltered collection is a data-leak-shaped bug: the
/// client believes it applied a restriction that was never applied.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    limit: Option<usize>,
    after: Option<String>,
}

async fn list_tables(
    State(catalog): State<Catalog>,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<Table>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.list_tables(&page).await?))
}

async fn get_table(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Table>, AppError> {
    catalog
        .get_table(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn update_table(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(update): AppJson<TableUpdate>,
) -> Result<Json<Table>, AppError> {
    catalog
        .update_table(&principal, id, update)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn delete_table(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if catalog.delete_table(&principal, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

async fn create_relationship(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<CreateRelationship>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, String); 1],
        Json<Relationship>,
    ),
    AppError,
> {
    let relationship = catalog.create_relationship(&principal, id, payload).await?;
    let location = format!("/relationships/{}", relationship.id);
    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(relationship),
    ))
}

async fn list_relationships_for_table(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Relationship>>, AppError> {
    catalog
        .list_relationships_for_table(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn delete_relationship(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if catalog.delete_relationship(&principal, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

/// The **only** place a `Principal` is constructed from a request.
///
/// Epic 12 replaces this body with token verification. Nothing else in the
/// server may build a principal, so that swap stays a one-file change — which
/// is the entire point of threading it through handlers now.
struct Auth(Principal);

/// Verified claims. Deliberately minimal: an identity and a display name.
/// Roles come from the catalog's own user record, not from the token — a token
/// that carries its own authorisation makes revocation impossible until it
/// expires.
#[derive(serde::Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[allow(dead_code)]
    exp: usize,
    /// Everything else the provider sent. Needed because the claim carrying
    /// roles is named by configuration — Auth0 namespaces custom claims, so
    /// there is no portable field to declare.
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// The signing secret. Read once at startup.
///
/// HS256 with a shared secret is the demo posture; Epic 12's JWKS path replaces
/// this function and nothing else, which is the payoff of the seam.
fn signing_secret() -> Option<String> {
    std::env::var("GRAPH_OWL_JWT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Whether OIDC JWKS authentication is configured.
fn oidc_config() -> Option<(String, String)> {
    let issuer = std::env::var("OIDC_ISSUER")
        .ok()
        .filter(|s| !s.is_empty())?;
    let audience =
        std::env::var("OIDC_AUDIENCE").unwrap_or_else(|_| "https://graph-owl.dev/api".to_string());
    Some((issuer, audience))
}

/// How a request is authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// RS256 against keys fetched from an OIDC issuer.
    Oidc,
    /// HS256 against a shared secret. Legacy, and a demo affordance.
    SharedSecret,
    /// Every request is the system principal.
    Open,
}

/// Resolve the authentication mode from what is configured.
///
/// **OIDC wins when both are set, and that is the whole point of this being a
/// function.** The natural implementation checks the shared secret first
/// because it is cheaper, which silently downgrades exactly the deployment
/// most at risk: one migrating to OIDC that has not yet removed
/// `GRAPH_OWL_JWT_SECRET` from its environment. Nothing about that deployment
/// looks wrong — OIDC is configured, the console signs in against the provider,
/// and the server is quietly still trusting a shared secret that anyone who
/// ever had it can still mint tokens with.
///
/// Refusing to start would be defensible, but it turns a stale environment
/// variable into an outage. Preferring the stronger mode and saying so is the
/// same protection without the outage.
#[must_use]
pub fn auth_mode(shared_secret: bool, oidc: bool) -> AuthMode {
    match (oidc, shared_secret) {
        (true, _) => AuthMode::Oidc,
        (false, true) => AuthMode::SharedSecret,
        (false, false) => AuthMode::Open,
    }
}

/// What a browser must know before it can authenticate against this server.
///
/// Served unauthenticated, because requiring a token to discover how to obtain
/// a token is circular. It carries no secret: in shared-secret mode the only
/// credential is server-side, and a mode name is not one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    /// `oidc`, `sharedSecret`, or `open`.
    pub mode: &'static str,
    /// Omitted entirely unless the mode is `oidc` — see [`auth_config`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// Describe this server's authentication to the console.
///
/// **The issuer is reported only in OIDC mode, and that is the whole point of
/// the function.** The natural implementation reports whatever `OIDC_ISSUER`
/// holds and lets the console decide, which reintroduces the bug this was
/// written for: a developer's `.env` keeps its OIDC settings across every run,
/// so a server started in shared-secret mode still has an issuer in its
/// environment. A console told about that issuer signs the user in against it,
/// receives a perfectly valid RS256 token, and hands it to a server that will
/// only ever accept HS256 — a loop in which every individual step succeeds.
///
/// So the rule is that the *verifier* decides. An issuer this server would not
/// verify against is not reported, because reporting it is an instruction to go
/// and fail.
#[must_use]
pub fn auth_config(
    mode: AuthMode,
    oidc: Option<(String, String)>,
    console_client_id: Option<String>,
) -> AuthConfig {
    match mode {
        AuthMode::Oidc => {
            let (issuer, audience) = oidc.unzip();
            AuthConfig {
                mode: "oidc",
                issuer,
                audience,
                client_id: console_client_id,
            }
        }
        // Both non-OIDC modes discard the provider details deliberately. See
        // above: the absence of an issuer is the signal that no interactive
        // provider can produce a token this server accepts.
        AuthMode::SharedSecret => AuthConfig {
            mode: "sharedSecret",
            issuer: None,
            audience: None,
            client_id: None,
        },
        AuthMode::Open => AuthConfig {
            mode: "open",
            issuer: None,
            audience: None,
            client_id: None,
        },
    }
}

/// Roles carried by a token, from the claim `OIDC_ROLES_CLAIM` names.
///
/// **Opt-in, and off by default.** An identity provider's claim becoming a
/// role here means the provider decides what this catalog authorizes — which is
/// a reasonable arrangement and a terrible default, because it is invisible.
/// With no claim configured the token contributes nothing and roles come from
/// the catalog alone, which is what shipped before this existed.
///
/// The claim is a JSON array of strings; anything else contributes nothing. A
/// provider that emits a bare string, an object, or numbers is not producing
/// roles this understands, and inventing an interpretation would grant access
/// on the strength of a guess.
///
/// Auth0 namespaces custom claims (`https://example.com/roles`), so the claim
/// name is configuration rather than a constant — there is no portable name to
/// hard-code.
#[must_use]
pub fn roles_from_claims(
    extra: &serde_json::Map<String, serde_json::Value>,
    claim: &str,
) -> Vec<String> {
    if claim.is_empty() {
        return Vec::new();
    }
    extra
        .get(claim)
        .and_then(serde_json::Value::as_array)
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| role.as_str())
                .filter(|role| !role.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a subject is designated an administrator by deployment
/// configuration.
///
/// **This exists because the first login otherwise looks broken.** A user
/// arriving from an identity provider is auto-provisioned with no roles, and
/// authorization denies by default, so a completely successful sign-in shows an
/// empty catalog — which is the exact failure `00f` says the console must never
/// present, delivered by the server instead. Granting the first role required
/// direct SQL, which is not a workable answer for anyone's first run.
///
/// `GRAPH_OWL_ADMIN_SUBJECTS` is a comma-separated list of `sub` claims. It is
/// deliberately **not** a database write: elevation is re-evaluated from the
/// environment on every request, so removing the variable and restarting
/// revokes it. A stored `is_admin` flag would outlive the configuration that
/// created it and quietly stay true.
///
/// Matching is exact and whitespace-trimmed. An empty entry never matches
/// anything — a trailing comma is a typo, not a grant of admin to the subject
/// whose id is the empty string.
#[must_use]
pub fn is_bootstrap_admin(subject: &str, configured: &str) -> bool {
    !subject.is_empty()
        && configured
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .any(|entry| entry == subject)
}

/// Whether a configuration is one an operator should be warned about.
///
/// Both configured is not an error — the stronger one is used — but it is
/// always a mistake, and a silent one. The secret is dead weight at best and a
/// live credential someone believes is in use at worst.
#[must_use]
pub fn is_ambiguous_auth_config(shared_secret: bool, oidc: bool) -> bool {
    shared_secret && oidc
}

/// Extract a bearer token from the Authorization header.
fn bearer_token(parts: &axum::http::request::Parts) -> Result<&str, AppError> {
    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthenticated)
}

/// Verify a token against the OIDC provider's JWKS and resolve the principal.
async fn verify_jwks(
    token: &str,
    jwks: &jwks::JwksClient,
    catalog: &Catalog,
) -> Result<Auth, AppError> {
    let header =
        jsonwebtoken::decode_header(token).map_err(|e| AppError::TokenInvalid(e.to_string()))?;

    let kid = header.kid.ok_or(AppError::TokenInvalid(
        "token is missing the `kid` header".to_string(),
    ))?;

    let decoding_key = jwks
        .decoding_key(&kid)
        .await
        .map_err(|e| AppError::TokenInvalid(e.to_string()))?;

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&[jwks.issuer()]);
    validation.set_audience(&[jwks.audience()]);
    // Auth0 tokens include `iat` but jsonwebtoken does not require it by
    // default. Keep that: missing `iat` is not a reason to reject.
    validation.set_required_spec_claims(&["exp", "sub", "iss", "aud"]);

    let claims = jsonwebtoken::decode::<Claims>(token, &decoding_key, &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AppError::TokenExpired,
            jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                AppError::TokenInvalid("issuer does not match".to_string())
            }
            jsonwebtoken::errors::ErrorKind::InvalidAudience => {
                AppError::TokenInvalid("audience does not match".to_string())
            }
            _ => AppError::TokenInvalid(e.to_string()),
        })?
        .claims;

    let name = claims.name.unwrap_or_else(|| claims.sub.clone());
    let mut principal = catalog
        .resolve_principal(&claims.sub, &name)
        .await
        .map_err(AppError::from)?;

    // Merged, not replaced: a role granted in the catalog is not withdrawn
    // because the provider did not also mention it. Deduplicated, because the
    // same role from both sources is one role, and a repeated one would be
    // looked up twice on every authorization decision.
    for role in roles_from_claims(
        &claims.extra,
        &std::env::var("OIDC_ROLES_CLAIM").unwrap_or_default(),
    ) {
        if !principal.roles.contains(&role) {
            principal.roles.push(role);
        }
    }

    // Applied after resolution, never written back. See `is_bootstrap_admin`.
    if is_bootstrap_admin(
        &claims.sub,
        &std::env::var("GRAPH_OWL_ADMIN_SUBJECTS").unwrap_or_default(),
    ) {
        principal.is_admin = true;
    }
    Ok(Auth(principal))
}

/// Leave the identity where the access log can find it.
///
/// Called on every path that resolves one, including open mode — a log line
/// naming `system` is what tells an operator the server is running unsecured,
/// which is the same thing the startup warning says and the only place it is
/// visible per-request.
fn record_principal(parts: &axum::http::request::Parts, principal: &Principal) {
    if let Some(slot) = parts.extensions.get::<observability::RequestPrincipal>() {
        slot.set(&principal.id);
    }
}

/// **The single place a `Principal` is constructed from a request.**
///
/// Authentication follows this precedence:
///
/// 1. `OIDC_ISSUER` — RS256 via JWKS from an OIDC provider.
/// 2. `GRAPH_OWL_JWT_SECRET` — HS256 shared secret (legacy/demo).
/// 3. Neither — open mode: every request is the system principal.
///
/// **OIDC first, deliberately** — see [`auth_mode`]. Checking the cheaper
/// shared secret first silently downgrades a deployment that has configured
/// OIDC but not yet removed its old secret, which is the one deployment where
/// the downgrade is invisible and the old credential is still live.
///
/// Open mode is logged as a warning at startup because a server that is
/// accidentally open must not look identical to a secured one.
impl<S> FromRequestParts<S> for Auth
where
    S: Send + Sync,
    Catalog: axum::extract::FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // OIDC / JWKS — key material fetched from the issuer. Checked first
        // because `auth_mode` prefers it: a deployment with a stale
        // `GRAPH_OWL_JWT_SECRET` beside a configured issuer must not be
        // silently downgraded to the shared secret.
        if let Some(jwks) = parts.extensions.get::<std::sync::Arc<jwks::JwksClient>>() {
            let token = bearer_token(parts)?;
            let catalog = <Catalog as axum::extract::FromRef<S>>::from_ref(state);
            let auth = verify_jwks(token, jwks, &catalog).await?;
            record_principal(parts, &auth.0);
            return Ok(auth);
        }

        // HS256 shared secret (legacy/demo).
        if let Some(secret) = signing_secret() {
            let token = bearer_token(parts)?;
            let claims = jsonwebtoken::decode::<Claims>(
                token,
                &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
                &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
            )
            .map_err(|_| AppError::Unauthenticated)?
            .claims;

            let catalog = <Catalog as axum::extract::FromRef<S>>::from_ref(state);
            let name = claims.name.unwrap_or_else(|| claims.sub.clone());
            let principal = catalog
                .resolve_principal(&claims.sub, &name)
                .await
                .map_err(AppError::from)?;
            record_principal(parts, &principal);
            return Ok(Auth(principal));
        }

        // No secret and no OIDC — open mode.
        let principal = Principal::system();
        record_principal(parts, &principal);
        Ok(Auth(principal))
    }
}

/// Wraps [`Query`] so a rejection becomes problem+json like every other error.
///
/// axum's own rejection is plain text, which would make query-parameter
/// failures the one error shape a client cannot parse — and `deny_unknown_fields`
/// makes this a path clients hit routinely, not an edge case.
struct AppQuery<T>(T);

impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Query(value) =
            Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|rejection| {
                    AppError::Validation(vec![FieldError::new(
                        "query",
                        FieldErrorCode::Type,
                        rejection.body_text(),
                    )])
                })?;
        Ok(AppQuery(value))
    }
}

/// Wraps [`Json`] to return `400 Bad Request` rather than axum's default `422`,
/// and to report **every** field violation in one response.
///
/// The body is parsed to a [`serde_json::Value`] first, because `serde`'s derived
/// deserializer stops at its first error — which forces a client into one round
/// trip per mistake. Validation runs over the untyped document, accumulating
/// failures, and only a clean document is deserialized into `T`.
/// A query string with the `extension.*` filters split off — Epic 22 Slice D.
///
/// **They cannot go through `AppQuery`**, and the reason is the rule that makes
/// `AppQuery` worth having. Custom-property names are defined at runtime, so
/// they cannot appear in a struct; the only serde shape that accepts them is a
/// flattened map, and a flattened map absorbs *every* unrecognised parameter —
/// which silently repeals `deny_unknown_fields` for the whole endpoint and turns
/// `?ownr=alice` back into a filter that matches everything.
///
/// So they are removed from the raw query first, and what remains is
/// deserialized by the same strict extractor as before. A typo'd `extension.*`
/// name is still caught, one layer down, by the facade checking it against the
/// definitions.
/// The `extension.*` filters as they arrive: a property name, a comparison and
/// the raw text. Untyped until the facade resolves each against its definition
/// — deciding that `30` is the number thirty needs the definition, which this
/// layer does not have.
type RequestedFilters = Vec<(String, graph_owl_storage::ExtensionOp, String)>;

struct AppQueryWithExtensions<T>(T, RequestedFilters);

impl<S, T> FromRequestParts<S> for AppQueryWithExtensions<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts.uri.query().unwrap_or_default().to_string();
        let (extensions, rest) = split_extension_filters(&raw)?;

        // Rebuilt onto the URI so the inner extractor sees a query string with
        // no `extension.*` in it at all. Reconstructing the whole `Parts` would
        // be the alternative, and it is more code doing the same thing.
        let mut builder = axum::http::Uri::builder();
        if let Some(scheme) = parts.uri.scheme() {
            builder = builder.scheme(scheme.clone());
        }
        if let Some(authority) = parts.uri.authority() {
            builder = builder.authority(authority.clone());
        }
        let path = parts.uri.path();
        let path_and_query = if rest.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{rest}")
        };
        if let Ok(uri) = builder.path_and_query(path_and_query).build() {
            parts.uri = uri;
        }

        let AppQuery(value) = AppQuery::<T>::from_request_parts(parts, state).await?;
        Ok(AppQueryWithExtensions(value, extensions))
    }
}

/// Wraps [`Json`] to return `400 Bad Request` rather than axum's default `422`,
/// and to report **every** field violation in one response.
///
/// The body is parsed to a [`serde_json::Value`] first, because `serde`'s derived
/// deserializer stops at its first error — which forces a client into one round
/// trip per mistake. Validation runs over the untyped document, accumulating
/// failures, and only a clean document is deserialized into `T`.
struct AppJson<T>(T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + ValidateBody,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(document) = Json::<serde_json::Value>::from_request(req, state)
            .await
            .map_err(|rejection: JsonRejection| AppError::MalformedBody(rejection.body_text()))?;

        let errors = T::validate_body(&document);
        if !errors.is_empty() {
            return Err(AppError::Validation(errors));
        }

        let value = serde_json::from_value(document)
            .map_err(|error| AppError::MalformedBody(error.to_string()))?;
        Ok(AppJson(value))
    }
}

/// Base for every `type` URI. Clients branch on these, never on prose, so the
/// strings are part of the wire contract and must not be reworded.
const PROBLEM_TYPE_BASE: &str = "https://graph-owl.dev/errors/";

enum AppError {
    /// The body was not parseable as the expected shape.
    MalformedBody(String),
    /// The body is a well-formed document, but one or more fields are invalid.
    /// Carries every violation, never just the first.
    Validation(Vec<FieldError>),
    Conflict {
        detail: String,
        existing_id: Option<Uuid>,
        kind: ConflictKind,
    },
    Internal(String),
    NotFound,
    /// `If-Match` named a version that is no longer current.
    PreconditionFailed {
        current: String,
    },
    /// No credential, or one that does not verify.
    Unauthenticated,
    /// Authenticated but not authorised — distinct from missing authentication.
    /// Will be constructed by the authorization middleware (Epic 14 / roles).
    #[allow(dead_code)]
    Forbidden,
    /// The bearer token has expired.
    TokenExpired,
    /// The bearer token is structurally invalid (wrong signature, issuer, or
    /// audience).
    TokenInvalid(String),
    /// The triple is well-formed and meaningless. Its own identity because a
    /// client fixes it by choosing a different relationship, not a value.
    IllegalRelationship {
        from: &'static str,
        relationship: &'static str,
        to: &'static str,
    },
    /// No permit was available on an admission-controlled path. The request is
    /// **refused, not queued** — see `admission`. Distinct from every other
    /// error here in that nothing about the request is wrong: it is the only
    /// variant a client is told to simply send again.
    Overloaded {
        class: &'static str,
        retry_after_seconds: u64,
    },
    /// A webhook endpoint's own configured rate limit was exceeded — Epic
    /// 18 Slice E. Distinct from `Overloaded`: this is not the server
    /// protecting its own resources, it is one specific sender's own
    /// budget, and a different endpoint's traffic is entirely unaffected.
    RateLimited {
        retry_after_seconds: u64,
    },
    /// An agent write was refused — Epic 32.
    ///
    /// **Its own variant rather than `Forbidden`**, because the caller is a
    /// program: it needs to know *which* rule refused and what would change the
    /// answer. `Forbidden` renders a fixed sentence, which would replace exactly
    /// the part an agent could act on — the capability to request, the scope it
    /// strayed outside, the seconds until its budget frees up.
    ///
    /// `retry_after_seconds` is `Some` only for the rate limit, which is also
    /// the only refusal here that becomes a `429` rather than a `403`: it is the
    /// one that will stop being true on its own.
    AgentRefused {
        detail: String,
        retry_after_seconds: Option<u64>,
    },
}

impl AppError {
    /// Stable, machine-readable identity. Distinct per variant — a client
    /// branches on this, so two variants sharing a slug is a contract bug.
    fn problem_slug(&self) -> &'static str {
        match self {
            AppError::MalformedBody(_) => "malformed-body",
            AppError::Validation(_) => "validation-failed",
            AppError::Conflict {
                kind: ConflictKind::Fqn,
                ..
            } => "fqn-conflict",
            AppError::Conflict {
                kind: ConflictKind::RelationshipTuple,
                ..
            } => "relationship-conflict",
            AppError::Conflict {
                kind: ConflictKind::DomainAssigned,
                ..
            } => "domain-already-assigned",
            AppError::Conflict {
                kind: ConflictKind::TagInUse,
                ..
            } => "tag-in-use",
            AppError::Conflict {
                kind: ConflictKind::TagExclusive,
                ..
            } => "tag-mutually-exclusive",
            AppError::Conflict {
                kind: ConflictKind::DomainInUse,
                ..
            } => "domain-in-use",
            AppError::Conflict {
                kind: ConflictKind::ProposalDecided,
                ..
            } => "proposal-already-decided",
            AppError::AgentRefused {
                retry_after_seconds: None,
                ..
            } => "agent-refused",
            AppError::AgentRefused {
                retry_after_seconds: Some(_),
                ..
            } => "agent-rate-limited",
            AppError::Conflict {
                kind: ConflictKind::WaiverExists,
                ..
            } => "waiver-exists",
            AppError::Conflict {
                kind: ConflictKind::AssignmentExists,
                ..
            } => "assignment-exists",
            AppError::Conflict {
                kind: ConflictKind::MemoryExists,
                ..
            } => "memory-exists",
            AppError::Conflict {
                kind: ConflictKind::PrincipalStillHolds,
                ..
            } => "principal-still-holds",
            AppError::Conflict {
                kind: ConflictKind::IdempotencyConflict,
                ..
            } => "idempotency-conflict",
            AppError::Conflict {
                kind: ConflictKind::GlossaryHasTerms,
                ..
            } => "glossary-has-terms",
            AppError::Conflict {
                kind: ConflictKind::MergeAlreadySplit,
                ..
            } => "merge-already-split",
            AppError::Conflict {
                kind: ConflictKind::ReviewAlreadyDecided,
                ..
            } => "review-already-decided",
            AppError::Conflict {
                kind: ConflictKind::WebhookPathExists,
                ..
            } => "webhook-path-exists",
            AppError::Conflict {
                kind: ConflictKind::StreamSubscriptionExists,
                ..
            } => "stream-subscription-exists",
            AppError::Conflict {
                kind: ConflictKind::CustomPropertyExists,
                ..
            } => "custom-property-exists",
            AppError::Internal(_) => "internal-error",
            AppError::NotFound => "not-found",
            AppError::PreconditionFailed { .. } => "version-conflict",
            AppError::Unauthenticated => "unauthenticated",
            AppError::Forbidden => "forbidden",
            AppError::TokenExpired => "token-expired",
            AppError::TokenInvalid(_) => "token-invalid",
            AppError::IllegalRelationship { .. } => "illegal-relationship",
            AppError::Overloaded { .. } => "overloaded",
            AppError::RateLimited { .. } => "rate-limited",
        }
    }

    /// Short human-readable summary. Constant per variant per RFC 9457 —
    /// per-occurrence information belongs in `detail`.
    fn title(&self) -> &'static str {
        match self {
            AppError::MalformedBody(_) => "Malformed request body",
            AppError::Validation(_) => "Validation failed",
            AppError::Conflict {
                kind: ConflictKind::Fqn,
                ..
            } => "Fully-qualified name already exists",
            AppError::Conflict {
                kind: ConflictKind::RelationshipTuple,
                ..
            } => "Relationship already exists",
            AppError::Conflict {
                kind: ConflictKind::DomainAssigned,
                ..
            } => "This asset already belongs to a domain",
            AppError::Conflict {
                kind: ConflictKind::ProposalDecided,
                ..
            } => "This proposal was already decided",
            AppError::AgentRefused {
                retry_after_seconds: None,
                ..
            } => "This agent may not do that",
            AppError::AgentRefused {
                retry_after_seconds: Some(_),
                ..
            } => "This agent has used its budget",
            AppError::Conflict {
                kind: ConflictKind::TagInUse,
                ..
            } => "This tag is still in use",
            AppError::Conflict {
                kind: ConflictKind::TagExclusive,
                ..
            } => "That classification permits only one tag",
            AppError::Conflict {
                kind: ConflictKind::DomainInUse,
                ..
            } => "This domain still holds things",
            AppError::Conflict {
                kind: ConflictKind::WaiverExists,
                ..
            } => "This finding is already waived",
            AppError::Conflict {
                kind: ConflictKind::AssignmentExists,
                ..
            } => "This finding is already assigned",
            AppError::Conflict {
                kind: ConflictKind::MemoryExists,
                ..
            } => "A memory with this id already exists",
            AppError::Conflict {
                kind: ConflictKind::PrincipalStillHolds,
                ..
            } => "This principal still holds things",
            AppError::Conflict {
                kind: ConflictKind::IdempotencyConflict,
                ..
            } => "Idempotency key conflict",
            AppError::Conflict {
                kind: ConflictKind::GlossaryHasTerms,
                ..
            } => "This glossary still has terms",
            AppError::Conflict {
                kind: ConflictKind::MergeAlreadySplit,
                ..
            } => "This merge has already been split",
            AppError::Conflict {
                kind: ConflictKind::ReviewAlreadyDecided,
                ..
            } => "This review entry has already been decided",
            AppError::Conflict {
                kind: ConflictKind::WebhookPathExists,
                ..
            } => "This path is already registered to another endpoint",
            AppError::Conflict {
                kind: ConflictKind::StreamSubscriptionExists,
                ..
            } => "This topic and consumer group are already registered to another subscription",
            AppError::Conflict {
                kind: ConflictKind::CustomPropertyExists,
                ..
            } => "That custom property is already defined on this entity type",
            AppError::Internal(_) => "Internal server error",
            AppError::NotFound => "Resource not found",
            AppError::PreconditionFailed { .. } => "Version precondition failed",
            AppError::Unauthenticated => "Authentication required",
            AppError::Forbidden => "Forbidden",
            AppError::TokenExpired => "Token expired",
            AppError::TokenInvalid(_) => "Token invalid",
            AppError::IllegalRelationship { .. } => "Illegal relationship",
            AppError::Overloaded { .. } => "Server overloaded",
            AppError::RateLimited { .. } => "Rate limit exceeded",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            AppError::MalformedBody(_)
            | AppError::Validation(_)
            | AppError::IllegalRelationship { .. } => StatusCode::BAD_REQUEST,
            AppError::Conflict { .. } => StatusCode::CONFLICT,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::PreconditionFailed { .. } => StatusCode::PRECONDITION_FAILED,
            AppError::Unauthenticated | AppError::TokenExpired | AppError::TokenInvalid(_) => {
                StatusCode::UNAUTHORIZED
            }
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::Overloaded { .. } => StatusCode::SERVICE_UNAVAILABLE,
            AppError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            // The rate limit is the one refusal that stops being true on its
            // own, so it is the one that says "try again" rather than "not you".
            AppError::AgentRefused {
                retry_after_seconds: Some(_),
                ..
            } => StatusCode::TOO_MANY_REQUESTS,
            AppError::AgentRefused { .. } => StatusCode::FORBIDDEN,
        }
    }

    fn detail(&self) -> String {
        match self {
            AppError::MalformedBody(message) | AppError::Internal(message) => message.clone(),
            AppError::Validation(errors) => {
                let plural = if errors.len() == 1 { "field" } else { "fields" };
                format!("{} {plural} failed validation", errors.len())
            }
            AppError::Conflict {
                detail,
                kind: ConflictKind::Fqn,
                ..
            } => format!("an entity with fullyQualifiedName '{detail}' already exists"),
            AppError::Conflict {
                detail,
                kind: ConflictKind::RelationshipTuple,
                ..
            } => format!("the relationship '{detail}' already exists"),
            // **Both pass the detail through**, for the reason the codebase has
            // now learned twice: a canned sentence per kind silently replaces
            // whatever the facade wrote, and here that is the name of the
            // current domain and the counts it still holds — the only parts an
            // operator can act on.
            AppError::Conflict {
                kind: ConflictKind::DomainAssigned | ConflictKind::DomainInUse,
                detail,
                ..
            } => detail.clone(),
            // Same rule: the counts by kind and the name of the conflicting tag
            // are the only parts an operator can act on, and a canned sentence
            // per kind would silently replace them.
            AppError::Conflict {
                kind: ConflictKind::TagInUse | ConflictKind::TagExclusive,
                detail,
                ..
            } => detail.clone(),
            // Same rule again: the refusal already names the rule and what would
            // change the answer, and that is the whole value of it to a program.
            AppError::Conflict {
                kind: ConflictKind::ProposalDecided,
                detail,
                ..
            }
            | AppError::AgentRefused { detail, .. } => detail.clone(),
            AppError::Conflict {
                kind: ConflictKind::WaiverExists,
                ..
            } => "this finding already has a waiver; revoke it before recording \
                  a different reason"
                .to_string(),
            AppError::Conflict {
                kind: ConflictKind::AssignmentExists,
                ..
            } => "this finding is already assigned; two owners is no owner".to_string(),
            AppError::Conflict {
                kind: ConflictKind::MemoryExists,
                ..
            } => "a memory with this id already exists".to_string(),
            // **The detail passes through**, unlike every other conflict here: the
            // counts by kind are the actionable part, and a canned sentence cannot
            // carry "1 service, 3 schemas, 396 columns".
            AppError::Conflict {
                kind: ConflictKind::PrincipalStillHolds,
                detail,
                ..
            } => detail.clone(),
            // Passes through for the same reason: the key and what went wrong with
            // it are the actionable part, and a canned sentence cannot carry them.
            AppError::Conflict {
                kind: ConflictKind::IdempotencyConflict,
                detail,
                ..
            } => detail.clone(),
            // The term count is the actionable part — same rule as
            // `PrincipalStillHolds`.
            AppError::Conflict {
                kind: ConflictKind::GlossaryHasTerms,
                detail,
                ..
            } => detail.clone(),
            // The original split time is the actionable part — same rule as
            // `PrincipalStillHolds`.
            AppError::Conflict {
                kind: ConflictKind::MergeAlreadySplit,
                detail,
                ..
            } => detail.clone(),
            AppError::Conflict {
                kind: ConflictKind::ReviewAlreadyDecided,
                detail,
                ..
            } => detail.clone(),
            // The path itself is the actionable detail — same rule as
            // `PrincipalStillHolds`. Passed through rather than wrapped: the
            // storage layer's message ("path '…' is already registered") is
            // already a complete sentence.
            AppError::Conflict {
                kind: ConflictKind::WebhookPathExists,
                detail,
                ..
            } => detail.clone(),
            // Same reasoning as `WebhookPathExists`: the storage layer's
            // message already names the topic and consumer group.
            AppError::Conflict {
                kind: ConflictKind::StreamSubscriptionExists,
                detail,
                ..
            } => detail.clone(),
            // The detail names the *pair*, because the same name on a
            // different entity type is allowed — a caller told only "conflict"
            // cannot tell which of the two it needs to change.
            AppError::Conflict {
                kind: ConflictKind::CustomPropertyExists,
                detail,
                ..
            } => detail.clone(),
            AppError::NotFound => "the requested resource does not exist".to_string(),
            AppError::PreconditionFailed { current } => format!(
                "this asset is now at version {current}; your `If-Match` named an \
                 earlier one. Re-read it and re-apply your change — proceeding \
                 would discard whatever was written in between"
            ),
            AppError::Unauthenticated => {
                "a valid bearer token is required for this request".to_string()
            }
            AppError::Forbidden => {
                "you do not have permission to perform this operation".to_string()
            }
            AppError::TokenExpired => "the bearer token has expired; refresh and retry".to_string(),
            AppError::TokenInvalid(reason) => {
                format!("the bearer token is invalid: {reason}")
            }
            AppError::IllegalRelationship {
                from,
                relationship,
                to,
            } => format!("`{from}` may not `{relationship}` a `{to}`"),
            // Names the class, because "the server is busy" and "the *ingestion*
            // path is busy" call for different responses: the first says stop,
            // the second says this one endpoint is saturated and the rest of the
            // catalog is still answering.
            AppError::Overloaded {
                class,
                retry_after_seconds,
            } => format!(
                "the {class} path is at its concurrency limit and this request was refused \
                 rather than queued. Retry after {retry_after_seconds}s — nothing about the \
                 request itself is wrong"
            ),
            AppError::RateLimited {
                retry_after_seconds,
            } => format!(
                "this endpoint's own rate limit was exceeded. Retry after {retry_after_seconds}s"
            ),
        }
    }
}

impl From<PageRequestError> for AppError {
    fn from(error: PageRequestError) -> Self {
        let field_error = match error {
            PageRequestError::LimitTooLarge { requested, max } => FieldError::new(
                "limit",
                FieldErrorCode::Type,
                format!("`limit` must be at most {max}, got {requested}"),
            ),
            PageRequestError::LimitZero => FieldError::new(
                "limit",
                FieldErrorCode::Type,
                "`limit` must be at least 1".to_string(),
            ),
            // Opaque by design, so there is nothing useful to say about *why* it
            // failed to decode — only that the client must not construct one.
            PageRequestError::MalformedCursor => FieldError::new(
                "after",
                FieldErrorCode::Type,
                "`after` is not a cursor this server issued".to_string(),
            ),
        };
        AppError::Validation(vec![field_error])
    }
}

impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Conflict {
                detail,
                existing_id,
                kind,
            } => AppError::Conflict {
                detail,
                existing_id,
                kind,
            },
            StorageError::Unexpected(message) => AppError::Internal(message),
        }
    }
}

impl From<CatalogError> for AppError {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::NotFound => AppError::NotFound,
            CatalogError::PreconditionFailed { current } => AppError::PreconditionFailed {
                current: format!("{}.{}", current.major, current.minor),
            },
            CatalogError::Conflict {
                detail,
                existing_id,
                kind,
            } => AppError::Conflict {
                detail,
                existing_id,
                kind,
            },
            CatalogError::Validation(errors) => AppError::Validation(errors),
            CatalogError::IllegalRelationship {
                from,
                relationship,
                to,
            } => AppError::IllegalRelationship {
                from: from.as_str(),
                relationship: relationship.as_str(),
                to: to.as_str(),
            },
            CatalogError::Forbidden => AppError::Forbidden,
            CatalogError::Unauthenticated => AppError::Unauthenticated,
            CatalogError::AgentRefused(refusal) => AppError::AgentRefused {
                detail: refusal.to_string(),
                retry_after_seconds: match refusal {
                    graph_owl_authz::agent::Refusal::RateLimited {
                        retry_after_seconds,
                        ..
                    } => Some(retry_after_seconds),
                    _ => None,
                },
            },
            CatalogError::Storage(storage_error) => storage_error.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut body = json!({
            "type": format!("{PROBLEM_TYPE_BASE}{}", self.problem_slug()),
            "title": self.title(),
            "status": status.as_u16(),
            "detail": self.detail(),
        });

        // Extension member: the per-field breakdown a client needs to fix a
        // request in one pass instead of one round trip per mistake.
        if let AppError::Validation(errors) = &self {
            body["errors"] = json!(errors);
        }

        // Extension member: only present when the adapter could identify the
        // row that was collided with.
        if let AppError::Conflict {
            existing_id: Some(id),
            ..
        } = &self
        {
            body["conflictingId"] = json!(id);
        }

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );

        // `Retry-After` is the half of a `503`/`429` that makes it
        // actionable. A rejection without one leaves every client to invent
        // its own backoff, and the ones that invent "immediately" are what
        // turn a shed load into a retry storm — the exact failure both
        // admission control and rate limiting exist to stop.
        let retry_after_seconds = match &self {
            AppError::Overloaded {
                retry_after_seconds,
                ..
            }
            | AppError::RateLimited {
                retry_after_seconds,
            } => Some(*retry_after_seconds),
            // Epic 32: a rate-limited agent gets the same treatment, and for the
            // same reason — an autonomous caller left to invent its own backoff
            // invents "immediately".
            AppError::AgentRefused {
                retry_after_seconds,
                ..
            } => *retry_after_seconds,
            _ => None,
        };
        if let Some(retry_after_seconds) = retry_after_seconds
            && let Ok(value) = axum::http::HeaderValue::from_str(&retry_after_seconds.to_string())
        {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }

        response
    }
}

/// Splits `extension.<name>` and `extension.<name>.<op>` pairs out of a raw
/// query string, returning them and everything else.
///
/// An unrecognised operator suffix is a `400` rather than being read as part of
/// the property name: `?extension.retentionDays.gt=30` is somebody meaning
/// `gte`, and treating it as a filter on a property called `retentionDays.gt`
/// answers with an empty page and no hint.
fn split_extension_filters(raw: &str) -> Result<(RequestedFilters, String), AppError> {
    use graph_owl_storage::ExtensionOp;
    const PREFIX: &str = "extension.";

    let mut filters = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    let mut errors = Vec::new();

    for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key);
        let Some(spec) = key.strip_prefix(PREFIX) else {
            rest.push(pair.to_string());
            continue;
        };
        let value = percent_decode(value.replace('+', " ").as_str());

        let (name, op) = match spec.rsplit_once('.') {
            Some((name, "gte")) => (name, ExtensionOp::Gte),
            Some((name, "lte")) => (name, ExtensionOp::Lte),
            Some((_, unknown)) => {
                errors.push(FieldError::new(
                    format!("{PREFIX}{spec}"),
                    FieldErrorCode::Value,
                    format!(
                        "`{unknown}` is not a comparison; use `gte` or `lte`, or omit \
                         it for equality"
                    ),
                ));
                continue;
            }
            None => (spec, ExtensionOp::Eq),
        };
        if name.is_empty() {
            errors.push(FieldError::new(
                format!("{PREFIX}{spec}"),
                FieldErrorCode::Required,
                "a custom property filter needs a property name",
            ));
            continue;
        }
        filters.push((name.to_string(), op, value));
    }

    if errors.is_empty() {
        Ok((filters, rest.join("&")))
    } else {
        Err(AppError::Validation(errors))
    }
}

/// Enough percent-decoding for a query parameter. `serde_urlencoded` does this
/// for the typed half; the `extension.*` half is peeled off before it runs, so
/// it has to do it here or a cost centre with a space in it would never match.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&raw[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- asset hierarchy (Epic 2) ----

/// `rename_all` beside `deny_unknown_fields`: every field here was a single
/// lowercase word until `dataProduct`, so the wire happened to be camelCase by
/// accident rather than by rule — and the first two-word filter shipped
/// `data_product` next to a wire that is camelCase everywhere else.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssetListQuery {
    kind: Option<String>,
    /// A user or team id — Epic 11 Slice E. Matches **effective** ownership, so
    /// a table with no owner of its own is matched by whoever owns its schema.
    ///
    /// Not `ownerKind`-qualified: `users.id` and `teams.id` can collide in
    /// principle, but a filter that matched the wrong one returns a wrong *page*
    /// rather than assigning accountability to the wrong principal, and requiring
    /// a second parameter on every steward's bookmarked URL is a worse trade
    /// than the ambiguity. The write path, where it matters, does require it.
    owner: Option<String>,
    /// The ownership-gap report: only assets with **no effective owner anywhere up
    /// their chain** — the query Slice D's `inherited` flag exists to make
    /// answerable, since inheriting without saying so turns a catalog nobody has
    /// assigned into one that reads as fully owned.
    ///
    /// A separate parameter rather than `owner=none`, because a sentinel would
    /// collide with a principal actually called `none`, and it lets the
    /// contradictory combination be refused rather than answered.
    unowned: Option<bool>,
    /// The accountability axis — Epic 23 Slice E. Matches **direct and
    /// inherited** assignment: "show me everything in the payments domain" is
    /// the query the epic exists for, and answering it with only the handful
    /// somebody assigned by hand would report a governed estate as almost
    /// empty.
    domain: Option<Uuid>,
    /// Membership of a data product.
    data_product: Option<Uuid>,
    limit: Option<usize>,
    after: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssetSearchQuery {
    q: String,
    kind: Option<String>,
    /// The same two filters the list endpoint takes, so a client does not have
    /// to learn two filtering languages — and so the one that got it wrong is
    /// not the one that silently returns more.
    domain: Option<Uuid>,
    data_product: Option<Uuid>,
    limit: Option<usize>,
    after: Option<String>,
}

/// An asset kind from a query parameter, naming what *is* supported when it is
/// not one.
fn parse_kind(raw: Option<&str>) -> Result<Option<AssetKind>, AppError> {
    raw.map(|value| {
        AssetKind::parse(value).map_err(|_| {
            AppError::Validation(vec![FieldError::new(
                "kind",
                FieldErrorCode::Type,
                format!(
                    "`{value}` is not an asset kind; expected one of: {}",
                    AssetKind::ALL
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        })
    })
    .transpose()
}

async fn upsert_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<UpsertAsset>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, String); 1],
        Json<Asset>,
    ),
    AppError,
> {
    let asset = catalog.upsert_asset(&principal, payload).await?;
    let location = format!("/assets/{}", asset.id);
    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(asset),
    ))
}

async fn list_assets(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppQueryWithExtensions(query, requested): AppQueryWithExtensions<AssetListQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    // **Contradictory, so refused rather than silently empty.** "Owned by X and
    // owned by nobody" has no answer, and returning an empty page for it would
    // look like a real result — the client would conclude X owns nothing.
    if query.owner.is_some() && query.unowned.unwrap_or(false) {
        return Err(AppError::Validation(vec![FieldError::new(
            "unowned",
            FieldErrorCode::Type,
            "`unowned` and `owner` cannot both be given: an asset cannot be owned by \
             a named principal and by nobody",
        )]));
    }
    // Resolved against the definitions before the query runs, so an undefined
    // name is a `400` rather than an empty page that reads like an answer.
    let extension = catalog.extension_filters(kind, &requested).await?;
    let (domain, data_product) = (query.domain, query.data_product);
    let filter = graph_owl_storage::AssetFilter {
        kind,
        owner: query.owner.as_deref(),
        unowned: query.unowned.unwrap_or(false),
        extension: &extension,
        domain,
        data_product,
    };
    Ok(Json(
        catalog.list_assets_for(&principal, &filter, &page).await?,
    ))
}

async fn search_assets(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppQueryWithExtensions(query, requested): AppQueryWithExtensions<AssetSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    let extension = catalog.extension_filters(kind, &requested).await?;
    let (domain, data_product) = (query.domain, query.data_product);
    let filter = graph_owl_storage::AssetFilter {
        kind,
        owner: None,
        unowned: false,
        extension: &extension,
        domain,
        data_product,
    };
    let page_result = catalog
        .search_assets_for(&principal, &query.q, &filter, &page)
        .await?;

    // Facets are computed over the *visible* set, like the counts. A facet
    // showing "core_banking (12)" to someone who may not see core_banking
    // leaks the schema's existence and its size.
    let mut by_kind: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut by_schema: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for asset in &page_result.data {
        *by_kind.entry(asset.kind.as_str()).or_default() += 1;
        // The schema is the third FQN segment: service.database.schema.…
        if let Some(schema) = asset.fully_qualified_name.split('.').nth(2) {
            *by_schema.entry(schema.to_string()).or_default() += 1;
        }
    }

    // Facets for enum-typed custom properties — Epic 22 Slice D. Only enums:
    // a facet is a short, closed list somebody can click, and a facet over a
    // free-text property is one bucket per value, which is a report rather than
    // a filter. Computed over the same visible page as the others, for the same
    // reason.
    let enums: Vec<String> = catalog
        .list_custom_properties(kind.map(AssetKind::as_str))
        .await?
        .into_iter()
        .filter(|(_, property)| {
            property.property_type == graph_owl_core::custom_property::PropertyType::Enum
        })
        .map(|(_, property)| property.name)
        .collect();
    let mut by_property: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, usize>,
    > = std::collections::BTreeMap::new();
    for asset in &page_result.data {
        let Some(bag) = &asset.extension else {
            continue;
        };
        for name in &enums {
            if let Some(value) = bag.get(name).and_then(serde_json::Value::as_str) {
                *by_property
                    .entry(name.clone())
                    .or_default()
                    .entry(value.to_string())
                    .or_default() += 1;
            }
        }
    }

    let mut facets = json!({
        "kind": by_kind.iter().map(|(k, n)| json!({ "value": k, "count": n })).collect::<Vec<_>>(),
        "schema": by_schema.iter().map(|(k, n)| json!({ "value": k, "count": n })).collect::<Vec<_>>(),
    });
    if let Some(object) = facets.as_object_mut() {
        for (name, counts) in by_property {
            object.insert(
                format!("extension.{name}"),
                json!(
                    counts
                        .iter()
                        .map(|(value, count)| json!({ "value": value, "count": count }))
                        .collect::<Vec<_>>()
                ),
            );
        }
    }

    Ok(Json(json!({
        "data": page_result.data,
        "paging": page_result.paging,
        "facets": facets,
    })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AsOfQuery {
    /// RFC 3339. Absent means now.
    as_of: Option<String>,
}

async fn get_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<AsOfQuery>,
) -> Result<Json<Asset>, AppError> {
    let Some(raw) = query.as_of else {
        return Ok(Json(catalog.get_asset_for(&principal, id).await?));
    };

    let at = chrono::DateTime::parse_from_rfc3339(&raw)
        .map_err(|e| {
            AppError::Validation(vec![FieldError::new(
                "asOf",
                FieldErrorCode::Type,
                format!("`{raw}` is not an RFC 3339 timestamp: {e}"),
            )])
        })?
        .with_timezone(&chrono::Utc);

    // Authorization is resolved against the *current* relational state, never
    // against the projection (`04-engine-triples.md` decision 7). Flakes lag
    // by design, so a permission revoked in that window would still be honoured
    // if the check read from them. Establishing visibility first and only then
    // reading history is what keeps time-travel from becoming a way to read
    // what you are no longer allowed to see.
    catalog.get_asset_for(&principal, id).await?;

    Ok(Json(catalog.get_asset_as_of(id, at).await?))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubgraphQuery {
    hops: Option<usize>,
    direction: Option<String>,
    max_nodes: Option<usize>,
    as_of: Option<String>,
}

/// The neighbourhood around an asset.
///
/// Returns nodes with their kind and name resolved, so a renderer can draw
/// labels without N follow-up reads — the whole point of one statement per
/// traversal is lost if the client then makes one request per node.
async fn asset_graph(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<SubgraphQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let direction = match query.direction.as_deref() {
        None | Some("both") => graph_owl_traversal::Direction::Both,
        Some("outgoing") => graph_owl_traversal::Direction::Outgoing,
        Some("incoming") => graph_owl_traversal::Direction::Incoming,
        Some(other) => {
            return Err(AppError::Validation(vec![FieldError::new(
                "direction",
                FieldErrorCode::Type,
                format!("`{other}` is not one of: outgoing, incoming, both"),
            )]));
        }
    };

    let defaults = graph_owl_traversal::Bounds::default();
    let bounds = graph_owl_traversal::Bounds {
        // Capped server-side. A client asking for 50 hops on a real estate is
        // asking for the whole graph, and the bound exists to protect the
        // server rather than to be polite to the client.
        max_hops: query.hops.unwrap_or(defaults.max_hops).min(6),
        max_nodes: query.max_nodes.unwrap_or(defaults.max_nodes).min(1_000),
    };

    let as_of = match query.as_of {
        None => None,
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map_err(|e| {
                    AppError::Validation(vec![FieldError::new(
                        "asOf",
                        FieldErrorCode::Type,
                        format!("`{raw}` is not an RFC 3339 timestamp: {e}"),
                    )])
                })?
                .with_timezone(&chrono::Utc),
        ),
    };

    let graph = catalog
        .asset_subgraph(&principal, id, direction, bounds, as_of)
        .await?;

    // Resolve labels for the nodes we are about to return. Unknown ids stay in
    // the result as bare nodes rather than being dropped: a node the reader
    // cannot see is still structurally present, and silently removing it would
    // leave the picture claiming a smaller neighbourhood than exists.
    let mut nodes = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        let resolved = match node.id.parse::<Uuid>() {
            Ok(uuid) => catalog.get_asset_for(&principal, uuid).await.ok(),
            Err(_) => None,
        };
        nodes.push(match resolved {
            Some(asset) => json!({
                "id": node.id,
                "name": asset.name,
                "kind": asset.kind.as_str(),
                "fullyQualifiedName": asset.fully_qualified_name,
            }),
            None => json!({ "id": node.id, "name": node.id, "kind": null }),
        });
    }

    Ok(Json(json!({
        "nodes": nodes,
        "edges": graph.edges.iter().map(|e| json!({
            "from": e.from.id,
            "to": e.to.id,
            "relationship": e.relationship,
            // **The reasoner concluded this; nobody asserted it.** Decision 2
            // keeps conclusions in their own graph so nobody mistakes one for a
            // stated fact, and a picture that draws both alike undoes that
            // separation in front of the person about to act on it.
            "derived": e.derived,
        })).collect::<Vec<_>>(),
        "truncated": graph.truncated,
    })))
}

async fn list_roots(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<Asset>>, AppError> {
    Ok(Json(catalog.list_children_for(&principal, None).await?))
}

async fn list_asset_children(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Asset>>, AppError> {
    // A missing parent is a 404, not an empty list: "this has no children" and
    // "this does not exist" are different answers. A parent hidden by policy
    // takes the same path, because 403 on a specific id confirms it exists.
    catalog.get_asset_for(&principal, id).await?;
    Ok(Json(catalog.list_children_for(&principal, Some(id)).await?))
}

async fn asset_ancestors(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Asset>>, AppError> {
    catalog.get_asset_for(&principal, id).await?;
    Ok(Json(catalog.ancestors_of(id).await?))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SparqlRequest {
    query: String,
    /// RFC 3339. Absent means now.
    as_of: Option<String>,
}

impl ValidateBody for SparqlRequest {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("query"),
            &mut errors,
        );
        errors
    }
}

/// SPARQL over the graph.
///
/// `POST` rather than `GET`, deliberately: a query is a body, not a URL. The
/// GET form the SPARQL protocol also allows would put a whole query — often
/// with literal values from the estate — into request logs and browser history.
async fn sparql(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<SparqlRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let as_of = match payload.as_of {
        None => None,
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map_err(|e| {
                    AppError::Validation(vec![FieldError::new(
                        "asOf",
                        FieldErrorCode::Type,
                        format!("`{raw}` is not an RFC 3339 timestamp: {e}"),
                    )])
                })?
                .with_timezone(&chrono::Utc),
        ),
    };

    // The budget is the server's, not the caller's. A client that could raise
    // its own limit does not have one.
    let outcome = catalog
        .sparql(&principal, &payload.query, as_of, SparqlBudget::default())
        .await?;

    Ok(Json(json!({
        "rows": outcome.rows,
        "factsScanned": outcome.facts_scanned,
        // Always present, never inferred from row count. A truncated answer
        // that looks complete is the failure this project refuses everywhere.
        "truncated": outcome.truncated,
        // The freshness stamp (`04-engine-triples.md` decision 8): an
        // eventually-consistent answer presented as current is this design's
        // failure mode, and the stamp is what makes it honest instead.
        "asOf": outcome.as_of,
        // **What the engine decided to read.** An author who cannot see
        // whether pushdown bounded their query cannot tell one that is
        // inherently expensive from one a single triple pattern away from
        // being cheap.
        "plan": outcome.plan,
        // **The order the query named them.** Solutions are sorted maps, so
        // this is the only place the author's own column order survives.
        "variables": outcome.variables,
    })))
}

/// Run a validation pass and replace the stored queue — Epic 5 Slice C.
///
/// Admin-only and `POST`, for the reasons the reasoning run is: a full pass
/// over the estate is the cheapest way an unprivileged caller could load the
/// database, and it replaces stored state.
async fn run_validation(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<graph_owl_api::ValidationRun>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.run_validation().await?))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorConfigRequest {
    connector: String,
    service_name: String,
    /// Everything a reader may see. Rendered by `SchemaForm` from the
    /// connector's own JSON Schema, which is why it is free-form here.
    #[serde(default)]
    settings: serde_json::Value,
    /// **Omit to keep the existing credential.** An edit form cannot resend what
    /// it was never given, and `Option` is what lets absent mean "leave it"
    /// rather than "clear it" — the difference between changing a port and
    /// breaking a connector.
    #[serde(default)]
    secret: Option<String>,
}

impl ValidateBody for ConnectorConfigRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// What a connector needs configured, as JSON Schema — Epic 41 Slice F.
///
/// **The connector declares its own shape**, so the console renders a form
/// without knowing what a Postgres connection needs. A hundred connectors with
/// hand-written screens is a hundred places for a field to go missing, and the
/// one that goes missing is always the optional-looking one somebody needed.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionTestRequest {
    /// The same settings the form would save. Sent rather than read from a stored
    /// config **because the point is to test before saving** — a test that could
    /// only run against what is already persisted would confirm the credential
    /// after the mistake was made.
    settings: serde_json::Value,
    /// Write-only, and never echoed back. Present here because a connection
    /// cannot be tested without it.
    #[serde(default)]
    secret: Option<String>,
}

impl ValidateBody for ConnectionTestRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Try the connection a form is about to save — Epic 41 Slice F.
///
/// **The failure message is passed through.** "Could not connect" tells an admin
/// nothing; `password authentication failed for user "catalog"` tells them which
/// of the five fields is wrong. The message comes from the driver and names the
/// host and user, never the secret.
async fn test_connector(
    Auth(principal): Auth,
    Path(connector): Path<String>,
    AppJson(payload): AppJson<ConnectionTestRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    if connector != "postgres" {
        return Err(AppError::NotFound);
    }

    let get = |key: &str| -> String {
        payload
            .settings
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let port = payload
        .settings
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(5432);
    let secret = payload.secret.clone().unwrap_or_default();
    let connection_string = format!(
        "postgres://{}:{}@{}:{}/{}",
        get("username"),
        secret,
        get("host"),
        port,
        get("database")
    );

    // `Ok(false)` rather than an error status for a refused connection: the
    // request succeeded and the answer is "no". A `502` here would make a wrong
    // password indistinguishable from the catalog being down.
    match PostgresConnector::connect(&connection_string, get("host")).await {
        Ok(connector) => match connector.test_connection().await {
            Ok(()) => Ok(Json(json!({ "ok": true }))),
            Err(e) => Ok(Json(
                json!({ "ok": false, "detail": redact(&e.to_string(), &secret) }),
            )),
        },
        Err(e) => Ok(Json(
            json!({ "ok": false, "detail": redact(&e.to_string(), &secret) }),
        )),
    }
}

/// Strip the credential out of a driver message before it leaves the process.
///
/// sqlx puts the whole connection string in some errors, and the whole point of a
/// write-only secret is that it never appears in a response. Redacting here rather
/// than trusting the driver's own masking: a message shape that changes in a patch
/// release would otherwise leak silently.
fn redact(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        return message.to_string();
    }
    message.replace(secret, "***")
}

/// **The batch ceiling.** `16-ingestion-apis.md` Slice A: "≤1000 items, larger →
/// `400`". A request is not a job — decision 2 puts anything bigger behind the
/// batch-file path, and accepting an unbounded body here would make a 500k-row
/// push a request that times out after doing half the work.
const MAX_INGEST_ITEMS: usize = 1000;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IngestRequest {
    #[serde(default)]
    items: Vec<IngestItemRequest>,
    /// Edges between entities, by FQN. Applied after every entity, because an
    /// endpoint may be an item submitted later in the same batch.
    #[serde(default)]
    edges: Vec<IngestEdgeRequest>,
}

#[derive(Debug, serde::Deserialize, std::hash::Hash)]
#[serde(rename_all = "camelCase")]
struct IngestEdgeRequest {
    from_fqn: String,
    to_fqn: String,
    /// A lineage relationship. Every pushed edge is lineage: `Relationship`
    /// operates on the `tables` relation, and a push creates assets.
    relationship: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, serde::Deserialize, std::hash::Hash)]
#[serde(rename_all = "camelCase")]
struct IngestItemRequest {
    kind: String,
    name: String,
    /// The containing entity's FQN. Absent means a root.
    #[serde(default)]
    parent_fqn: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    properties: Option<serde_json::Value>,
}

impl ValidateBody for IngestRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Push a batch — Epic 16 Slice A.
///
/// **`207 Multi-Status`, always, when anything was attempted.** A `200` would say
/// the batch succeeded when item 42 did not, and a `400` would say it failed when
/// 999 items landed. Neither is true, and a pusher branching on the status needs
/// the one that means "read the per-item results".
async fn ingest(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    headers: axum::http::HeaderMap,
    AppJson(payload): AppJson<IngestRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // Epic 16 Slice B. Decision 4: mandatory for push, not optional — at-least-once
    // transport duplicates without it, and a pusher that times out has no way to
    // know whether the first attempt landed.
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);

    if let Some(key) = &idempotency_key {
        // Hashed over the *body*, so the same key with different content is
        // reported rather than silently served the first answer.
        let fingerprint = fingerprint(&payload);
        match catalog.claim_idempotency(key, &fingerprint).await? {
            graph_owl_storage::IdempotencyClaim::Claimed => {}
            graph_owl_storage::IdempotencyClaim::Replay { status, body } => {
                return Ok((
                    StatusCode::from_u16(status).unwrap_or(StatusCode::MULTI_STATUS),
                    Json(body),
                ));
            }
            graph_owl_storage::IdempotencyClaim::Mismatch => {
                return Err(AppError::Conflict {
                    detail: format!(
                        "idempotency key `{key}` was already used for a different request. \
                         A key identifies one request, not a slot — reusing it for new \
                         content would silently drop this push"
                    ),
                    existing_id: None,
                    kind: ConflictKind::IdempotencyConflict,
                });
            }
            graph_owl_storage::IdempotencyClaim::InFlight => {
                return Err(AppError::Conflict {
                    detail: format!(
                        "idempotency key `{key}` is being processed by another request. \
                         Retry once it completes; processing it twice is exactly what \
                         the key exists to prevent"
                    ),
                    existing_id: None,
                    kind: ConflictKind::IdempotencyConflict,
                });
            }
        }
    }

    // The ceiling counts everything the request asks for: a batch of 900 entities
    // and 900 edges is 1800 units of work, and counting only one list would let a
    // caller double the cost by splitting it across two fields.
    let total = payload.items.len() + payload.edges.len();
    if total > MAX_INGEST_ITEMS {
        return Err(AppError::Validation(vec![FieldError::new(
            "items",
            FieldErrorCode::Type,
            format!(
                "a push carries at most {MAX_INGEST_ITEMS} items and edges combined; \
                 this one has {total}. Larger loads go through the batch-file path, \
                 because a request that big cannot be answered synchronously"
            ),
        )]));
    }

    let mut items = Vec::with_capacity(payload.items.len());
    for (index, item) in payload.items.iter().enumerate() {
        // The kind is parsed up front rather than per item during application:
        // an unknown kind is a malformed *request*, not a per-item failure, and
        // reporting it as one would let a typo look like a rejected entity.
        let kind = parse_kind(Some(&item.kind))?.ok_or_else(|| {
            AppError::Validation(vec![FieldError::new(
                format!("items[{index}].kind"),
                FieldErrorCode::Required,
                "an item needs a kind",
            )])
        })?;
        items.push(graph_owl_api::IngestItem {
            kind,
            name: item.name.clone(),
            parent_fqn: item.parent_fqn.clone(),
            description: item.description.clone(),
            properties: item.properties.clone(),
        });
    }

    let edges = payload
        .edges
        .iter()
        .map(|edge| graph_owl_api::IngestEdge {
            from_fqn: edge.from_fqn.clone(),
            to_fqn: edge.to_fqn.clone(),
            relationship: edge.relationship.clone(),
            query: edge.query.clone(),
            description: edge.description.clone(),
        })
        .collect();

    let outcomes = catalog.ingest(&principal, items, edges).await?;
    let accepted = outcomes.iter().filter(|o| o.status < 400).count();
    let body = json!({
        "accepted": accepted,
        "rejected": outcomes.len() - accepted,
        "results": outcomes
            .iter()
            .map(|outcome| json!({
                "index": outcome.index,
                "status": outcome.status,
                "id": outcome.id,
                "problem": outcome.problem,
            }))
            .collect::<Vec<_>>(),
    });

    // Recorded **after** the work, so a replay returns what actually happened
    // rather than what was intended. A key recorded up front would replay a
    // success for a push that then failed.
    if let Some(key) = &idempotency_key {
        catalog
            .record_idempotent_response(key, StatusCode::MULTI_STATUS.as_u16(), &body)
            .await?;
    }

    Ok((StatusCode::MULTI_STATUS, Json(body)))
}

/// Where an upload is spooled while it is being read.
///
/// **Spooled to disk, not held in memory.** The request body has to be fully
/// received before the connection can be answered, and a 500k-row file held in a
/// `Vec<u8>` to satisfy that would break the memory bound this slice exists for
/// before the parser ever saw a row.
fn spool_path(id: uuid::Uuid) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("graph-owl-ingest-{id}"))
}

/// Accept a batch file — Epic 16 Slice C.
///
/// **Raw body with a `Content-Type`, not `multipart/form-data`.** The plan says
/// multipart, and this deliberately differs: multipart is a browser form
/// encoding, every pusher here is a program, and it would add a parsing
/// dependency and a second place for the byte stream to be buffered. A client
/// sends `curl --data-binary @file -H 'Content-Type: application/x-ndjson'`.
/// Recorded in the plan under "deviations".
async fn ingest_batch(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    use graph_owl_connectors::rows::Format;

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    // Refused by name rather than guessed at. A Parquet file fed to a line parser
    // reports every row as malformed, which buries "this build does not read
    // Parquet" under half a million parse errors.
    let format = Format::parse(content_type).ok_or_else(|| {
        AppError::Validation(vec![FieldError::new(
            "content-type",
            FieldErrorCode::Type,
            format!(
                "`{content_type}` is not a batch format this build reads. \
                 Send `application/x-ndjson` (one entity per line) or `text/csv`. \
                 Parquet is columnar, so a reader must hold a row group at a time, \
                 which is exactly the property batch ingestion avoids — convert it \
                 to JSONL before pushing"
            ),
        )])
    })?;

    let id = uuid::Uuid::new_v4();
    catalog
        .create_ingest_job(id, &format!("{format:?}").to_lowercase(), &principal.id)
        .await?;

    let path = spool_path(id);
    spool(body, &path)
        .await
        .map_err(|error| AppError::Internal(format!("the upload could not be spooled: {error}")))?;

    // Detached, because the response has to go back now. Every outcome from here
    // lands on the job row — including a panic, which the reaper turns into
    // `failed` rather than a row that reads `running` forever.
    let worker = catalog.clone();
    tokio::spawn(async move {
        let outcome = match std::fs::File::open(&path) {
            Ok(file) => {
                worker
                    .run_batch_ingest(
                        id,
                        std::io::BufReader::new(file),
                        format,
                        principal,
                        graph_owl_api::BATCH_ERROR_CAP,
                    )
                    .await
            }
            Err(error) => {
                worker
                    .fail_ingest_job(
                        id,
                        &format!("the spooled upload could not be read: {error}"),
                    )
                    .await
            }
        };
        if let Err(error) = outcome {
            tracing::error!(job = %id, "batch job could not be recorded: {error:?}");
        }
        // Removed whether it succeeded or not: a spool file that outlives its job
        // is a copy of somebody's metadata sitting in a shared temp directory.
        if let Err(error) = std::fs::remove_file(&path) {
            tracing::warn!(job = %id, "the spooled upload could not be removed: {error}");
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "id": id,
            "state": "queued",
            "poll": format!("/ingest/jobs/{id}"),
        })),
    ))
}

/// Stream a request body to disk without ever holding it whole.
async fn spool(body: axum::body::Body, path: &std::path::Path) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;

    let mut file = spool_file(path).await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(std::io::Error::other)?;
        file.write_all(&chunk).await?;
    }
    file.flush().await
}

/// Create the spool file, readable only by this process's user where the
/// platform can express that.
///
/// The temp directory is shared, and the file is a verbatim copy of somebody's
/// metadata — a default-`0644` spool would publish it to every account on the
/// host for the life of the job.
async fn spool_file(path: &std::path::Path) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).await
}

/// Poll a batch job — Epic 16 Slice C.
async fn ingest_job(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let job = catalog.ingest_job(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(serde_json::to_value(&job).unwrap_or_default()))
}

/// Ask a batch job to stop — Epic 16 Slice C.
///
/// `200` with the job either way, rather than `204`: a client cancelling
/// something needs to see what had landed by the time it stopped, and a body-less
/// response makes them poll again to find out.
async fn cancel_ingest_job(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    // `false` means it had already finished, which is **not** an error: a client
    // that cancels a job the instant it completes has not done anything wrong,
    // and a `409` there would be noise in every well-behaved cancel path.
    catalog.cancel_ingest_job(id).await?;
    let job = catalog.ingest_job(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(serde_json::to_value(&job).unwrap_or_default()))
}

/// A stable fingerprint of a push body.
///
/// Serialized through `serde_json` rather than hashing the raw bytes: two
/// semantically identical requests that differ only in key order or whitespace are
/// the same request, and reporting them as a mismatch would make a client's
/// formatting choice a `409`.
fn fingerprint(request: &IngestRequest) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in &request.items {
        item.kind.hash(&mut hasher);
        item.name.hash(&mut hasher);
        item.parent_fqn.hash(&mut hasher);
        item.description.hash(&mut hasher);
        item.properties
            .as_ref()
            .map(ToString::to_string)
            .hash(&mut hasher);
    }
    // Edges are part of the request's identity: two pushes with the same entities
    // and different edges are different requests, and hashing only the entities
    // would replay the first answer for the second.
    for edge in &request.edges {
        edge.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

async fn connector_schema(
    Auth(principal): Auth,
    Path(connector): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    match connector.as_str() {
        "postgres" => Ok(Json(PostgresConnector::describe_config())),
        // A connector nobody has registered is a `404`, not an empty schema: an
        // empty schema renders as a form with no fields, which reads as "this
        // connector needs nothing" rather than "this connector does not exist".
        _ => Err(AppError::NotFound),
    }
}

/// Save a connector configuration — Epic 41 Slice F.
///
/// Admin-only: a connector configuration holds a credential and decides what
/// gets catalogued, which is administration rather than cataloguing.
async fn save_connector_config(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<ConnectorConfigRequest>,
) -> Result<(StatusCode, Json<graph_owl_storage::ConnectorConfig>), AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let saved = catalog
        .save_connector_config(
            &payload.connector,
            &payload.service_name,
            payload.settings,
            payload.secret.as_deref(),
        )
        .await?;
    // `ConnectorConfig` has no field for a credential, so this response cannot
    // carry one — the guarantee is the type, not this handler remembering.
    Ok((StatusCode::CREATED, Json(saved)))
}

/// Every configuration, without credentials.
async fn list_connector_configs(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<graph_owl_storage::ConnectorConfig>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.connector_configs().await?))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEndpointRequest {
    path: String,
    source: String,
    signature_scheme: graph_owl_storage::SignatureScheme,
    mapping: String,
    #[serde(default)]
    event_filter: Vec<String>,
    /// Absent means "usable immediately" — a freshly registered endpoint has
    /// no prior state for a client to be preserving.
    #[serde(default = "default_webhook_enabled")]
    enabled: bool,
    /// Required on first registration; write-only and never echoed back.
    /// `Option` for the same reason as [`ConnectorConfigRequest::secret`] —
    /// there is no update path for this endpoint yet (Slice A only
    /// registers), so today this is always `Some`, but the field stays
    /// optional so a future edit form can omit it to keep the existing key.
    #[serde(default)]
    secret: Option<String>,
    /// Deliveries this endpoint accepts per minute; absent means unlimited
    /// — Epic 18 Slice E.
    #[serde(default)]
    rate_limit_per_minute: Option<u32>,
}

fn default_webhook_enabled() -> bool {
    true
}

impl ValidateBody for WebhookEndpointRequest {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("path"),
            &mut errors,
        );
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("source"),
            &mut errors,
        );
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("mapping"),
            &mut errors,
        );
        errors
    }
}

/// Register a webhook endpoint — Epic 18 Slice A.
///
/// Admin-only: a webhook endpoint holds a secret and decides what an external
/// system may write into the catalog, which is administration rather than
/// cataloguing — same reasoning as [`save_connector_config`].
async fn register_webhook_endpoint(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<WebhookEndpointRequest>,
) -> Result<(StatusCode, Json<graph_owl_storage::WebhookEndpoint>), AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let now = chrono::Utc::now();
    let endpoint = graph_owl_storage::WebhookEndpoint {
        id: Uuid::new_v4(),
        path: payload.path,
        source: payload.source,
        signature_scheme: payload.signature_scheme,
        mapping: payload.mapping,
        event_filter: payload.event_filter,
        enabled: payload.enabled,
        // Ignored on the way in — `upsert_webhook_endpoint` returns the real
        // value from `secret IS NOT NULL`, not this placeholder.
        has_secret: false,
        rate_limit_per_minute: payload.rate_limit_per_minute,
        created_at: now,
        updated_at: now,
    };
    let saved = catalog
        .register_webhook_endpoint(endpoint, payload.secret.as_deref().map(str::as_bytes))
        .await?;
    // `WebhookEndpoint` has no field for the secret, so this response cannot
    // carry one — the guarantee is the type, not this handler remembering.
    Ok((StatusCode::CREATED, Json(saved)))
}

/// Every registered endpoint, without secrets.
async fn list_webhook_endpoints(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<graph_owl_storage::WebhookEndpoint>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.list_webhook_endpoints().await?))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamSubscriptionRequest {
    broker: graph_owl_storage::BrokerConfig,
    topic: String,
    consumer_group: String,
    mapping: String,
    #[serde(default = "default_start_position")]
    start_position: graph_owl_storage::StartPosition,
    #[serde(default = "default_max_in_flight")]
    max_in_flight: usize,
    #[serde(default = "default_poison_threshold")]
    poison_threshold: u32,
    #[serde(default = "default_stream_subscription_enabled")]
    enabled: bool,
    /// Write-only and never echoed back — same reasoning as
    /// [`WebhookEndpointRequest::secret`]. `Option` because not every broker
    /// needs one: the Kafka/Pulsar testcontainers this epic's own tests run
    /// against are unauthenticated.
    #[serde(default)]
    secret: Option<String>,
}

fn default_start_position() -> graph_owl_storage::StartPosition {
    graph_owl_storage::StartPosition::Latest
}

/// Not a stated fact the way `admission::DEFAULT_PERMITS` derives from the
/// Postgres pool size — there is no equivalent number to derive this from.
/// `100` is a starting point an operator overrides once real throughput is
/// known, the same reasoning `rate_limit_per_minute` avoided inventing a
/// global default entirely; this one needs *some* value or the type would
/// have to be `Option<usize>` for a field the plan's own reference always
/// shows as required.
fn default_max_in_flight() -> usize {
    100
}

fn default_poison_threshold() -> u32 {
    3
}

fn default_stream_subscription_enabled() -> bool {
    true
}

impl ValidateBody for StreamSubscriptionRequest {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("topic"),
            &mut errors,
        );
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("consumerGroup"),
            &mut errors,
        );
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("mapping"),
            &mut errors,
        );
        errors
    }
}

/// Register a streaming subscription — Epic 19 Slice A.
///
/// Admin-only, same reasoning as [`register_webhook_endpoint`]: a
/// subscription holds broker credentials and decides what an external
/// system may write into the catalog on an ongoing basis.
async fn register_stream_subscription(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<StreamSubscriptionRequest>,
) -> Result<(StatusCode, Json<graph_owl_storage::StreamSubscription>), AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let now = chrono::Utc::now();
    let subscription = graph_owl_storage::StreamSubscription {
        id: Uuid::new_v4(),
        broker: payload.broker,
        topic: payload.topic,
        consumer_group: payload.consumer_group,
        mapping: payload.mapping,
        start_position: payload.start_position,
        max_in_flight: payload.max_in_flight,
        poison_threshold: payload.poison_threshold,
        // Ignored on the way in — `upsert_stream_subscription` returns the
        // real value from `secret IS NOT NULL`, not this placeholder.
        has_secret: false,
        enabled: payload.enabled,
        created_at: now,
        updated_at: now,
    };
    let saved = catalog
        .register_stream_subscription(subscription, payload.secret.as_deref().map(str::as_bytes))
        .await?;
    // Starts immediately rather than waiting for the next server restart —
    // `spawn_enabled_subscriptions` (called once at startup) is what makes a
    // *restart* resume existing subscriptions; this is what makes a *fresh*
    // registration usable without one.
    if saved.enabled {
        streaming::spawn_consumer(catalog, saved.clone());
    }
    Ok((StatusCode::CREATED, Json(saved)))
}

/// Every registered subscription, without secrets.
async fn list_stream_subscriptions(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<graph_owl_storage::StreamSubscription>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.list_stream_subscriptions().await?))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamReplayRequest {
    subscription: Uuid,
    since: chrono::DateTime<chrono::Utc>,
}

impl ValidateBody for StreamReplayRequest {
    /// Nothing to check structurally: both fields are required and typed,
    /// so serde's own rejection is the validation — same reasoning as
    /// `ReplayRequest` (Epic 18 Slice D).
    fn validate_body(_value: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Replay a subscription's messages from a timestamp — Epic 19 Slice E.
///
/// Synchronous, unlike the webhook replay's fire-and-forget: a replay reads
/// a bounded historical window and the summary *is* the answer an operator
/// asked for, so returning `202` with nothing to poll would be worse than
/// waiting. It is admission-controlled as ingestion for the same reason
/// `/ingest/batch` is.
async fn replay_stream_window(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<StreamReplayRequest>,
) -> Result<Json<streaming::StreamReplaySummary>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(
        streaming::replay_window(&catalog, payload.subscription, payload.since).await?,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamDeadLetterQuery {
    subscription: Option<Uuid>,
}

/// Poisoned streamed messages, newest first — Epic 19 Slice D. Admin-gated
/// like every other streaming surface: the raw payload is external data an
/// operator triages, not something every catalog reader needs to see.
async fn list_stream_dead_letters(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppQuery(query): AppQuery<StreamDeadLetterQuery>,
) -> Result<Json<Vec<graph_owl_storage::StreamDeadLetter>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.stream_dead_letters(query.subscription).await?))
}

/// Replays one dead letter after a mapping fix — Epic 19 Slice D. `200`
/// with no body on success (the letter is gone); the failure a
/// still-broken mapping produces comes back as the same validation error a
/// live message would have hit.
async fn replay_stream_dead_letter(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    catalog.replay_stream_dead_letter(id).await?;
    Ok(StatusCode::OK)
}

/// `graph_owl_<subsystem>_<noun>_<unit>`, per the observability contract's
/// naming convention. `endpoint` is the registered *path*, not the endpoint's
/// UUID — an operator's dashboard groups by the name they configured, the
/// same reasoning `admission`'s `route` label uses the template rather than
/// a generated id.
const WEBHOOK_EVENTS: &str = "graph_owl_webhook_events_total";

/// Epic 18 Slice E's "metrics per endpoint for received, applied, duplicate,
/// dead-lettered" criterion, recorded here rather than in `graph-owl-api`:
/// every other Prometheus counter in this codebase lives in the HTTP layer
/// (`admission`, `observability`), and the facade's own instrumentation is
/// `tracing::instrument` spans, not metrics — this keeps that boundary
/// rather than drawing a new one for one subsystem.
fn record_webhook_event(endpoint_path: &str, state: graph_owl_core::webhook::EventState) {
    metrics::counter!(
        WEBHOOK_EVENTS,
        "endpoint" => endpoint_path.to_string(),
        "state" => state.as_str()
    )
    .increment(1);
}

/// Receive a webhook delivery — Epic 18 Slice A.
///
/// **Raw bytes, read before any JSON parsing.** Plan decision 2: an
/// unverified payload is never deserialized, since parsing untrusted bytes is
/// the attack surface. `body` is `axum::body::Bytes`, so the signature is
/// checked against exactly what the sender sent — never a re-serialization,
/// which could differ in whitespace or key order and silently break
/// verification.
///
/// **No [`Auth`] extractor.** The sender is not a graph-owl principal and
/// carries no bearer token; what stands in its place is the endpoint's own
/// configured signature scheme, checked inside [`Catalog::receive_webhook`].
/// A path that names no registered endpoint, and a disabled one, both read
/// as `404` — an unregistered path and a disabled one are indistinguishable
/// to an outside caller by design (Slice E's reasoning, pulled forward
/// because it falls out of `enabled` for free).
async fn receive_webhook(
    State(catalog): State<Catalog>,
    axum::Extension(rate_limiter): axum::Extension<Arc<rate_limit::RateLimiter>>,
    Path(path): Path<String>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let endpoint = catalog
        .webhook_endpoint_by_path(&path)
        .await?
        .ok_or(AppError::NotFound)?;

    // Checked before signature verification, deliberately: the criterion is
    // "one misbehaving sender must not cost every other sender its
    // traffic", and a flood of unsigned noise is exactly the case that
    // would otherwise burn CPU on HMAC/Ed25519 verification per request
    // before ever being refused.
    if let Err(rate_limit::RateLimited {
        retry_after_seconds,
    }) = rate_limiter.try_admit(endpoint.id, endpoint.rate_limit_per_minute)
    {
        return Err(AppError::RateLimited {
            retry_after_seconds,
        });
    }

    let header_name = match &endpoint.signature_scheme {
        graph_owl_storage::SignatureScheme::HmacSha256 { header, .. }
        | graph_owl_storage::SignatureScheme::Ed25519 { header } => header,
    };
    let signature = headers
        .get(header_name.as_str())
        .and_then(|value| value.to_str().ok());
    let event = catalog.receive_webhook(&endpoint, signature, &body).await?;
    record_webhook_event(&endpoint.path, event.state);
    // Detached, same reasoning as `ingest_batch`: the response has already
    // gone back with the event's id, and mapping/applying (Slice D) can
    // take longer than a caller should have to wait for a `201`. Only for
    // an actually-new delivery — a `Duplicate` has already had its one
    // effect (none, and reprocessing it would just repeat the state check
    // in `process_inbound_event` for nothing), and a `Failed` delivery
    // (malformed JSON, checked synchronously) has nothing to map.
    if event.state == graph_owl_core::webhook::EventState::Received {
        let worker = catalog.clone();
        let event_id = event.id;
        let endpoint_path = endpoint.path.clone();
        tokio::spawn(async move {
            if let Err(error) = worker.process_inbound_event(event_id).await {
                tracing::error!(event = %event_id, "inbound event could not be processed: {error:?}");
                return;
            }
            // Read back rather than threaded through the return value: the
            // metric is a side effect of whatever state the pipeline landed
            // on (`Mapped`/`Applied`/`Failed`/`Superseded`), not a value
            // `process_inbound_event` needs to hand back for any other
            // reason, and this runs after the response has already gone
            // back so the extra read costs no caller anything.
            if let Ok(Some(processed)) = worker.inbound_event(event_id).await {
                record_webhook_event(&endpoint_path, processed.state);
            }
        });
    }
    // `Failed` here can only be the synchronous malformed-JSON check —
    // every other rejection (mapping, shape, containment) happens later,
    // asynchronously, after this response has already gone back as `201`.
    let status = if event.state == graph_owl_core::webhook::EventState::Failed {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(json!({ "id": event.id, "state": event.state, "reason": event.reason })),
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MappingRequest {
    name: String,
    kind: graph_owl_storage::Expression,
    entity_name: graph_owl_storage::Expression,
    #[serde(default)]
    parent_fqn: Option<graph_owl_storage::Expression>,
    #[serde(default)]
    description: Option<graph_owl_storage::Expression>,
    #[serde(default)]
    properties: std::collections::BTreeMap<String, graph_owl_storage::Expression>,
}

impl ValidateBody for MappingRequest {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

/// Records a new version of a mapping — Epic 18 Slice C.
///
/// Admin-only: a mapping decides how an external payload becomes a catalog
/// entity, which is administration rather than cataloguing.
async fn register_mapping(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<MappingRequest>,
) -> Result<(StatusCode, Json<graph_owl_storage::Mapping>), AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let now = chrono::Utc::now();
    let mapping = graph_owl_storage::Mapping {
        name: payload.name,
        version: 0, // ignored on write; storage computes the real next version
        kind: payload.kind,
        entity_name: payload.entity_name,
        parent_fqn: payload.parent_fqn,
        description: payload.description,
        properties: payload.properties,
        created_at: now,
    };
    let saved = catalog.upsert_mapping(mapping).await?;
    Ok((StatusCode::CREATED, Json(saved)))
}

/// The latest version of a mapping.
async fn get_mapping(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(name): Path<String>,
) -> Result<Json<graph_owl_storage::Mapping>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    catalog
        .mapping(&name)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// Every version of a mapping, newest first — the audit trail versioning
/// exists for.
async fn list_mapping_versions(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(name): Path<String>,
) -> Result<Json<Vec<graph_owl_storage::Mapping>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.mapping_versions(&name).await?))
}

/// Applies a mapping to a sample payload without writing anything — Epic 18
/// Slice C's dry-run criterion.
///
/// **The body is the sample payload itself**, not a request DTO wrapping
/// it — a dry run tests exactly what a real delivery would send, so
/// wrapping it would test a shape a sender never produces.
async fn dry_run_mapping(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(name): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let outcome = catalog.dry_run_mapping(&name, &payload).await?;
    Ok(Json(match outcome {
        graph_owl_api::MappingOutcome::Draft(draft) => json!({
            "outcome": "draft",
            "kind": draft.kind,
            "name": draft.name,
            "parentFqn": draft.parent_fqn,
            "description": draft.description,
            "properties": draft.properties,
        }),
        graph_owl_api::MappingOutcome::MissingField { field } => json!({
            "outcome": "missingField",
            "field": field,
        }),
        graph_owl_api::MappingOutcome::InvalidKind { kind } => json!({
            "outcome": "invalidKind",
            "kind": kind,
        }),
        graph_owl_api::MappingOutcome::ShapeViolation { reason } => json!({
            "outcome": "shapeViolation",
            "reason": reason,
        }),
    }))
}

/// The status of one inbound event — Epic 18 Slice D.
async fn inbound_event_status(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let event = catalog.inbound_event(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({
        "id": event.id,
        "endpoint": event.endpoint,
        "state": event.state,
        "reason": event.reason,
        "receivedAt": event.received_at,
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeadLetterQuery {
    endpoint: Option<Uuid>,
    reason_contains: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

/// The dead-letter queue, filtered — Epic 18 Slice D.
async fn dead_letter_queue(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Query(query): Query<DeadLetterQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    // 50, matching the page size the rest of the API uses.
    let limit = query.limit.unwrap_or(50).min(200);
    let filter = graph_owl_storage::DeadLetterFilter {
        endpoint: query.endpoint,
        reason_contains: query.reason_contains,
        limit,
        offset: query.offset.unwrap_or(0),
    };
    let events = catalog.dead_letter_queue(&filter).await?;
    Ok(Json(json!({
        "data": events.iter().map(|event| json!({
            "id": event.id,
            "endpoint": event.endpoint,
            "reason": event.reason,
            "receivedAt": event.received_at,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayRequest {
    endpoint: Uuid,
    since: chrono::DateTime<chrono::Utc>,
    until: chrono::DateTime<chrono::Utc>,
}

impl ValidateBody for ReplayRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Replays a window of an endpoint's events — Epic 18 Slice D.
async fn replay_window(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<ReplayRequest>,
) -> Result<Json<graph_owl_api::ReplaySummary>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(
        catalog
            .replay_window(payload.endpoint, payload.since, payload.until)
            .await?,
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurgeQuery {
    older_than_days: u32,
}

/// Deletes dead-lettered events older than the given number of days — Epic
/// 18 Slice D's bounded-retention criterion. The bound is named by whoever
/// calls this (a runbook, a schedule), not decided by the server.
async fn purge_dead_letters(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Query(query): Query<PurgeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(query.older_than_days));
    let purged = catalog.purge_dead_letters(cutoff).await?;
    Ok(Json(json!({ "purged": purged })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamRequest {
    id: String,
    /// The team this one reports into — Epic 11 Slice B. A cycle at **any** depth
    /// is refused, not merely self-parenting.
    #[serde(default)]
    parent_team_id: Option<String>,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    /// The complete membership, not a delta — a partial update cannot express
    /// "remove everybody", and removal is the operation that has to work.
    #[serde(default)]
    members: Vec<String>,
}

impl ValidateBody for TeamRequest {
    /// Shape only. "A team needs a name", "a member has to be a known user" are
    /// facts about the *estate*, which only the facade can check, and a rule
    /// stated in two places is a rule that will disagree with itself.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

fn team_body(team: &graph_owl_storage::Team) -> serde_json::Value {
    json!({
        "id": team.id,
        "displayName": team.display_name,
        "description": team.description,
        "members": team.members,
        // Always present, `null` for a root. A console reading its absence cannot
        // tell "top of the hierarchy" from "a server that does not know about
        // nesting" — the same argument as `inherited` on an owner.
        "parentTeamId": team.parent_team_id,
    })
}

// ---- Epic 24 Slice A: glossary and terms ----

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GlossaryRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

impl ValidateBody for GlossaryRequest {
    /// Shape only. "A glossary needs a name" is a fact the facade checks
    /// against the estate (a blank name derives no FQN), not a rule stated
    /// twice.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

fn glossary_body(glossary: &graph_owl_storage::Glossary) -> serde_json::Value {
    json!({
        "id": glossary.id,
        "name": glossary.name,
        "description": glossary.description,
        "fullyQualifiedName": glossary.fully_qualified_name,
        "createdAt": glossary.created_at,
        "updatedAt": glossary.updated_at,
    })
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GlossaryTermRequest {
    name: String,
    #[serde(default)]
    definition: String,
    #[serde(default)]
    synonyms: Vec<String>,
    #[serde(default)]
    abbreviations: Vec<String>,
}

impl ValidateBody for GlossaryTermRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// A field-level change to a term. No `id`, no `glossaryId`, no `status` —
/// structural rather than validated, the same reason `TableUpdate` has no
/// `id`: there is nothing here for a client to send that could move the
/// term or skip its review workflow (Slice C).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GlossaryTermUpdateRequest {
    #[serde(default)]
    definition: Option<String>,
    #[serde(default)]
    synonyms: Option<Vec<String>>,
    #[serde(default)]
    abbreviations: Option<Vec<String>>,
}

impl ValidateBody for GlossaryTermUpdateRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

fn term_body(term: &graph_owl_storage::GlossaryTermRecord) -> serde_json::Value {
    json!({
        "id": term.id,
        "glossaryId": term.glossary_id,
        "name": term.name,
        "fullyQualifiedName": term.fully_qualified_name,
        "definition": term.definition,
        "status": term.status.as_str(),
        "synonyms": term.synonyms,
        "abbreviations": term.abbreviations,
        "version": format!("{}.{}", term.version.major, term.version.minor),
        "createdAt": term.created_at,
        "updatedAt": term.updated_at,
    })
}

async fn create_glossary(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<GlossaryRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let glossary = catalog
        .create_glossary(&payload.name, payload.description)
        .await?;
    Ok((StatusCode::CREATED, Json(glossary_body(&glossary))))
}

async fn list_glossaries(
    State(catalog): State<Catalog>,
) -> Result<Json<serde_json::Value>, AppError> {
    let glossaries = catalog.list_glossaries().await?;
    Ok(Json(json!(
        glossaries.iter().map(glossary_body).collect::<Vec<_>>()
    )))
}

async fn get_glossary(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    catalog
        .get_glossary(id)
        .await?
        .map(|glossary| Json(glossary_body(&glossary)))
        .ok_or(AppError::NotFound)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteGlossaryQuery {
    #[serde(default)]
    recursive: bool,
}

async fn delete_glossary(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<DeleteGlossaryQuery>,
) -> Result<StatusCode, AppError> {
    catalog.delete_glossary(id, query.recursive).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_glossary_term(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(glossary_id): Path<Uuid>,
    AppJson(payload): AppJson<GlossaryTermRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let term = catalog
        .create_term(
            glossary_id,
            &payload.name,
            payload.definition,
            payload.synonyms,
            payload.abbreviations,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(term_body(&term))))
}

async fn list_glossary_terms(
    State(catalog): State<Catalog>,
    Path(glossary_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let terms = catalog.list_terms(glossary_id).await?;
    Ok(Json(json!(terms.iter().map(term_body).collect::<Vec<_>>())))
}

async fn get_glossary_term(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    catalog
        .get_term(id)
        .await?
        .map(|term| Json(term_body(&term)))
        .ok_or(AppError::NotFound)
}

async fn update_glossary_term(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<GlossaryTermUpdateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let term = catalog
        .update_term(
            id,
            graph_owl_storage::GlossaryTermUpdate {
                definition: payload.definition,
                synonyms: payload.synonyms,
                abbreviations: payload.abbreviations,
            },
        )
        .await?;
    Ok(Json(term_body(&term)))
}

async fn delete_glossary_term(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.delete_term(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchGlossaryTermsQuery {
    q: String,
}

async fn search_glossary_terms(
    State(catalog): State<Catalog>,
    AppQuery(query): AppQuery<SearchGlossaryTermsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let terms = catalog.search_terms(&query.q).await?;
    Ok(Json(json!(terms.iter().map(term_body).collect::<Vec<_>>())))
}

// ---- Epic 24 Slice B: SKOS relations ----

/// `kind`/`target` rather than the tagged-enum wire shape
/// [`graph_owl_core::glossary::SkosRelation`] itself serializes to: `kind` is
/// validated against the vocabulary *here*, with a field name a client can
/// act on, rather than surfacing serde's untagged-variant error.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkosRelationRequest {
    kind: String,
    target: String,
}

impl ValidateBody for SkosRelationRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Parse a wire `kind`/`target` pair into the domain relation.
///
/// # Errors
///
/// [`AppError::Validation`] naming `kind` if it is not one of the five SKOS
/// relations this catalog knows.
fn parse_skos_relation(
    kind: &str,
    target: String,
) -> Result<graph_owl_core::glossary::SkosRelation, AppError> {
    use graph_owl_core::glossary::SkosRelation;
    match kind {
        "broader" => Ok(SkosRelation::Broader(target)),
        "narrower" => Ok(SkosRelation::Narrower(target)),
        "related" => Ok(SkosRelation::Related(target)),
        "exactMatch" => Ok(SkosRelation::ExactMatch(target)),
        "closeMatch" => Ok(SkosRelation::CloseMatch(target)),
        other => Err(AppError::Validation(vec![FieldError::new(
            "kind",
            FieldErrorCode::Type,
            format!("`{other}` is not a recognised SKOS relation kind"),
        )])),
    }
}

/// `SkosRelation` already derives `Serialize` with this exact
/// `kind`/`target` shape, so a response is just `Json(relation)` — this
/// exists only to render a `Vec` the same way a single relation renders.
fn relation_body(relation: &graph_owl_core::glossary::SkosRelation) -> serde_json::Value {
    json!(relation)
}

async fn add_term_relation(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<SkosRelationRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let relation = parse_skos_relation(&payload.kind, payload.target)?;
    catalog.add_term_relation(id, relation.clone()).await?;
    Ok((StatusCode::CREATED, Json(relation_body(&relation))))
}

async fn list_term_relations(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let relations = catalog.term_relations(id).await?;
    Ok(Json(json!(
        relations.iter().map(relation_body).collect::<Vec<_>>()
    )))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteRelationQuery {
    kind: String,
    target: String,
}

async fn delete_term_relation(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<DeleteRelationQuery>,
) -> Result<StatusCode, AppError> {
    let relation = parse_skos_relation(&query.kind, query.target)?;
    catalog.remove_term_relation(id, &relation).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Epic 24 Slice C: review workflow ----

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetReviewersRequest {
    #[serde(default)]
    reviewers: Vec<String>,
}

impl ValidateBody for SetReviewersRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

fn reviewers_body(reviewers: &[String]) -> serde_json::Value {
    json!({ "reviewers": reviewers })
}

async fn set_term_reviewers(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<SetReviewersRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    catalog.set_term_reviewers(id, payload.reviewers).await?;
    let reviewers = catalog.term_reviewers(id).await?;
    Ok(Json(reviewers_body(&reviewers)))
}

async fn list_term_reviewers(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let reviewers = catalog.term_reviewers(id).await?;
    Ok(Json(reviewers_body(&reviewers)))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TermTransitionRequest {
    to: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    successor_term_id: Option<Uuid>,
}

impl ValidateBody for TermTransitionRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Move a term to a new status — Slice C. `actor` is the authenticated
/// principal, never a body field: a request that could name its own author
/// could forge one, and "only an assigned reviewer may approve" means
/// nothing if a caller can approve as somebody else.
async fn create_term_transition(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<TermTransitionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let to = graph_owl_core::glossary::TermStatus::parse(&payload.to).map_err(|raw| {
        AppError::Validation(vec![FieldError::new(
            "to",
            FieldErrorCode::Type,
            format!("`{raw}` is not a recognised term status"),
        )])
    })?;
    let term = catalog
        .transition_term(
            id,
            to,
            &principal.id,
            payload.reason,
            payload.successor_term_id,
        )
        .await?;
    Ok(Json(term_body(&term)))
}

// ---- Epic 24 Slice D: terms attach to assets and columns ----

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachTermRequest {
    target_fqn: String,
}

impl ValidateBody for AttachTermRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

async fn attach_term(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<AttachTermRequest>,
) -> Result<StatusCode, AppError> {
    catalog
        .attach_term(id, &payload.target_fqn, &principal.id)
        .await?;
    Ok(StatusCode::CREATED)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetachTermQuery {
    target_fqn: String,
}

async fn detach_term(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<DetachTermQuery>,
) -> Result<StatusCode, AppError> {
    catalog.detach_term(id, &query.target_fqn).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn term_usage(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<String>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.term_usage(id, &page).await?))
}

// ---- Epic 24 Slice E: Metric as a first-class entity ----

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricRequest {
    name: String,
    definition: String,
    #[serde(default)]
    formula: Option<String>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    granularity: Option<String>,
    #[serde(default = "default_calculation_type")]
    calculation_type: String,
    #[serde(default)]
    source_assets: Vec<String>,
    #[serde(default)]
    defined_by: Option<Uuid>,
}

fn default_calculation_type() -> String {
    "simple".to_string()
}

impl ValidateBody for MetricRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

fn parse_calculation_type(raw: &str) -> Result<graph_owl_core::metric::CalculationType, AppError> {
    graph_owl_core::metric::CalculationType::parse(raw).map_err(|other| {
        AppError::Validation(vec![FieldError::new(
            "calculationType",
            FieldErrorCode::Type,
            format!("`{other}` is not a recognised calculation type"),
        )])
    })
}

fn metric_body(metric: &graph_owl_storage::MetricRecord) -> serde_json::Value {
    let gaps = graph_owl_core::metric::gaps(&graph_owl_core::metric::MetricClaims {
        source_assets: &metric.source_assets,
        defined_by: metric.defined_by.map(|_| "_").as_deref(),
        formula: metric.formula.as_deref(),
    });
    json!({
        "id": metric.id,
        "name": metric.name,
        "fullyQualifiedName": metric.fully_qualified_name,
        "definition": metric.definition,
        "formula": metric.formula,
        "unit": metric.unit,
        "granularity": metric.granularity,
        "calculationType": metric.calculation_type.as_str(),
        "definedBy": metric.defined_by,
        "sourceAssets": metric.source_assets,
        "gaps": gaps.iter().map(|g| g.as_str()).collect::<Vec<_>>(),
        "createdAt": metric.created_at,
        "updatedAt": metric.updated_at,
    })
}

async fn create_metric(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<MetricRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let calculation_type = parse_calculation_type(&payload.calculation_type)?;
    let metric = catalog
        .create_metric(
            &payload.name,
            payload.definition,
            payload.formula,
            payload.unit,
            payload.granularity,
            calculation_type,
            payload.source_assets,
            payload.defined_by,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(metric_body(&metric))))
}

async fn list_metrics(
    State(catalog): State<Catalog>,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    let page = catalog.list_metrics(&page).await?;
    Ok(Json(json!({
        "data": page.data.iter().map(metric_body).collect::<Vec<_>>(),
        "paging": page.paging,
    })))
}

async fn get_metric(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    catalog
        .get_metric(id)
        .await?
        .map(|metric| Json(metric_body(&metric)))
        .ok_or(AppError::NotFound)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricUpdateRequest {
    #[serde(default)]
    definition: Option<String>,
    #[serde(default)]
    formula: Option<String>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    granularity: Option<String>,
    #[serde(default)]
    calculation_type: Option<String>,
}

impl ValidateBody for MetricUpdateRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

async fn update_metric(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<MetricUpdateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let calculation_type = payload
        .calculation_type
        .as_deref()
        .map(parse_calculation_type)
        .transpose()?;
    let metric = catalog
        .update_metric(
            id,
            graph_owl_storage::MetricUpdate {
                definition: payload.definition,
                formula: payload.formula,
                unit: payload.unit,
                granularity: payload.granularity,
                calculation_type,
            },
        )
        .await?;
    Ok(Json(metric_body(&metric)))
}

async fn delete_metric(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.delete_metric(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchMetricsQuery {
    q: String,
}

async fn search_metrics(
    State(catalog): State<Catalog>,
    AppQuery(query): AppQuery<SearchMetricsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let metrics = catalog.search_metrics(&query.q).await?;
    Ok(Json(json!(
        metrics.iter().map(metric_body).collect::<Vec<_>>()
    )))
}

// ---- Epic 24 Slice F: metric lineage reconciliation ----

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricSourcesRequest {
    #[serde(default)]
    source_assets: Vec<String>,
}

impl ValidateBody for MetricSourcesRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Replace a metric's declared sources — Slice F. Scoped to
/// `metric_sources`, not yet a graph-traversable lineage edge; see
/// [`graph_owl_api::Catalog::set_metric_sources`]'s doc for why.
async fn set_metric_sources(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<MetricSourcesRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let metric = catalog
        .set_metric_sources(id, payload.source_assets)
        .await?;
    Ok(Json(metric_body(&metric)))
}

// ---- Epic 31: organizational memory ----

/// A memory as a client submits it.
///
/// **No `id`, no `authorship`, no `supersedes`/`supersededBy`.** The id is the
/// server's; authorship comes from the authenticated principal, because a body
/// that could name its own author is a body that can forge one, and the whole
/// trust model rests on it; the supersession fields are set by the supersede
/// operation, which writes both halves at once. Structural rather than validated
/// — serde drops what is not here, so there is nothing for a future handler to
/// forget to reject.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryRequest {
    kind: graph_owl_core::memory::MemoryKind,
    content: String,
    #[serde(default)]
    summary: Option<String>,
    /// Omitted means "the default for this author": `1.0` for a person, and a
    /// refusal for an agent, which must state its own.
    #[serde(default)]
    confidence: Option<f64>,
    links: Vec<graph_owl_core::memory::MemoryLink>,
    /// When this was true of its subject. Defaults to now, because the common
    /// case is writing down what you just learned.
    #[serde(default)]
    as_of: Option<chrono::DateTime<chrono::Utc>>,
}

impl ValidateBody for MemoryRequest {
    /// Shape only. "A memory needs an anchor", "confidence is between 0 and 1"
    /// and "an agent must state its own confidence" are all enforced by
    /// `Memory::new`, and a rule stated in two places is a rule that will
    /// disagree with itself.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// The principal, as authorship.
///
/// **Taken from the token, never from the body.** A bot principal becomes agent
/// authorship, a person becomes human authorship — the distinction is the trust
/// model, and letting a request assert it would make the whole ranking term
/// meaningless.
fn authorship_of(principal: &Principal) -> graph_owl_core::memory::Authorship {
    match principal.kind {
        // `Service` and `System` both mean "not a person". `System` reaching this
        // path at all would be a migration or a reconciler writing a memory, and
        // recording that as human-authored is the exact relabelling the trust
        // model refuses — so the non-person branch is the default and `User` is
        // the one that has to be proven.
        graph_owl_core::PrincipalKind::User => graph_owl_core::memory::Authorship::Human {
            user_id: principal.id.clone(),
        },
        graph_owl_core::PrincipalKind::Service | graph_owl_core::PrincipalKind::System => {
            graph_owl_core::memory::Authorship::Agent {
                agent_id: principal.id.clone(),
                // The model is not in the token. Recorded as unknown rather than
                // guessed: "which model said this" matters when its conclusions
                // turn out wrong, and a fabricated answer is worse than an
                // absent one.
                model: "unknown".to_string(),
            }
        }
    }
}

fn memory_body(memory: &graph_owl_core::memory::Memory) -> serde_json::Value {
    json!(memory)
}

/// Write something down — Epic 31 Slice A.
async fn create_memory(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<MemoryRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let mut memory = graph_owl_core::memory::Memory::new(
        payload.kind,
        payload.content,
        authorship_of(&principal),
        payload.confidence,
        payload.links,
        payload.as_of.unwrap_or_else(chrono::Utc::now),
    )
    .map_err(memory_rejection)?;
    memory.summary = payload.summary;

    catalog.create_memory(&memory).await?;
    Ok((StatusCode::CREATED, Json(memory_body(&memory))))
}

/// A domain refusal as a field error.
///
/// Each maps to the field a client can actually change. `NoAnchor` points at
/// `links` rather than at the memory as a whole, because "add an about link" is
/// the fix and a message about the memory does not say that.
fn memory_rejection(error: graph_owl_core::memory::MemoryError) -> AppError {
    use graph_owl_core::memory::MemoryError;
    let (field, code) = match &error {
        MemoryError::NoAnchor => ("links", FieldErrorCode::Required),
        MemoryError::NoContent => ("content", FieldErrorCode::Empty),
        MemoryError::ConfidenceOutOfRange(_) | MemoryError::AgentWithoutConfidence => {
            ("confidence", FieldErrorCode::Type)
        }
    };
    AppError::Validation(vec![FieldError::new(field, code, error.to_string())])
}

async fn get_memory(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    // A superseded memory is returned, not 404'd: the record of what people
    // believed before they were corrected is most of the reason to keep a record,
    // and the body carries `supersededBy` so a reader can follow the correction.
    catalog
        .memory(id)
        .await?
        .map(|memory| Json(memory_body(&memory)))
        .ok_or(AppError::NotFound)
}

/// Correct a memory — Epic 31 Slice B.
///
/// `409` when it has already been corrected, naming the current one. A client
/// with only "no" cannot retry against the right target.
async fn supersede_memory(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<MemoryRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let mut replacement = graph_owl_core::memory::Memory::new(
        payload.kind,
        payload.content,
        authorship_of(&principal),
        payload.confidence,
        payload.links,
        payload.as_of.unwrap_or_else(chrono::Utc::now),
    )
    .map_err(memory_rejection)?;
    replacement.summary = payload.summary;
    replacement.supersedes = Some(id);

    catalog.supersede_memory(id, &replacement).await?;
    Ok((StatusCode::CREATED, Json(memory_body(&replacement))))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveMentionRequest {
    text: String,
    expected_type: Option<graph_owl_core::AssetKind>,
    #[serde(default)]
    context: String,
}

impl ValidateBody for ResolveMentionRequest {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("text"),
            &mut errors,
        );
        errors
    }
}

/// `POST /memories/{id}/mentions` — Epic 17 Slice G. `id` is the mention's
/// source (the memory it was found in). **Never a merge** — a `null`
/// resolution (no candidate cleared the threshold) is a normal `200`, not an
/// error.
async fn resolve_mention(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<ResolveMentionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mention = graph_owl_core::resolution::TextMention {
        text: payload.text,
        expected_type: payload.expected_type,
        context: payload.context,
    };
    let resolution = catalog.resolve_mention(&principal, id, mention).await?;
    Ok(Json(json!({ "resolution": resolution })))
}

/// `POST /merges/{id}/split` — Epic 17 Slice E. Restores both entities;
/// splitting an already-split merge is a `409` (`ConflictKind::MergeAlreadySplit`).
async fn split_merge(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_core::resolution::MergeRecord>, AppError> {
    let record = catalog.split_merge(&principal, id).await?;
    Ok(Json(record))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewQueueQuery {
    status: Option<graph_owl_core::resolution::ReviewStatus>,
    kind: Option<graph_owl_core::AssetKind>,
    min_score: Option<f64>,
    max_score: Option<f64>,
    limit: Option<usize>,
    offset: Option<usize>,
}

/// `GET /resolution/queue` — Epic 17 Slice F. Pending by default; a queue is
/// worked from the top, so a larger default limit would ship rows nobody
/// scrolls to (matching `validation_report`'s reasoning).
async fn review_queue(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Query(query): Query<ReviewQueueQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = query.limit.unwrap_or(50).min(200);
    let filter = graph_owl_storage::ReviewQueueFilter {
        status: query.status,
        kind: query.kind,
        min_score: query.min_score,
        max_score: query.max_score,
        limit,
        offset: query.offset.unwrap_or(0),
    };
    let (entries, total) = catalog.review_queue(&principal, &filter).await?;
    Ok(Json(json!({ "data": entries, "total": total })))
}

/// `POST /resolution/queue/{id}/confirm` — Epic 17 Slice F. Writes the merge
/// `decided_by: Human`; `409` if this entry was already decided.
async fn confirm_review(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_core::resolution::Resolution>, AppError> {
    let resolution = catalog.confirm_review(&principal, id).await?;
    Ok(Json(resolution))
}

/// `POST /resolution/queue/{id}/reject` — Epic 17 Slice F. Records the
/// decision so the pair is not re-queued by a later re-resolution.
async fn reject_review(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.reject_review(&principal, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BulkReviewDecision {
    ids: Vec<Uuid>,
    decision: BulkDecisionKind,
}

impl ValidateBody for BulkReviewDecision {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value
            .get("ids")
            .and_then(serde_json::Value::as_array)
            .is_none_or(std::vec::Vec::is_empty)
        {
            errors.push(FieldError::new(
                "ids",
                FieldErrorCode::Required,
                "at least one id is required".to_string(),
            ));
        }
        errors
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum BulkDecisionKind {
    Confirm,
    Reject,
}

/// `POST /resolution/queue/bulk` — Epic 17 Slice F's bulk confirm/reject.
///
/// One request, N independent decisions: each id's outcome is reported on
/// its own rather than the whole batch failing for one bad id, which is
/// what makes "confirm these 40, three of which someone already rejected"
/// a normal result instead of a client having to resubmit the other 37.
async fn bulk_decide_review(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<BulkReviewDecision>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut results = Vec::with_capacity(payload.ids.len());
    for id in payload.ids {
        let outcome = match payload.decision {
            BulkDecisionKind::Confirm => catalog.confirm_review(&principal, id).await.map(|_| ()),
            BulkDecisionKind::Reject => catalog.reject_review(&principal, id).await,
        };
        results.push(json!({
            "id": id,
            "ok": outcome.is_ok(),
            "problem": outcome.err().map(|e| AppError::from(e).detail()),
        }));
    }
    Ok(Json(json!({ "data": results })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecallQuery {
    /// The words to rank against. Absent is legitimate — "everything we know
    /// about this table" is a real question — and scores zero on the lexical
    /// term rather than producing `NaN`.
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    include_superseded: bool,
}

/// What we know about an asset, best first — Epic 31 Slice C.
async fn recall_memories(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    Query(params): Query<RecallQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let recalled = catalog
        .recall(
            id,
            params.q.as_deref().unwrap_or(""),
            params.include_superseded,
        )
        .await?;

    Ok(Json(json!(
        recalled
            .iter()
            .map(|item| json!({
                "memory": memory_body(&item.memory),
                // **Flagged, never hidden.** A stale memory is returned with its
                // verdict; dropping it leaves a reader believing nobody looked.
                "staleness": item.staleness,
                // The decomposition, so a reader who disagrees with the order can
                // see which term produced it.
                "score": item.score,
            }))
            .collect::<Vec<_>>()
    )))
}

/// Open disagreements about an asset — Epic 31 Slice E.
///
/// Nothing is resolved and neither memory is hidden. A human decides.
async fn list_contradictions(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(json!(catalog.contradictions_about(id).await?)))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRequest {
    a: Uuid,
    b: Uuid,
    /// `confirmed` or `dismissed`. **No default** — a verdict this endpoint had to
    /// guess would be a judgement about institutional disagreement made by the
    /// absence of a field.
    verdict: graph_owl_core::contradiction::Verdict,
    /// Nullable: "these are about different quarters" is worth capturing, and
    /// forcing a note gets the field filled with "n/a".
    #[serde(default)]
    note: Option<String>,
}

impl ValidateBody for ReviewRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Confirm or dismiss a candidate contradiction — Epic 31 Slice E.
///
/// Recorded against the reviewing principal, because a verdict with no author is
/// an unattributable judgement about institutional disagreement, which is the one
/// thing this epic must never produce.
///
/// **Confirming does not close it.** The pair stays in the queue marked
/// confirmed; only a dismissal removes it. Neither memory is ever hidden and
/// neither is ever picked as the winner.
async fn review_contradiction(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<ReviewRequest>,
) -> Result<StatusCode, AppError> {
    catalog
        .review_contradiction(
            graph_owl_core::contradiction::Review {
                a: payload.a,
                b: payload.b,
                verdict: payload.verdict,
            },
            &principal.id,
            payload.note.as_deref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnersRequest {
    /// The complete list. An empty array is a legitimate request that makes the
    /// asset unowned — a real, reportable state.
    owners: Vec<graph_owl_core::ownership::OwnerRef>,
}

impl ValidateBody for OwnersRequest {
    /// Shape only. "This principal exists" is a fact about the estate that only
    /// the facade can check, and a rule stated in two places is a rule that will
    /// disagree with itself.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Set who owns an asset — Epic 11 Slice C.
///
/// Ownership is a governance statement about accountability, so who may set it is
/// an administrative question rather than a cataloguing one.
async fn set_asset_owners(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<OwnersRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let owners = catalog.set_asset_owners(id, &payload.owners).await?;
    Ok(Json(json!({ "owners": owners })))
}

/// Who owns this asset.
async fn get_asset_owners(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(json!({ "owners": catalog.asset_owners(id).await? })))
}

/// Create or update a team — Epic 11.
async fn upsert_team(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<TeamRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // A team is who owns things, so who may define one is an administrative
    // question rather than a cataloguing one.
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let stored = catalog
        .upsert_team(&graph_owl_storage::Team {
            id: payload.id,
            display_name: payload.display_name,
            description: payload.description,
            members: payload.members,
            parent_team_id: payload.parent_team_id,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(team_body(&stored))))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserRequest {
    display_name: String,
    #[serde(default)]
    email: Option<String>,
}

impl ValidateBody for UserRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Create or update a user — Epic 11 Slice A's missing half.
///
/// **Users previously existed only by signing in.** Auto-provisioning on first
/// authentication (Epic 12 Slice A) is right for people who use the console, and
/// it left no way to name somebody who has not yet — so a person could not be
/// recorded as an owner at all until they logged in, which is exactly backwards
/// for onboarding.
///
/// `PUT` on the id rather than `POST` to a collection: the caller chooses the id
/// (it is the identity-provider subject), so the request is idempotent and a retry
/// is not a second user.
async fn upsert_user(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    AppJson(payload): AppJson<UserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let stored = catalog
        .upsert_user_record(&id, &payload.display_name, payload.email.as_deref())
        .await?;
    Ok(Json(json!({
        "id": stored.id,
        "displayName": stored.display_name,
        "email": stored.email,
        "roles": stored.roles,
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletePrincipalQuery {
    /// Transfer what this principal owns before deleting it. `kind` is required
    /// alongside, because `users.id` and `teams.id` can collide and transferring
    /// ownership to the wrong principal is not recoverable by reading the response.
    #[serde(default)]
    reassign_to: Option<String>,
    #[serde(default)]
    reassign_to_kind: Option<graph_owl_core::ownership::OwnerKind>,
}

/// Resolve the optional reassignment target from a delete request.
fn reassignment(
    query: &DeletePrincipalQuery,
) -> Result<Option<graph_owl_core::ownership::OwnerRef>, AppError> {
    match (&query.reassign_to, query.reassign_to_kind) {
        (None, _) => Ok(None),
        (Some(id), Some(kind)) => Ok(Some(graph_owl_core::ownership::OwnerRef {
            id: id.clone(),
            kind,
        })),
        // **Not defaulted to `user`.** Guessing the kind would transfer an estate
        // to whichever principal happened to share the id.
        (Some(_), None) => Err(AppError::Validation(vec![FieldError::new(
            "reassignToKind",
            FieldErrorCode::Required,
            "`reassignToKind` is required with `reassignTo`: a user and a team can \
             share an id, and guessing would transfer ownership to the wrong one",
        )])),
    }
}

async fn delete_user(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    AppQuery(query): AppQuery<DeletePrincipalQuery>,
) -> Result<StatusCode, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let target = reassignment(&query)?;
    catalog
        .delete_principal(
            &graph_owl_core::ownership::OwnerRef {
                id,
                kind: graph_owl_core::ownership::OwnerKind::User,
            },
            target.as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_team(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    AppQuery(query): AppQuery<DeletePrincipalQuery>,
) -> Result<StatusCode, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let target = reassignment(&query)?;
    catalog
        .delete_principal(
            &graph_owl_core::ownership::OwnerRef {
                id,
                kind: graph_owl_core::ownership::OwnerKind::Team,
            },
            target.as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Teams reporting into this one.
async fn list_child_teams(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let children = catalog.child_teams(&id).await?;
    Ok(Json(json!(
        children.iter().map(team_body).collect::<Vec<_>>()
    )))
}

/// Follow an asset — Epic 11 Slice F.
///
/// **`200`, not `201`, and idempotent.** Following what you already follow is the
/// state you asked for; a `409` would make a retried request look like a conflict
/// and a `201` would claim a second edge was created.
async fn follow_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path((id, user_id)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Following on somebody else's behalf is an administrative act; following for
    // yourself is not.
    if principal.id != user_id && !principal.is_admin {
        return Err(AppError::Forbidden);
    }
    let outcome = catalog.follow_asset(id, &user_id).await?;
    Ok(Json(json!({
        "following": true,
        "created": outcome == graph_owl_storage::FollowOutcome::Followed,
        "followerCount": catalog.follower_count(id).await?,
    })))
}

async fn unfollow_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path((id, user_id)): Path<(Uuid, String)>,
) -> Result<StatusCode, AppError> {
    if principal.id != user_id && !principal.is_admin {
        return Err(AppError::Forbidden);
    }
    catalog.unfollow_asset(id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a user follows, paginated like every other asset page.
async fn list_follows(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<String>,
    AppQuery(query): AppQuery<AssetListQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.assets_followed_by(&id, &page).await?))
}

/// Every team, so an owner picker has something to offer.
async fn list_teams(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    let teams = catalog.teams().await?;
    Ok(Json(json!(teams.iter().map(team_body).collect::<Vec<_>>())))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RolesRequest {
    /// The complete set, not a delta. A grant-only endpoint cannot express
    /// revocation, and revocation is the operation that has to work.
    roles: Vec<String>,
}

impl ValidateBody for RolesRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Replace a user's roles — Epic 13.
///
/// Admin-only: granting oneself a role is the shortest path to every other
/// permission, so this is the endpoint where a missing check is worst.
///
/// `PUT` rather than `PATCH` because the body is the whole set. A partial
/// update could not express "remove every role", which is the operation that
/// most needs to be expressible.
async fn set_user_roles(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    AppJson(payload): AppJson<RolesRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let user = catalog.set_user_roles(&id, payload.roles).await?;
    Ok(Json(json!({
        "id": user.id,
        "displayName": user.display_name,
        "roles": user.roles,
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssignRequest {
    shape: String,
    focus_node: String,
    #[serde(default)]
    path: Option<String>,
    constraint: String,
    /// A `users.id`. Free text is refused, because a finding assigned to a name
    /// nobody can resolve looks worked and is not.
    assignee: String,
}

impl ValidateBody for AssignRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Put a finding on somebody's plate — Epic 41.
async fn assign_finding(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<AssignRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let finding = graph_owl_storage::ValidationFinding {
        id: Uuid::new_v4(),
        shape: payload.shape,
        focus_node: payload.focus_node,
        path: payload.path,
        constraint_kind: payload.constraint,
        severity: String::new(),
        message: String::new(),
        actual: None,
        suggestion: None,
    };

    let assignment = catalog
        .assign_finding(&principal, &finding, &payload.assignee)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": assignment.id,
            "assignee": assignment.assignee,
            "assignedBy": assignment.assigned_by,
        })),
    ))
}

/// Take a finding off somebody's plate.
async fn unassign_finding(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.unassign_finding(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DryRunRequest {
    /// The policy as it would be saved.
    policy: graph_owl_authz::Policy,
    /// Whose access to simulate. Roles matter: a policy is only meaningful
    /// against a subject, and "what would this do" has no answer without one.
    #[serde(default)]
    roles: Vec<String>,
}

impl ValidateBody for DryRunRequest {
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// What a policy *would* do, without saving it — Epic 41.
///
/// **Writes nothing.** A dry-run that persisted would be the opposite of a dry
/// run, and the whole reason to offer one is that a policy is hard to reason
/// about and easy to get catastrophically wrong in the permissive direction.
///
/// Reports counts *and* examples: "admits 4,231 assets" is what a reader acts
/// on, and a handful of names is how they check the count means what they
/// think.
async fn dry_run_policy(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<DryRunRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }

    let outcome = catalog
        .dry_run_policy(&payload.policy, &payload.roles)
        .await?;

    Ok(Json(json!({
        "admitted": outcome.admitted,
        "denied": outcome.denied,
        "total": outcome.admitted + outcome.denied,
        // A sample, not the whole estate: a dry-run that returned every FQN
        // would be a second way to enumerate the catalog, and this endpoint is
        // about the *shape* of the decision.
        "examples": outcome.examples,
        // **The one an admin is really asking about.** A policy that admits
        // everything is almost always a mistake, and it looks identical to a
        // correct one in a count alone.
        "admitsEverything": outcome.admits_everything,
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaiveRequest {
    /// The finding's *identity*, not its row id: results are replaced wholesale
    /// each pass and every row gets a fresh id, so a waiver keyed on one would
    /// survive until the next run and then point at nothing.
    shape: String,
    focus_node: String,
    #[serde(default)]
    path: Option<String>,
    constraint: String,
    reason: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl ValidateBody for WaiveRequest {
    /// The reason and the expiry are checked in the facade, not here: both are
    /// governance rules ("a waiver has to say why", "a waiver has to expire"),
    /// and a rule stated in two places is a rule that will disagree with itself.
    /// Shape alone is this trait's job.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Accept a violation, on the record — Epic 41.
async fn waive_finding(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<WaiveRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let finding = graph_owl_storage::ValidationFinding {
        id: Uuid::new_v4(),
        shape: payload.shape,
        focus_node: payload.focus_node,
        path: payload.path,
        constraint_kind: payload.constraint,
        severity: String::new(),
        message: String::new(),
        actual: None,
        suggestion: None,
    };

    let waiver = catalog
        .waive_finding(&principal, &finding, &payload.reason, payload.expires_at)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": waiver.id,
            "reason": waiver.reason,
            "waivedBy": waiver.waived_by,
            "expiresAt": waiver.expires_at,
        })),
    ))
}

/// Withdraw a waiver, putting the finding back in the queue.
///
/// `204` whether or not one was there: revoking twice is the same intent twice,
/// and a `404` would make a client treat an already-clean state as a failure.
async fn revoke_waiver(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.revoke_waiver(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Write the shapes the core entity model ships with — Epic 5.
///
/// Explicit rather than automatic on startup: a server that silently seeds
/// governance rules re-imposes one somebody removed on purpose, on every
/// restart. Admin-only, because a shape is a rule.
async fn seed_core_shapes(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let written = catalog.seed_core_shapes().await?;
    Ok(Json(json!({ "flakes": written })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportQuery {
    severity: Option<String>,
    shape: Option<String>,
    /// The asset panel's filter: everything wrong with one node.
    focus_node: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

/// The violations queue — Epic 5 Slice E.
///
/// Reads stored results. A pass is triggered explicitly, so this endpoint is
/// cheap enough for a view that polls it.
async fn validation_report(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Query(query): Query<ReportQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 50, matching the page size the rest of the API uses. A queue is worked
    // from the top, so a larger default would ship rows nobody scrolls to.
    let limit = query.limit.unwrap_or(50).min(200);
    let filter = graph_owl_storage::ValidationFilter {
        severity: query.severity,
        shape: query.shape,
        focus_node: query.focus_node,
        limit,
        offset: query.offset.unwrap_or(0),
    };

    let (findings, computed_at_t, total) = catalog.validation_report(&filter).await?;

    Ok(Json(json!({
        "data": findings.iter().map(|row| json!({
            "id": row.finding.id,
            "shape": row.finding.shape,
            "focusNode": row.finding.focus_node,
            "path": row.finding.path,
            "constraint": row.finding.constraint_kind,
            "severity": row.finding.severity,
            "message": row.finding.message,
            "actual": row.finding.actual,
            "suggestion": row.finding.suggestion,
            // **Marked, not hidden.** A waived finding removed from the queue
            // is one nobody reviews — including nobody noticing its acceptance
            // is about to lapse.
            // Independent of the waiver: "somebody is on this" and "somebody
            // accepted this" are different statements, and either can hold
            // without the other.
            "assignment": row.assignment.as_ref().map(|a| json!({
                "id": a.id,
                "assignee": a.assignee,
                "assignedBy": a.assigned_by,
                "assignedAt": a.assigned_at,
            })),
            "waiver": row.waiver.as_ref().map(|w| json!({
                "id": w.id,
                "reason": w.reason,
                "waivedBy": w.waived_by,
                "waivedAt": w.waived_at,
                "expiresAt": w.expires_at,
                // An expired waiver and no waiver at all look identical
                // otherwise, and only the first is somebody's to answer for.
                "expired": row.waiver_expired,
            })),
        })).collect::<Vec<_>>(),
        // **The instant this reflects.** A validation report whose currency is
        // unknown is unactionable: a steward cannot tell a queue that is clean
        // from one that has not run since the data changed.
        "computedAtT": computed_at_t,
        "total": total,
        "limit": filter.limit,
        "offset": filter.offset,
    })))
}

/// Run the reasoner and replace the overlay — Epic 6 Slice E.
///
/// `POST` because it writes, even though it derives nothing a caller supplied:
/// the run replaces `graph:reasoning` wholesale, and a `GET` that rewrites a
/// graph is a `GET` no cache, proxy or retry can treat correctly.
///
/// Admin-only, for the same reason reconciliation is: a full forward-chaining
/// pass over the estate is the cheapest way an unprivileged caller could load
/// the database.
async fn run_reasoning(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<graph_owl_api::ReasoningReport>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    // The budget is the server's, not the caller's — the same rule SPARQL
    // follows. A client that can raise its own limit does not have one.
    let report = catalog
        .run_reasoning(&graph_owl_reasoning::Budget::default())
        .await?;
    Ok(Json(report))
}

#[derive(Debug, serde::Deserialize)]
struct DerivedQuery {
    subject: String,
}

/// What the reasoner concluded about one subject — Epic 6 Slice E.
///
/// The overlay as stored, not a fresh pass: an asset page opens with this, and
/// re-deriving per page view would make the catalog slowest where it is browsed
/// most.
async fn derived_about(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Query(query): Query<DerivedQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let subject = parse_sid("subject", &query.subject)?;
    let facts = catalog.derived_about(&subject).await?;
    Ok(Json(json!(
        facts.iter().map(flake_body).collect::<Vec<_>>()
    )))
}

/// A triple, named the way flakes name one: `namespace:local` per position.
#[derive(Debug, serde::Deserialize)]
struct ExplainQuery {
    s: String,
    p: String,
    o: String,
}

/// `ns:local` back into an identifier.
///
/// Split on the **first** colon: a local name may contain one — `graph:reasoning`
/// is itself a local name in the `dsc` namespace — and splitting on the last
/// would silently reattribute it to a different vocabulary.
fn parse_sid(field: &str, raw: &str) -> Result<graph_owl_core::flake::Sid, AppError> {
    let invalid = |detail: String| {
        AppError::Validation(vec![FieldError::new(field, FieldErrorCode::Type, detail)])
    };
    let (namespace, local) = raw
        .split_once(':')
        .ok_or_else(|| invalid(format!("`{raw}` is not `namespace:name`")))?;
    let code: u16 = namespace
        .parse()
        .map_err(|_| invalid(format!("`{namespace}` is not a namespace code")))?;
    if local.is_empty() {
        return Err(invalid(format!("`{raw}` names no local part")));
    }
    Ok(graph_owl_core::flake::Sid::new(code, local))
}

/// Why a fact holds — Epic 6 Slice D.
///
/// `404` when the fact is neither asserted nor implied, because "nothing
/// supports this" and "this is supported by nothing" read alike and mean
/// opposite things. `400` when an identifier does not parse, which tells the
/// caller the difference between a mistake and a missing fact.
async fn explain_fact(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Query(query): Query<ExplainQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let subject = parse_sid("s", &query.s)?;
    let predicate = parse_sid("p", &query.p)?;
    let object = parse_sid("o", &query.o)?;

    let explanation = catalog
        .explain_fact(
            &subject,
            &predicate,
            &object,
            &graph_owl_reasoning::Budget::default(),
        )
        .await?;
    Ok(Json(explanation_body(&explanation)))
}

/// The explanation as a wire document.
///
/// Written out rather than derived from serde on the enum: the recursion is the
/// point of this endpoint, and a reader consuming it needs one predictable
/// discriminator at every level rather than serde's nesting for a tuple
/// variant.
fn explanation_body(explanation: &graph_owl_reasoning::Explanation) -> serde_json::Value {
    use graph_owl_reasoning::Explanation;
    match explanation {
        Explanation::Asserted(fact) => json!({ "status": "asserted", "fact": flake_body(fact) }),
        Explanation::Circular(fact) => json!({ "status": "circular", "fact": flake_body(fact) }),
        Explanation::Unknown => json!({ "status": "unknown" }),
        Explanation::Derived { chains } => json!({
            "status": "derived",
            "chains": chains
                .iter()
                .map(|chain| json!({
                    "rule": chain.rule,
                    "premises": chain.premises.iter().map(explanation_body).collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>(),
        }),
    }
}

fn flake_body(flake: &graph_owl_core::flake::Flake) -> serde_json::Value {
    json!({
        "s": flake.s.to_string(),
        "p": flake.p.to_string(),
        "o": match &flake.o {
            graph_owl_core::flake::FlakeValue::Ref(sid) => sid.to_string(),
            other => format!("{other:?}"),
        },
        "t": flake.t,
    })
}

/// Re-project whatever the graph is missing, and report the drift either way.
///
/// A `POST` because it repairs; the drift count in the response is what makes
/// it useful to call even when nothing needs repairing — that number is the
/// operability signal Slice G asks for, and an endpoint that only reported
/// after fixing would have no way to say "nothing is wrong".
async fn reconcile_projection(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    // Reconciliation rewrites the graph view of the whole estate. That is an
    // administrative operation, not a read, and a non-admin triggering it
    // repeatedly is a cheap way to load the database.
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let drifted = catalog.projection_drift().await?.len();
    let repaired = catalog.reconcile_projection().await?;
    Ok(Json(json!({ "drifted": drifted, "repaired": repaired })))
}

/// Everything the landing page needs, in one request.
async fn overview(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    let overview = catalog.overview(&principal).await?;
    Ok(Json(json!({
        "assets": {
            "total": overview.total,
            "byKind": overview.by_kind.iter()
                .map(|(kind, n)| json!({ "kind": kind.as_str(), "count": n }))
                .collect::<Vec<_>>(),
        },
        "documentation": {
            "described": overview.described,
            "total": overview.documented_total,
        },
        "graph": overview.graph,
        "recentlyChanged": overview.recently_changed,
    })))
}

async fn asset_stats(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    // Counted through the same predicate as the rows: a total computed before
    // filtering leaks the existence of what it filtered out.
    let counts = catalog.count_assets_by_kind_for(&principal).await?;
    Ok(Json(json!({
        "byKind": counts
            .into_iter()
            .map(|(kind, n)| json!({ "kind": kind.as_str(), "count": n }))
            .collect::<Vec<_>>(),
    })))
}

// ---- connector runs (Epic 15) ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunPostgresConnector {
    connection_string: String,
    service_name: String,
    #[serde(default)]
    include_schemas: Vec<String>,
    /// Tombstone assets the source no longer reports. Off by default: a run
    /// that deletes is a different kind of operation from one that only adds,
    /// and defaulting to the destructive reading of "sync" is how a routine
    /// re-run becomes an incident.
    #[serde(default)]
    detect_deletions: bool,
    /// Fraction of the scope this run may tombstone before it refuses.
    /// Absent uses [`DeletionPlan::DEFAULT_THRESHOLD`].
    deletion_threshold: Option<f64>,
}

impl ValidateBody for RunPostgresConnector {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("connectionString"),
            &mut errors,
        );
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("serviceName"),
            &mut errors,
        );
        errors
    }
}

async fn run_postgres_connector(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<RunPostgresConnector>,
) -> Result<Json<serde_json::Value>, AppError> {
    let connector = PostgresConnector::connect(&payload.connection_string, &payload.service_name)
        .await
        .map_err(|error| {
            AppError::Validation(vec![FieldError::new(
                "connectionString",
                FieldErrorCode::Type,
                error.to_string(),
            )])
        })?;

    let scope = RunScope {
        include_schemas: payload.include_schemas,
    };
    let records = connector
        .fetch(&scope)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;

    // Per-record failure does not abort the run (15-connectors.md Slice B): a
    // single unreadable table must not cost the other nine hundred.
    // Opened before the work, so a run that dies mid-flight leaves a row with
    // no `finished_at` rather than leaving nothing. A history that only records
    // completions cannot show a crash, which is what it is most needed for.
    let mut run = graph_owl_storage::ConnectorRun {
        id: Uuid::new_v4(),
        connector: connector.type_name().to_string(),
        service_name: payload.service_name.clone(),
        started_at: chrono::Utc::now(),
        finished_at: None,
        created: 0,
        skipped: 0,
        failed: 0,
        deleted: 0,
        failures: json!([]),
        refusal: None,
        triggered_by: principal.id.clone(),
    };
    // Recording history must not fail the run it is recording. A catalogue that
    // refused to sync because its own audit row would not write would be
    // trading the thing for the record of the thing.
    let _ = catalog.begin_run(&run).await;

    let mut created = 0;
    let mut skipped = 0;
    let mut failures: Vec<serde_json::Value> = Vec::new();
    // What the source reported *and* the catalog accepted. Deletion is decided
    // against this, never against the fetched list: an asset that failed to
    // ingest is a write problem, and treating it as absent would convert a
    // transient error into a tombstone.
    let mut ingested: std::collections::HashSet<String> = std::collections::HashSet::new();

    // One round trip for the whole batch (decision 7). The point of
    // fingerprinting is to make an unchanged re-run cheap, and a lookup per
    // record would replace the write it saves with a read.
    let fqns: Vec<String> = records.iter().map(|record| record.path.join(".")).collect();
    let existing = catalog
        .existing_fingerprints(&fqns)
        .await
        .unwrap_or_default();

    for record in records {
        let path = record.path.join(".");
        let hash = record.source_hash();
        let outcome = graph_owl_connectors::decide_ingest(
            existing
                .get(&path)
                .copied()
                // An FQN the batch lookup did not answer for is treated as
                // absent, which creates. Guessing "unchanged" on a failed read
                // would skip a write on the strength of a query that did not
                // succeed.
                .unwrap_or(graph_owl_connectors::Existing::Absent),
            hash,
        );

        if outcome == graph_owl_connectors::Ingest::Skip {
            skipped += 1;
            // Counted as reported-by-the-source, which is what deletion
            // detection reconciles against. A skipped record is present at the
            // source; omitting it here would tombstone every unchanged asset on
            // the first run that used fingerprinting.
            ingested.insert(path);
            continue;
        }

        match catalog
            .ingest_record(
                &principal,
                record.kind,
                &record.path,
                record.description,
                record.properties,
            )
            .await
        {
            Ok(asset) => {
                created += 1;
                // After the write, never before: a fingerprint recorded for a
                // write that then failed would skip the retry.
                let _ = catalog.remember_source_hash(asset.id, &hash).await;
                ingested.insert(asset.fully_qualified_name);
            }
            // A run that reports only a count tells an operator something is
            // wrong and nothing about what. Each failure names the record and
            // the reason.
            Err(error) => {
                let app_error = AppError::from(error);
                let mut failure = json!({ "path": path, "reason": app_error.detail() });
                if let AppError::Validation(errors) = &app_error {
                    failure["errors"] = json!(errors);
                }
                failures.push(failure);
            }
        }
    }

    // Deletion runs *after* ingestion, over what the source actually reported.
    // Running it first would delete against a stale picture; running it on the
    // fetched records rather than the ingested ones would tombstone anything
    // that failed to ingest, turning a transient write error into data loss.
    let deletions = if payload.detect_deletions {
        let threshold = payload
            .deletion_threshold
            .unwrap_or(DeletionPlan::DEFAULT_THRESHOLD);
        Some(
            catalog
                .reconcile_deletions(&principal, &payload.service_name, &ingested, threshold)
                .await?,
        )
    } else {
        None
    };

    run.finished_at = Some(chrono::Utc::now());
    run.created = i32::try_from(created).unwrap_or(i32::MAX);
    run.skipped = i32::try_from(skipped).unwrap_or(i32::MAX);
    run.failed = i32::try_from(failures.len()).unwrap_or(i32::MAX);
    run.deleted = deletions
        .as_ref()
        .map_or(0, |plan| i32::try_from(plan.absent).unwrap_or(i32::MAX));
    run.refusal = deletions.as_ref().and_then(|plan| plan.refused.clone());
    run.failures = json!(failures);
    let _ = catalog.finish_run(&run).await;

    Ok(Json(json!({
        "runId": run.id,
        "connector": connector.type_name(),
        "serviceName": payload.service_name,
        "created": created,
        // Reported, not inferred. A run that wrote nothing because nothing
        // changed and a run that wrote nothing because it was broken produce
        // the same `created` count, and an operator needs to tell them apart.
        "skipped": skipped,
        "failed": failures.len(),
        "failures": failures,
        "deletions": deletions,
    })))
}

/// Recent connector runs, newest first.
///
/// Unfiltered by service unless asked, because the first question after a
/// nightly sync is "did anything run", not "did this one run".
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunHistoryQuery {
    service_name: Option<String>,
    limit: Option<usize>,
}

/// Bounded so a history that has grown for a year cannot be asked for at once.
const RUN_HISTORY_MAX: usize = 100;

async fn list_connector_runs(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppQuery(query): AppQuery<RunHistoryQuery>,
) -> Result<Json<Vec<graph_owl_storage::ConnectorRun>>, AppError> {
    let _ = principal;
    let limit = query.limit.unwrap_or(20).min(RUN_HISTORY_MAX);
    Ok(Json(
        catalog
            .recent_runs(query.service_name.as_deref().unwrap_or_default(), limit)
            .await?,
    ))
}

// ---- lineage (Epic 29) ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssertLineage {
    from_asset_id: Uuid,
    to_asset_id: Uuid,
    /// `feeds` or `derivedFrom`. Defaulted to `feeds`, which is the edge people
    /// mean when they say lineage; `derivedFrom` is provenance and is asked for
    /// deliberately.
    #[serde(default)]
    relationship: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

impl ValidateBody for AssertLineage {
    /// Nothing beyond the field types. The rules that matter here — the two
    /// endpoints differ, the kinds may carry lineage, both exist — need the
    /// *assets*, which only the facade can read. Restating them as shape checks
    /// would put half the rule in one place and half in another.
    fn validate_body(_: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

async fn assert_lineage(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<AssertLineage>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, String); 1],
        Json<graph_owl_core::lineage::LineageEdge>,
    ),
    AppError,
> {
    let relationship = graph_owl_core::relationship_type::RelationshipType::parse(
        payload.relationship.as_deref().unwrap_or("feeds"),
    )
    .map_err(|unknown| {
        AppError::Validation(vec![FieldError::new(
            "relationship",
            FieldErrorCode::Type,
            format!("`{}` is not a relationship type", unknown.got),
        )])
    })?;

    let source = graph_owl_core::lineage::LineageSource::parse(
        payload.source.as_deref().unwrap_or("manual"),
    )
    .map_err(|unknown| {
        AppError::Validation(vec![FieldError::new(
            "source",
            FieldErrorCode::Type,
            format!("`{unknown}` is not a lineage source; expected manual or connector"),
        )])
    })?;

    let edge = catalog
        .assert_lineage(
            &principal,
            payload.from_asset_id,
            payload.to_asset_id,
            relationship,
            graph_owl_core::lineage::LineageDetails {
                source,
                query: payload.query,
                description: payload.description,
            },
        )
        .await?;
    let location = format!("/lineage/{}", edge.id);
    Ok((
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(edge),
    ))
}

// ---- Epic 22: organization-defined custom properties ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefineCustomProperty {
    name: String,
    entity_type: String,
    /// A wire name, parsed here rather than deserialized straight into the
    /// enum: serde's own error for an unknown variant does not list what *is*
    /// supported, and a client told only "unsupported" has to go and find the
    /// documentation.
    property_type: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    constraints: graph_owl_core::custom_property::Constraints,
}

impl ValidateBody for DefineCustomProperty {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["name", "entityType", "propertyType"] {
            if value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(|found| found.trim().is_empty())
            {
                errors.push(FieldError::new(
                    field,
                    FieldErrorCode::Required,
                    format!("`{field}` is required"),
                ));
            }
        }

        if let Some(kind) = value.get("entityType").and_then(serde_json::Value::as_str)
            && AssetKind::parse(kind).is_err()
        {
            errors.push(FieldError::new(
                "entityType",
                FieldErrorCode::Value,
                format!("`{kind}` is not an entity type"),
            ));
        }

        if let Some(name) = value
            .get("propertyType")
            .and_then(serde_json::Value::as_str)
            && graph_owl_core::custom_property::PropertyType::parse(name).is_err()
        {
            // **The supported set is listed.** Decision 4 is a closed type set
            // on purpose, so a client that named one outside it needs to know
            // what the alternatives are — not merely that it guessed wrong.
            let supported: Vec<&str> = graph_owl_core::custom_property::PropertyType::all()
                .iter()
                .map(|property_type| property_type.as_str())
                .collect();
            errors.push(FieldError::new(
                "propertyType",
                FieldErrorCode::Value,
                format!(
                    "`{name}` is not a supported type; supported: {}",
                    supported.join(", ")
                ),
            ));
        }

        errors
    }
}

async fn define_custom_property(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<DefineCustomProperty>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let _ = principal;
    let property_type = graph_owl_core::custom_property::PropertyType::parse(
        &payload.property_type,
    )
    .map_err(|unknown| {
        AppError::Validation(vec![FieldError::new(
            "propertyType",
            FieldErrorCode::Value,
            format!("`{unknown}` is not a supported type"),
        )])
    })?;

    let (id, property) = catalog
        .define_custom_property(graph_owl_core::custom_property::CustomProperty {
            name: payload.name,
            entity_type: payload.entity_type,
            property_type,
            description: payload.description,
            constraints: payload.constraints,
        })
        .await?;

    let mut body = serde_json::to_value(&property).unwrap_or(serde_json::Value::Null);
    if let Some(object) = body.as_object_mut() {
        object.insert("id".to_string(), serde_json::json!(id));
    }
    Ok((StatusCode::CREATED, Json(body)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomPropertyQuery {
    entity_type: Option<String>,
}

async fn list_custom_properties(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Query(query): Query<CustomPropertyQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let _ = principal;
    let properties = catalog
        .list_custom_properties(query.entity_type.as_deref())
        .await?;

    Ok(Json(
        properties
            .into_iter()
            .map(|(id, property)| {
                let mut body = serde_json::to_value(&property).unwrap_or(serde_json::Value::Null);
                if let Some(object) = body.as_object_mut() {
                    object.insert("id".to_string(), serde_json::json!(id));
                }
                body
            })
            .collect(),
    ))
}

/// A change to a definition — Epic 22 Slice C.
///
/// **No `entityType`.** Moving a definition between entity types is a delete and
/// a define, not an edit, and every value under the old type would be orphaned
/// by an operation that reads like a rename. `deny_unknown_fields` means sending
/// one is a `400` rather than a silently dropped field.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::option_option)]
struct UpdateCustomProperty {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    property_type: Option<String>,
    #[serde(default, deserialize_with = "optional_double_option")]
    description: Option<Option<String>>,
    #[serde(default)]
    constraints: Option<graph_owl_core::custom_property::Constraints>,
}

/// Absent stays `None`; present — including an explicit `null` — becomes
/// `Some`. The same double-option trick `AssetUpdate` uses, spelled locally
/// because it is a wire concern rather than a domain one.
///
/// `option_option` is exactly the shape being asked for here: the two levels
/// mean different things ("not mentioned" and "clear it"), which is the
/// distinction the lint assumes is accidental.
#[allow(clippy::option_option)]
fn optional_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

impl ValidateBody for UpdateCustomProperty {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if let Some(name) = value.get("name")
            && name.as_str().is_none_or(|found| found.trim().is_empty())
        {
            errors.push(FieldError::new(
                "name",
                FieldErrorCode::Required,
                "`name` cannot be blank; omit it to leave the name alone",
            ));
        }
        errors
    }
}

async fn update_custom_property(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<UpdateCustomProperty>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = principal;
    let property_type = match payload.property_type.as_deref() {
        None => None,
        Some(name) => Some(
            graph_owl_core::custom_property::PropertyType::parse(name).map_err(|unknown| {
                AppError::Validation(vec![FieldError::new(
                    "propertyType",
                    FieldErrorCode::Value,
                    format!("`{unknown}` is not a supported type"),
                )])
            })?,
        ),
    };

    let property = catalog
        .update_custom_property(
            id,
            graph_owl_api::CustomPropertyUpdate {
                name: payload.name,
                property_type,
                description: payload.description,
                constraints: payload.constraints,
            },
        )
        .await?;

    let mut body = serde_json::to_value(&property).unwrap_or(serde_json::Value::Null);
    if let Some(object) = body.as_object_mut() {
        object.insert("id".to_string(), serde_json::json!(id));
    }
    Ok(Json(body))
}

/// `?force=true` on a delete.
///
/// A query parameter rather than a body, because a `DELETE` with a body is
/// something proxies and clients disagree about — and this is a flag on the
/// operation, not data.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ForceQuery {
    #[serde(default)]
    force: Option<bool>,
}

async fn delete_custom_property(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<ForceQuery>,
) -> Result<StatusCode, AppError> {
    catalog
        .delete_custom_property(&principal, id, query.force.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Epic 30: quality signals ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTestDefinitionBody {
    name: String,
    test_type: String,
    #[serde(default)]
    description: Option<String>,
    /// ISO 8601, days and smaller. A year is not a fixed length of time and a
    /// cadence has to be answerable by subtracting two instants.
    #[serde(default)]
    expected_cadence: Option<String>,
}

impl ValidateBody for CreateTestDefinitionBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["name", "testType"] {
            require_non_empty_string(
                value,
                &graph_owl_api::validation::FieldPath::root().key(field),
                &mut errors,
            );
        }
        errors
    }
}

fn definition_body(d: &graph_owl_storage::StoredTestDefinition) -> serde_json::Value {
    json!({
        "id": d.id,
        "name": d.name,
        "testType": d.test_type,
        "description": d.description,
        "expectedCadence": d.expected_cadence,
    })
}

async fn create_test_definition(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<CreateTestDefinitionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let created = catalog
        .create_test_definition(
            payload.name,
            payload.test_type,
            payload.description,
            payload.expected_cadence,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(definition_body(&created))))
}

async fn list_test_definitions(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .list_test_definitions()
            .await?
            .iter()
            .map(definition_body)
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CadenceBody {
    #[serde(default)]
    expected_cadence: Option<String>,
}

impl ValidateBody for CadenceBody {
    fn validate_body(_value: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

/// **The whole point of the definition/case split** (decision 3a): one edit,
/// and every case that inherited the cadence follows.
async fn set_definition_cadence(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<CadenceBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let affected = catalog
        .set_definition_cadence(id, payload.expected_cadence)
        .await?;
    Ok(Json(json!({ "affectedCases": affected })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTestSuiteBody {
    name: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

impl ValidateBody for CreateTestSuiteBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

async fn create_test_suite(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<CreateTestSuiteBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let id = catalog
        .create_test_suite(payload.name, payload.owner, payload.description)
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTestCaseBody {
    name: String,
    target_fqn: String,
    test_type: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    definition_id: Option<Uuid>,
    #[serde(default)]
    suite_id: Option<Uuid>,
    #[serde(default)]
    expected_cadence: Option<String>,
}

impl ValidateBody for CreateTestCaseBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["name", "targetFqn", "testType"] {
            require_non_empty_string(
                value,
                &graph_owl_api::validation::FieldPath::root().key(field),
                &mut errors,
            );
        }
        errors
    }
}

fn case_body(c: &graph_owl_storage::StoredTestCase) -> serde_json::Value {
    json!({
        "id": c.id,
        "name": c.name,
        "targetFqn": c.target_fqn,
        "testType": c.test_type,
        "description": c.description,
        "definitionId": c.definition_id,
        "suiteId": c.suite_id,
        // Already resolved: the case's own cadence, or the definition's.
        "expectedCadence": c.expected_cadence,
    })
}

async fn create_test_case(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<CreateTestCaseBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let created = catalog
        .create_test_case(graph_owl_api::CreateTestCase {
            name: payload.name,
            target_fqn: payload.target_fqn,
            test_type: payload.test_type,
            description: payload.description,
            definition_id: payload.definition_id,
            suite_id: payload.suite_id,
            expected_cadence: payload.expected_cadence,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(case_body(&created))))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TestCaseQuery {
    #[serde(default)]
    target_fqn: Option<String>,
    #[serde(default)]
    suite_id: Option<Uuid>,
}

async fn list_test_cases(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<TestCaseQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .list_test_cases(query.target_fqn.as_deref(), query.suite_id)
            .await?
            .iter()
            .map(case_body)
            .collect(),
    ))
}

async fn delete_test_case(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.delete_test_case(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultBatchBody {
    results: Vec<TestResultBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TestResultBody {
    status: String,
    observed_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    metrics: Option<serde_json::Value>,
}

impl ValidateBody for ResultBatchBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if !value
            .get("results")
            .is_some_and(serde_json::Value::is_array)
        {
            errors.push(FieldError::new(
                "results",
                FieldErrorCode::Required,
                "`results` must be an array",
            ));
        }
        errors
    }
}

/// **Never bumps the entity version and emits no change event** (decision 2). A
/// nightly suite across ten thousand tables would otherwise fill every history
/// with observations rather than changes.
async fn record_test_results(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<ResultBatchBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    use graph_owl_core::quality::TestStatus;

    let mut batch = Vec::with_capacity(payload.results.len());
    for result in payload.results {
        let status = TestStatus::parse(&result.status).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "results.status",
                FieldErrorCode::Value,
                format!(
                    "`{unknown}` is not a test status; expected one of: {}",
                    TestStatus::all()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        })?;
        batch.push(graph_owl_storage::TestResultWrite {
            case_id: id,
            status,
            observed_at: result.observed_at,
            message: result.message,
            metrics: result.metrics,
        });
    }

    let ingest = catalog.record_test_results(batch).await?;
    Ok(Json(json!({
        "accepted": ingest.accepted,
        "duplicates": ingest.duplicates,
        "rejected": ingest.rejected,
        "unknownCase": ingest.unknown_case,
    })))
}

async fn list_test_results(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .test_results(id)
            .await?
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "caseId": r.case_id,
                    "status": r.status,
                    "observedAt": r.observed_at,
                    "message": r.message,
                    "metrics": r.metrics,
                })
            })
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HealthQuery {
    /// Off by default, because the walk costs a query per upstream asset —
    /// and because upstream health is a different question from this asset's.
    #[serde(default)]
    include_upstream: Option<bool>,
}

async fn asset_health(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
    AppQuery(query): AppQuery<HealthQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let own = catalog.health_of(&fqn).await?;
    let mut body = json!({ "health": own });

    if query.include_upstream.unwrap_or(false) {
        // **Reported separately, never merged into the asset's own.**
        // Conflating them makes the signal unactionable: a steward cannot tell
        // whether to fix this table or go upstream.
        let upstream = catalog.upstream_health(&fqn).await?;
        if let Some(object) = body.as_object_mut() {
            object.insert(
                "upstream".to_string(),
                serde_json::to_value(upstream).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    Ok(Json(body))
}

async fn prune_test_results(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    // Admin-only, for the same reason usage pruning is: a scan-and-delete over
    // one of the largest tables in the system.
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let pruned = catalog.prune_test_results().await?;
    Ok(Json(json!({ "pruned": pruned })))
}

// ---- Epic 29 Slices D and E: column lineage and reconciliation ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ColumnMappingsBody {
    mappings: Vec<ColumnMappingBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ColumnMappingBody {
    from_column_fqn: String,
    to_column_fqn: String,
    #[serde(default)]
    expression: Option<String>,
}

impl ValidateBody for ColumnMappingsBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if !value
            .get("mappings")
            .is_some_and(serde_json::Value::is_array)
        {
            errors.push(FieldError::new(
                "mappings",
                FieldErrorCode::Required,
                "`mappings` must be an array",
            ));
        }
        errors
    }
}

/// `PUT`, because the mappings are replaced wholesale: a partial update cannot
/// express "this column now comes from one source instead of two", and that is
/// the edit a refactor produces.
async fn set_column_mappings(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<ColumnMappingsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mappings = payload
        .mappings
        .into_iter()
        .map(|m| graph_owl_storage::ColumnMapping {
            from_column_fqn: m.from_column_fqn,
            to_column_fqn: m.to_column_fqn,
            expression: m.expression,
        })
        .collect();
    let count = catalog.set_column_mappings(id, mappings).await?;
    Ok(Json(json!({ "mappings": count })))
}

async fn get_column_mappings(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .column_mappings(id)
            .await?
            .iter()
            .map(|m| {
                json!({
                    "fromColumnFqn": m.from_column_fqn,
                    "toColumnFqn": m.to_column_fqn,
                    "expression": m.expression,
                })
            })
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileLineageBody {
    source: String,
    /// The FQN prefix this run enumerated. **Required**, because a
    /// reconciliation with no scope would replace every edge this source ever
    /// asserted anywhere — including in schemas the run never looked at.
    scope_prefix: String,
    #[serde(default)]
    edges: Vec<ReconcileEdgeBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileEdgeBody {
    from_asset_id: Uuid,
    to_asset_id: Uuid,
    relationship: String,
}

impl ValidateBody for ReconcileLineageBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["source", "scopePrefix"] {
            require_non_empty_string(
                value,
                &graph_owl_api::validation::FieldPath::root().key(field),
                &mut errors,
            );
        }
        errors
    }
}

async fn reconcile_lineage(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<ReconcileLineageBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let asserted: Vec<(Uuid, Uuid, String)> = payload
        .edges
        .into_iter()
        .map(|e| (e.from_asset_id, e.to_asset_id, e.relationship))
        .collect();
    let report = catalog
        .reconcile_lineage(
            &principal,
            &payload.source,
            &payload.scope_prefix,
            &asserted,
        )
        .await?;
    Ok(Json(json!({
        "added": report.added,
        // Only this source's, within this scope — a curated edge is never in
        // this count.
        "removed": report.removed,
    })))
}

// ---- Epic 27: data contracts ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateContractBody {
    name: String,
    asset_fqn: String,
    producer: String,
    #[serde(default)]
    consumers: Vec<String>,
    #[serde(default)]
    schema_guarantee: graph_owl_core::contract::SchemaGuarantee,
    #[serde(default)]
    slas: Vec<graph_owl_core::contract::Sla>,
    /// Defaults to `none`: a contract that has not stated a compatibility mode
    /// has not agreed to one, and inferring a strict default would make every
    /// schema change a breach for contracts nobody wrote that intent into.
    #[serde(default)]
    compatibility: Option<String>,
    /// Defaults to `draft`, because a contract is a proposal until somebody
    /// activates it — and a `Draft` one is not evaluated.
    #[serde(default)]
    status: Option<String>,
}

impl ValidateBody for CreateContractBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["name", "assetFqn", "producer"] {
            require_non_empty_string(
                value,
                &graph_owl_api::validation::FieldPath::root().key(field),
                &mut errors,
            );
        }
        errors
    }
}

fn parse_compatibility(raw: Option<&str>) -> Result<CompatibilityMode, AppError> {
    match raw {
        None => Ok(CompatibilityMode::None),
        Some(name) => CompatibilityMode::parse(name).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "compatibility",
                FieldErrorCode::Value,
                format!(
                    "`{unknown}` is not a compatibility mode; expected one of: {}",
                    CompatibilityMode::all()
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        }),
    }
}

fn parse_contract_status(raw: Option<&str>) -> Result<ContractStatus, AppError> {
    match raw {
        None => Ok(ContractStatus::Draft),
        Some(name) => ContractStatus::parse(name).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "status",
                FieldErrorCode::Value,
                format!(
                    "`{unknown}` is not a contract status; expected one of: {}",
                    ContractStatus::all()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        }),
    }
}

async fn create_contract(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateContractBody>,
) -> Result<(StatusCode, Json<graph_owl_core::contract::Contract>), AppError> {
    let compatibility = parse_compatibility(payload.compatibility.as_deref())?;
    let status = parse_contract_status(payload.status.as_deref())?;

    let created = catalog
        .create_contract(
            &principal,
            graph_owl_api::CreateContract {
                name: payload.name,
                asset_fqn: payload.asset_fqn,
                producer: payload.producer,
                consumers: payload.consumers,
                schema_guarantee: payload.schema_guarantee,
                slas: payload.slas,
                compatibility,
                status,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContractListQuery {
    #[serde(default)]
    asset_fqn: Option<String>,
}

async fn list_contracts(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<ContractListQuery>,
) -> Result<Json<Vec<graph_owl_core::contract::Contract>>, AppError> {
    Ok(Json(
        catalog.list_contracts(query.asset_fqn.as_deref()).await?,
    ))
}

async fn get_contract(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let stored = catalog.get_contract(id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({
        "contract": stored.contract,
        // Breaches ride with the contract rather than behind a second request:
        // "is this contract in good standing" is the question every reader
        // has, and answering it in two round trips invites the second one
        // being skipped.
        "breaches": stored.breaches,
    })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContractStatusBody {
    status: String,
}

impl ValidateBody for ContractStatusBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("status"),
            &mut errors,
        );
        errors
    }
}

async fn set_contract_status(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<ContractStatusBody>,
) -> Result<StatusCode, AppError> {
    let status = parse_contract_status(Some(&payload.status))?;
    catalog.set_contract_status(&principal, id, status).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_contract_breaches(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cleared = catalog.clear_contract_breaches(&principal, id).await?;
    Ok(Json(json!({ "cleared": cleared })))
}

async fn evaluate_slas(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .evaluate_slas(id)
            .await?
            .into_iter()
            .map(|(sla, evaluation)| json!({ "sla": sla, "evaluation": evaluation }))
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SchemaChangeBody {
    change: graph_owl_core::contract::SchemaChange,
    /// The asset version the change produced, so "when did this break" is
    /// answerable against the asset's own history rather than only a timestamp.
    #[serde(default)]
    asset_version: Option<String>,
}

impl ValidateBody for SchemaChangeBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value.get("change").is_none() {
            errors.push(FieldError::new(
                "change",
                FieldErrorCode::Required,
                "`change` is required",
            ));
        }
        errors
    }
}

/// **Reports, never blocks** (decision 3). graph-owl observes metadata and
/// cannot stop a warehouse `ALTER TABLE`, so this returns `200` with the
/// breaches it found rather than refusing anything — a `409` here would be a
/// promise the system has no way to keep.
async fn evaluate_schema_change(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
    AppJson(payload): AppJson<SchemaChangeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let version = payload
        .asset_version
        .unwrap_or_else(|| "unknown".to_string());
    let breaches = catalog
        .evaluate_schema_change(&fqn, &payload.change, &version)
        .await?;

    Ok(Json(json!({
        "breaches": breaches
            .iter()
            .map(|report| json!({
                "contractId": report.contract_id,
                "contractName": report.contract_name,
                "producer": report.producer,
                "consumers": report.consumers,
                "column": report.column,
                "detail": report.detail,
            }))
            .collect::<Vec<_>>(),
    })))
}

// ---- Epic 28: usage and popularity ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsageBatchBody {
    observations: Vec<UsageObservationBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsageObservationBody {
    asset_fqn: String,
    /// A warehouse identity. Resolved to a principal if one matches, kept
    /// opaque otherwise — a username nothing here matches is still a distinct
    /// consumer, and dropping it would under-count exactly the external usage
    /// nobody has onboarded.
    consumer: String,
    #[serde(default)]
    consumer_is_principal: bool,
    operation: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    row_count: Option<i64>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    query_id: Option<String>,
    /// Accepted on the wire and **dropped before storage** unless the
    /// deployment opted in. Accepting-then-dropping rather than refusing,
    /// because a client pushing a log it cannot easily strip should not have
    /// its whole batch rejected over a field the server was never going to keep.
    #[serde(default)]
    query_text: Option<String>,
}

impl ValidateBody for UsageBatchBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if !value
            .get("observations")
            .is_some_and(serde_json::Value::is_array)
        {
            errors.push(FieldError::new(
                "observations",
                FieldErrorCode::Required,
                "`observations` must be an array",
            ));
        }
        errors
    }
}

async fn record_usage(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppJson(payload): AppJson<UsageBatchBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    use graph_owl_core::usage::{Consumer, UsageOperation};

    let mut batch = Vec::with_capacity(payload.observations.len());
    for observation in payload.observations {
        let operation = UsageOperation::parse(&observation.operation).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "observations.operation",
                FieldErrorCode::Value,
                format!(
                    "`{unknown}` is not a usage operation; expected one of: {}",
                    UsageOperation::all()
                        .iter()
                        .map(|o| o.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        })?;
        let consumer = if observation.consumer_is_principal {
            Consumer::Principal {
                id: observation.consumer,
            }
        } else {
            Consumer::Opaque {
                identifier: observation.consumer,
            }
        };
        batch.push(graph_owl_storage::UsageWrite {
            asset_fqn: observation.asset_fqn,
            consumer,
            operation,
            occurred_at: observation.occurred_at,
            row_count: observation.row_count,
            duration_ms: observation.duration_ms,
            query_id: observation.query_id,
            query_text: observation.query_text,
        });
    }

    let ingest = catalog.record_usage(batch).await?;
    Ok(Json(json!({
        "accepted": ingest.accepted,
        // Reported rather than hidden: an operator wants to know how much of a
        // push landed on assets the catalog has never seen, because that is a
        // connector gap rather than a usage fact.
        "unmatched": ingest.unmatched,
        "duplicates": ingest.duplicates,
        "rejected": ingest.rejected,
    })))
}

async fn asset_popularity(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
) -> Result<Json<graph_owl_core::usage::PopularitySummary>, AppError> {
    Ok(Json(catalog.popularity(&fqn).await?))
}

async fn asset_rollups(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .usage_rollups(&fqn)
            .await?
            .iter()
            .map(|rollup| {
                json!({
                    "consumerKey": rollup.consumer_key,
                    "day": rollup.day,
                    "operation": rollup.operation,
                    "count": rollup.count,
                    "totalRows": rollup.total_rows,
                })
            })
            .collect(),
    ))
}

async fn prune_usage(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<serde_json::Value>, AppError> {
    // Admin-only, for the same reason reconciliation is: a scan-and-delete over
    // the largest table in the system is the cheapest way an unprivileged
    // caller could load the database.
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    let pruned = catalog.prune_usage().await?;
    Ok(Json(json!({ "pruned": pruned })))
}

// ---- Epic 25: tags and classifications ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateClassificationBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// Defaulting to `false` because most vocabularies are additive —
    /// `PII.Sensitive` beside `PII.Restricted` is normal — and a default that
    /// refused the common case would be discovered only after somebody hit it.
    #[serde(default)]
    mutually_exclusive: bool,
}

impl ValidateBody for CreateClassificationBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

async fn create_classification(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateClassificationBody>,
) -> Result<
    (
        StatusCode,
        Json<graph_owl_core::classification::Classification>,
    ),
    AppError,
> {
    let created = catalog
        .create_classification(
            &principal,
            payload.name,
            payload.description,
            payload.mutually_exclusive,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn list_classifications(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<Vec<graph_owl_core::classification::Classification>>, AppError> {
    Ok(Json(catalog.list_classifications().await?))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecursiveQuery {
    #[serde(default)]
    recursive: Option<bool>,
}

async fn delete_classification(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<RecursiveQuery>,
) -> Result<StatusCode, AppError> {
    catalog
        .delete_classification(id, query.recursive.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTagBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

impl ValidateBody for CreateTagBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

async fn create_tag(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<CreateTagBody>,
) -> Result<(StatusCode, Json<graph_owl_core::classification::Tag>), AppError> {
    let created = catalog
        .create_tag(&principal, id, payload.name, payload.description)
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TagListQuery {
    #[serde(default)]
    classification_id: Option<Uuid>,
}

async fn list_tags(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<TagListQuery>,
) -> Result<Json<Vec<graph_owl_core::classification::Tag>>, AppError> {
    Ok(Json(catalog.list_tags(query.classification_id).await?))
}

async fn tag_usage(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let usage = catalog.tag_usage(&fqn).await?;
    Ok(Json(json!({
        "total": usage.total(),
        "byKind": usage
            .by_kind
            .iter()
            .map(|(kind, count)| json!({ "kind": kind, "count": count }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ForceFlagQuery {
    #[serde(default)]
    force: Option<bool>,
}

async fn delete_tag(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(fqn): Path<String>,
    AppQuery(query): AppQuery<ForceFlagQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let removed = catalog
        .delete_tag(&principal, &fqn, query.force.unwrap_or(false))
        .await?;
    Ok(Json(json!({ "removedLabels": removed })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyLabelBody {
    tag_fqn: String,
    /// Defaults to a human applying it directly, which is what a bare `POST`
    /// from a console is. A scanner states `automated` and gets `suggested`
    /// with it.
    #[serde(default)]
    label_type: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

impl ValidateBody for ApplyLabelBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("tagFqn"),
            &mut errors,
        );
        errors
    }
}

async fn apply_tag_label(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(target_fqn): Path<String>,
    AppJson(payload): AppJson<ApplyLabelBody>,
) -> Result<StatusCode, AppError> {
    use graph_owl_core::classification::{LabelState, LabelType};

    let label_type = match payload.label_type.as_deref() {
        None => LabelType::Manual,
        Some(raw) => LabelType::parse(raw).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "labelType",
                FieldErrorCode::Value,
                format!(
                    "`{unknown}` is not a label type; expected one of: {}",
                    LabelType::all()
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )])
        })?,
    };
    // **A manual application defaults to confirmed; an automated one to
    // suggested.** A scanner's proposal counting as curation is the failure
    // decision 2 exists to prevent, and making the caller state it would mean
    // one that forgot got the dangerous answer.
    let state = match payload.state.as_deref() {
        Some(raw) => LabelState::parse(raw).map_err(|unknown| {
            AppError::Validation(vec![FieldError::new(
                "state",
                FieldErrorCode::Value,
                format!("`{unknown}` is not a label state; expected suggested or confirmed"),
            )])
        })?,
        None if matches!(label_type, LabelType::Automated | LabelType::Derived) => {
            LabelState::Suggested
        }
        None => LabelState::Confirmed,
    };

    catalog
        .apply_tag(&principal, &payload.tag_fqn, &target_fqn, label_type, state)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn labels_on(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(target_fqn): Path<String>,
) -> Result<Json<Vec<graph_owl_core::classification::TagLabel>>, AppError> {
    Ok(Json(catalog.labels_on(&target_fqn).await?))
}

async fn remove_tag_label(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path((target_fqn, tag_fqn)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    catalog.remove_tag(&tag_fqn, &target_fqn).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn confirm_label(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path((target_fqn, tag_fqn)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    catalog
        .decide_label(&principal, &tag_fqn, &target_fqn, true)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reject_label(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path((target_fqn, tag_fqn)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    catalog
        .decide_label(&principal, &tag_fqn, &target_fqn, false)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn propagate_label(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path((target_fqn, tag_fqn)): Path<(String, String)>,
    AppQuery(query): AppQuery<RecursiveQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let affected = catalog
        .propagate_tag(
            &principal,
            &tag_fqn,
            &target_fqn,
            query.recursive.unwrap_or(false),
        )
        .await?;
    Ok(Json(json!({ "affected": affected })))
}

async fn label_suggestions(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<Vec<graph_owl_core::classification::TagLabel>>, AppError> {
    Ok(Json(catalog.suggested_labels().await?))
}

// ---- Epic 26: lifecycle and certification ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetLifecycleBody {
    lifecycle: String,
    #[serde(default)]
    deprecation: Option<DeprecationBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeprecationBody {
    reason: String,
    #[serde(default)]
    successor_fqn: Option<String>,
    #[serde(default)]
    sunset_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ValidateBody for SetLifecycleBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("lifecycle"),
            &mut errors,
        );
        // A deprecation whose reason is blank is a deprecation nobody can act
        // on, and refusing it here means the facade's own check never has to
        // guess whether an empty string counts.
        // **The path is walked from the value it is given.** Passing the
        // nested `deprecation` object *and* a root-anchored path looks for
        // `deprecation.deprecation.reason`, which is never there — so every
        // deprecation carrying a perfectly good reason was refused as missing
        // one. The whole document, with the whole path.
        if value.get("deprecation").is_some() {
            require_non_empty_string(
                value,
                &graph_owl_api::validation::FieldPath::root()
                    .key("deprecation")
                    .key("reason"),
                &mut errors,
            );
        }
        errors
    }
}

async fn set_lifecycle(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<SetLifecycleBody>,
) -> Result<Json<Asset>, AppError> {
    use graph_owl_core::lifecycle::{Deprecation, LifecycleState};

    let to = LifecycleState::parse(&payload.lifecycle).map_err(|unknown| {
        AppError::Validation(vec![FieldError::new(
            "lifecycle",
            FieldErrorCode::Value,
            format!(
                "`{unknown}` is not a lifecycle state; expected one of: {}",
                LifecycleState::all()
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )])
    })?;

    let deprecation = payload.deprecation.map(|body| Deprecation {
        reason: body.reason,
        successor_fqn: body.successor_fqn,
        deprecated_at: chrono::Utc::now(),
        sunset_at: body.sunset_at,
    });

    Ok(Json(
        catalog
            .set_lifecycle(&principal, id, to, deprecation)
            .await?,
    ))
}

async fn terminal_successor(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(fqn): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // `null` rather than a 404 when the chain ends nowhere: "this is deprecated
    // and there is no replacement" is a real answer, and the most useful one an
    // agent can get short of a successor.
    let found = catalog.terminal_successor(&fqn).await?;
    Ok(Json(
        serde_json::to_value(found).unwrap_or(serde_json::Value::Null),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCertificationTypeBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    default_validity_days: i32,
    #[serde(default)]
    required_evidence: Vec<String>,
    #[serde(default)]
    authorized_issuers: Vec<String>,
}

impl ValidateBody for CreateCertificationTypeBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        if value.get("defaultValidityDays").is_none() {
            errors.push(FieldError::new(
                "defaultValidityDays",
                FieldErrorCode::Required,
                "a certification must expire, so its default validity is required",
            ));
        }
        errors
    }
}

fn certification_type_body(
    stored: &graph_owl_storage::StoredCertificationType,
) -> serde_json::Value {
    json!({
        "id": stored.id,
        "name": stored.name,
        "description": stored.description,
        "defaultValidityDays": stored.default_validity_days,
        "requiredEvidence": stored.required_evidence,
        "authorizedIssuers": stored.authorized_issuers,
    })
}

async fn create_certification_type(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateCertificationTypeBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let created = catalog
        .create_certification_type(
            &principal,
            graph_owl_api::CreateCertificationType {
                name: payload.name,
                description: payload.description,
                default_validity_days: payload.default_validity_days,
                required_evidence: payload.required_evidence,
                authorized_issuers: payload.authorized_issuers,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(certification_type_body(&created))))
}

async fn list_certification_types(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .list_certification_types()
            .await?
            .iter()
            .map(certification_type_body)
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IssueCertificationBody {
    type_id: Uuid,
    #[serde(default)]
    criteria: Option<String>,
    /// Omitted means "use the type's default validity" — the common case, and
    /// the one that keeps every certification of a type ageing at the same rate.
    #[serde(default)]
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    evidence: Vec<EvidenceBody>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidenceBody {
    kind: String,
    reference: String,
}

impl ValidateBody for IssueCertificationBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value.get("typeId").is_none() {
            errors.push(FieldError::new(
                "typeId",
                FieldErrorCode::Required,
                "`typeId` is required",
            ));
        }
        errors
    }
}

fn certification_body(
    stored: &graph_owl_storage::StoredCertification,
    status: graph_owl_core::lifecycle::CertificationStatus,
) -> serde_json::Value {
    json!({
        "id": stored.id,
        "targetFqn": stored.target_fqn,
        "typeId": stored.type_id,
        "typeName": stored.type_name,
        "issuer": stored.issuer,
        "criteria": stored.criteria,
        "issuedAt": stored.issued_at,
        "expiresAt": stored.expires_at,
        "evidence": stored
            .evidence
            .iter()
            .map(|(kind, reference)| json!({ "kind": kind, "reference": reference }))
            .collect::<Vec<_>>(),
        // **Computed here, on every read.** A stored status goes stale without
        // the entity changing, so an asset would read as certified for as long
        // as nobody wrote to it.
        "status": status,
    })
}

async fn issue_certification(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(target_fqn): Path<String>,
    AppJson(payload): AppJson<IssueCertificationBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let evidence = payload
        .evidence
        .into_iter()
        .map(|e| (e.kind, e.reference))
        .collect();
    let issued = catalog
        .issue_certification(
            &principal,
            &target_fqn,
            payload.type_id,
            payload.criteria,
            payload.expires_at,
            evidence,
        )
        .await?;
    let status = graph_owl_core::lifecycle::certification_status(
        Some(issued.expires_at),
        chrono::Utc::now(),
        graph_owl_core::lifecycle::DEFAULT_EXPIRY_WINDOW_DAYS,
    );
    Ok((
        StatusCode::CREATED,
        Json(certification_body(&issued, status)),
    ))
}

async fn certifications_on(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(target_fqn): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    Ok(Json(
        catalog
            .certifications_on(&target_fqn)
            .await?
            .iter()
            .map(|(stored, status)| certification_body(stored, *status))
            .collect(),
    ))
}

async fn recertification_queue(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let now = chrono::Utc::now();
    Ok(Json(
        catalog
            .recertification_queue()
            .await?
            .iter()
            .map(|stored| {
                let status = graph_owl_core::lifecycle::certification_status(
                    Some(stored.expires_at),
                    now,
                    graph_owl_core::lifecycle::DEFAULT_EXPIRY_WINDOW_DAYS,
                );
                certification_body(stored, status)
            })
            .collect(),
    ))
}

// ---- Epic 23: domains and data products ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateDomainBody {
    name: String,
    #[serde(default)]
    parent_id: Option<Uuid>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    domain_type: Option<String>,
    #[serde(default)]
    experts: Vec<String>,
}

impl ValidateBody for CreateDomainBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

/// **No `fullyQualifiedName`.** It is derived from the parent chain, and a
/// client-supplied path is a path that can disagree with the parent — the same
/// immutability-by-DTO-shape the asset hierarchy uses.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateDomainBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "optional_double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "optional_double_option")]
    domain_type: Option<Option<String>>,
    #[serde(default)]
    experts: Option<Vec<String>>,
    #[serde(default, deserialize_with = "optional_double_option")]
    parent_id: Option<Option<Uuid>>,
}

impl ValidateBody for UpdateDomainBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if let Some(name) = value.get("name")
            && name.as_str().is_none_or(|found| found.trim().is_empty())
        {
            errors.push(FieldError::new(
                "name",
                FieldErrorCode::Required,
                "`name` cannot be blank; omit it to leave the name alone",
            ));
        }
        errors
    }
}

async fn create_domain(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateDomainBody>,
) -> Result<(StatusCode, Json<graph_owl_core::domain::Domain>), AppError> {
    let domain = catalog
        .create_domain(
            &principal,
            graph_owl_api::CreateDomain {
                name: payload.name,
                parent_id: payload.parent_id,
                description: payload.description,
                domain_type: payload.domain_type,
                experts: payload.experts,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(domain)))
}

async fn list_domains(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<graph_owl_core::domain::Domain>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.list_domains(&page).await?))
}

async fn get_domain(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_core::domain::Domain>, AppError> {
    catalog
        .get_domain(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn child_domains(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<graph_owl_core::domain::Domain>>, AppError> {
    // A 404 for an absent parent rather than an empty list: "this domain has no
    // children" and "there is no such domain" are different answers, and a
    // client that cannot tell them apart will render an empty tree for a typo.
    if catalog.get_domain(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.child_domains(Some(id)).await?))
}

async fn update_domain(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<UpdateDomainBody>,
) -> Result<Json<graph_owl_core::domain::Domain>, AppError> {
    Ok(Json(
        catalog
            .update_domain(
                &principal,
                id,
                graph_owl_storage::DomainUpdate {
                    name: payload.name,
                    description: payload.description,
                    domain_type: payload.domain_type,
                    experts: payload.experts,
                    parent_id: payload.parent_id,
                },
            )
            .await?,
    ))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteDomainQuery {
    #[serde(default)]
    reassign_to: Option<Uuid>,
}

async fn delete_domain(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<DeleteDomainQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let outcome = catalog
        .delete_domain(&principal, id, query.reassign_to)
        .await?;
    // Reports what moved. A delete that silently reassigned five thousand
    // assets and returned 204 would leave an operator unable to tell whether it
    // did what they meant.
    let graph_owl_storage::DomainDeletion::Deleted {
        reassigned_assets,
        reassigned_products,
    } = outcome
    else {
        // Every other variant became an error in the facade; reaching here would
        // mean that mapping had a hole, and a silent 200 would hide it.
        return Err(AppError::Internal(
            "domain deletion reported an outcome the facade should have refused".to_string(),
        ));
    };
    Ok(Json(json!({
        "reassignedAssets": reassigned_assets,
        "reassignedDataProducts": reassigned_products,
    })))
}

async fn count_domain_assets(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    if catalog.get_domain(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let total = catalog.count_assets_in_domain(id).await?;
    Ok(Json(json!({ "total": total })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignDomainBody {
    domain_id: Uuid,
}

impl ValidateBody for AssignDomainBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("domainId"),
            &mut errors,
        );
        errors
    }
}

/// `?replace=true` on an assignment.
///
/// A query parameter rather than a second endpoint, for the same reason Epic
/// 22's `?force=true` is one: it is a flag on the operation saying the caller
/// meant it, not data about the thing being assigned.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceQuery {
    #[serde(default)]
    replace: Option<bool>,
}

async fn assign_asset_domain(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<ReplaceQuery>,
    AppJson(payload): AppJson<AssignDomainBody>,
) -> Result<Json<Asset>, AppError> {
    Ok(Json(
        catalog
            .assign_asset_domain(
                &principal,
                id,
                payload.domain_id,
                query.replace.unwrap_or(false),
            )
            .await?,
    ))
}

async fn clear_asset_domain(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Asset>, AppError> {
    Ok(Json(catalog.clear_asset_domain(&principal, id).await?))
}

async fn get_asset_domain(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    if catalog.get_asset(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    // `null` rather than a 404 when nothing resolves: "this asset is in no
    // domain" is a real and reportable state — it is the assignment-gap report
    // — and a 404 would make it indistinguishable from a bad id.
    let resolved = catalog.resolve_asset_domain(id).await?;
    Ok(Json(
        serde_json::to_value(resolved).unwrap_or(serde_json::Value::Null),
    ))
}

async fn get_asset_products(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<graph_owl_core::domain::DataProduct>>, AppError> {
    if catalog.get_asset(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.asset_products(id).await?))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateDataProductBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    domain_id: Option<Uuid>,
}

impl ValidateBody for CreateDataProductBody {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(
            value,
            &graph_owl_api::validation::FieldPath::root().key("name"),
            &mut errors,
        );
        errors
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateDataProductBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "optional_double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "optional_double_option")]
    purpose: Option<Option<String>>,
    #[serde(default, deserialize_with = "optional_double_option")]
    domain_id: Option<Option<Uuid>>,
}

impl ValidateBody for UpdateDataProductBody {
    fn validate_body(_value: &serde_json::Value) -> Vec<FieldError> {
        Vec::new()
    }
}

async fn create_data_product(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<CreateDataProductBody>,
) -> Result<(StatusCode, Json<graph_owl_core::domain::DataProduct>), AppError> {
    let product = catalog
        .create_data_product(
            &principal,
            graph_owl_api::CreateDataProduct {
                name: payload.name,
                description: payload.description,
                purpose: payload.purpose,
                domain_id: payload.domain_id,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(product)))
}

async fn list_data_products(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<graph_owl_core::domain::DataProduct>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.list_data_products(&page).await?))
}

async fn get_data_product(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_core::domain::DataProduct>, AppError> {
    catalog
        .get_data_product(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn update_data_product(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<UpdateDataProductBody>,
) -> Result<Json<graph_owl_core::domain::DataProduct>, AppError> {
    Ok(Json(
        catalog
            .update_data_product(
                &principal,
                id,
                graph_owl_storage::DataProductUpdate {
                    name: payload.name,
                    description: payload.description,
                    purpose: payload.purpose,
                    domain_id: payload.domain_id,
                },
            )
            .await?,
    ))
}

async fn delete_data_product(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.delete_data_product(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_product_assets(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    if catalog.get_data_product(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.product_assets(id, &page).await?))
}

/// `PUT`, because adding an asset that is already a member is the state the
/// caller asked for rather than an error — the same idempotency rule Epic 11's
/// follow endpoint follows.
async fn add_product_asset(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    catalog.add_product_asset(id, asset_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_product_asset(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    // **Removes the edge, never the asset.** A product is a view of things that
    // exist independently of it, and a membership removal that deleted the
    // table would be catastrophic and irreversible.
    catalog.remove_product_asset(id, asset_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Epic 21: the surface an out-of-process worker submits to ----

/// What a worker sends: the document it parsed and the claims it drew from it.
///
/// **The document travels with the claims, rather than being fetched here.**
/// The worker is the only party that read the source — graph-owl never saw the
/// PDF — so the parsed text is what every evidence span is an offset into. A
/// server that re-read the original would resolve spans against a file the
/// extractor never saw, which is silent drift on every later edit.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmitExtraction {
    document: graph_owl_core::extraction::ParsedDocument,
    result: graph_owl_core::extraction::ExtractionResult,
    /// The worker's name for itself (`pdf-worker`, `gpt-5-extractor`).
    /// A string, not an enum — adding a worker must be a deployment, not a
    /// migration of a type that has already been persisted.
    extractor: String,
    extractor_version: String,
}

impl ValidateBody for SubmitExtraction {
    /// **All three identify the run, and idempotence is judged on all three.**
    /// A blank extractor name would make every worker look like the same one,
    /// so a second worker over the same document would be mistaken for a
    /// re-run of the first and silently do nothing — a failure that reads as
    /// "the document had already been processed" rather than as a bad request.
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        for field in ["extractor", "extractorVersion"] {
            if value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(|found| found.trim().is_empty())
            {
                errors.push(FieldError::new(
                    field,
                    FieldErrorCode::Required,
                    format!("`{field}` identifies the run and must not be blank"),
                ));
            }
        }
        if value
            .get("document")
            .and_then(|document| document.get("sourceId"))
            .and_then(serde_json::Value::as_str)
            .is_none_or(|found| found.trim().is_empty())
        {
            errors.push(FieldError::new(
                "document.sourceId",
                FieldErrorCode::Required,
                "`document.sourceId` is how a re-ingestion recognises the same \
                 document and must not be blank"
                    .to_string(),
            ));
        }
        errors
    }
}

async fn submit_extraction(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppJson(payload): AppJson<SubmitExtraction>,
) -> Result<
    (
        StatusCode,
        Json<graph_owl_api::extraction::SubmissionOutcome>,
    ),
    AppError,
> {
    let _ = principal;

    let outcome = catalog
        .submit_extraction(
            &payload.document,
            payload.result,
            &payload.extractor,
            &payload.extractor_version,
        )
        .await?;

    // 200 for a document already extracted, 201 for one that produced a new
    // run. A worker retrying after a timeout gets a different status for
    // "nothing happened because it already had" than for "this created
    // something", which is the distinction its own logs need.
    let status = match outcome {
        graph_owl_api::extraction::SubmissionOutcome::Recorded { .. } => StatusCode::CREATED,
        graph_owl_api::extraction::SubmissionOutcome::AlreadyExtracted { .. } => StatusCode::OK,
    };
    Ok((status, Json(outcome)))
}

async fn extraction_queue(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<graph_owl_api::extraction::PendingClaim>>, AppError> {
    let _ = principal;
    Ok(Json(catalog.extraction_queue().await?))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimDecision {
    confirmed: bool,
}

impl ValidateBody for ClaimDecision {
    /// `confirmed` has no default. A missing field defaulting either way would
    /// turn a malformed request into a silent decision on somebody's behalf,
    /// and both directions are wrong: defaulting to true asserts what nobody
    /// approved, defaulting to false rejects what nobody refused.
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        if value
            .get("confirmed")
            .and_then(serde_json::Value::as_bool)
            .is_none()
        {
            return vec![FieldError::new(
                "confirmed",
                FieldErrorCode::Required,
                "`confirmed` must be true or false — there is no default for a \
                 review decision"
                    .to_string(),
            )];
        }
        Vec::new()
    }
}

async fn decide_extraction_claim(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppJson(payload): AppJson<ClaimDecision>,
) -> Result<StatusCode, AppError> {
    // **The reviewer is the authenticated caller, never a body field.** A
    // `decidedBy` a client could set would make the audit trail say whatever
    // the client wanted it to say, which is worse than having no audit trail
    // because it looks like one.
    catalog
        .decide_extraction_claim(id, payload.confirmed, &principal.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_extraction_run(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let _ = principal;
    catalog.delete_extraction_run(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_lineage(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let _ = principal;
    if catalog.remove_lineage(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LineageQuery {
    upstream: Option<usize>,
    downstream: Option<usize>,
}

/// How far a single request may walk.
///
/// Bounded because lineage graphs are the kind that surprise you: a warehouse
/// with a hundred views over one table produces a fan-out nobody predicted, and
/// an unbounded walk turns one click into a full-table read.
const MAX_LINEAGE_DEPTH: usize = 10;

async fn lineage_graph(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    AppQuery(query): AppQuery<LineageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = principal;
    let upstream = query.upstream.unwrap_or(1);
    let downstream = query.downstream.unwrap_or(1);
    if upstream > MAX_LINEAGE_DEPTH || downstream > MAX_LINEAGE_DEPTH {
        return Err(AppError::Validation(vec![FieldError::new(
            "upstream",
            FieldErrorCode::Type,
            format!("depth may not exceed {MAX_LINEAGE_DEPTH}"),
        )]));
    }

    // The root must exist. Answering an empty graph for a nonexistent asset
    // would read as "nothing feeds this", which is a different and wrong
    // statement.
    if catalog.get_asset(id).await?.is_none() {
        return Err(AppError::NotFound);
    }

    let (nodes, edges) = catalog.lineage_graph(id, upstream, downstream).await?;
    Ok(Json(json!({
        "rootId": id,
        "nodes": nodes.iter().map(|asset| json!({
            "id": asset.id,
            "name": asset.name,
            "kind": asset.kind.as_str(),
            "fullyQualifiedName": asset.fully_qualified_name,
            // Included rather than filtered: a lineage graph running into a
            // deleted table must show the break. "Nothing downstream" and "the
            // downstream was deleted" are opposite conclusions.
            "deleted": asset.deleted,
        })).collect::<Vec<_>>(),
        "edges": edges,
    })))
}

// ---- envelope (Epic 3) ----

/// Reads `If-Match: "0.2"` — the entity version the caller believed it was
/// editing.
///
/// Absent, the update is last-write-wins, which is the documented default
/// (`00d`). Present and stale, the write is refused rather than silently
/// discarding whatever landed in between.
fn if_match_version(headers: &axum::http::HeaderMap) -> Result<Option<EntityVersion>, AppError> {
    let Some(raw) = headers.get(axum::http::header::IF_MATCH) else {
        return Ok(None);
    };
    let raw = raw
        .to_str()
        .map_err(|_| {
            AppError::Validation(vec![FieldError::new(
                "If-Match",
                FieldErrorCode::Type,
                "the header is not valid text".to_string(),
            )])
        })?
        // Quoted per the HTTP entity-tag convention, but accepted bare too:
        // refusing `0.2` would be pedantry that costs a round trip and teaches
        // nothing.
        .trim()
        .trim_matches('"');

    let parsed = raw
        .split_once('.')
        .and_then(|(major, minor)| {
            Some(EntityVersion {
                major: major.parse().ok()?,
                minor: minor.parse().ok()?,
            })
        })
        .ok_or_else(|| {
            AppError::Validation(vec![FieldError::new(
                "If-Match",
                FieldErrorCode::Type,
                format!("`{raw}` is not a version of the form `major.minor`"),
            )])
        })?;
    Ok(Some(parsed))
}

async fn update_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    AppJson(payload): AppJson<AssetUpdate>,
) -> Result<Json<Asset>, AppError> {
    let expected = if_match_version(&headers)?;
    Ok(Json(
        catalog
            .update_asset(&principal, id, &payload, expected)
            .await?,
    ))
}

async fn asset_versions(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AssetVersion>>, AppError> {
    Ok(Json(catalog.asset_versions(id).await?))
}

async fn delete_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Reports the cascade count. A delete that silently tombstoned 400 columns
    // and returned 204 would leave an operator unable to tell whether it did
    // what they meant.
    let affected = catalog.soft_delete_asset(&principal, id).await?;
    Ok(Json(json!({ "deleted": affected })))
}

async fn restore_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let affected = catalog.restore_asset(&principal, id).await?;
    Ok(Json(json!({ "restored": affected })))
}

/// `POST /assets/{id}/resolve` — Epic 17. Deterministic match auto-merges;
/// `>= 0.9` auto-merges if enabled; `0.6`–`0.9` returns `Ambiguous`, creating
/// nothing; below that, `New`.
async fn resolve_asset(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_core::resolution::Resolution>, AppError> {
    let resolution = catalog.resolve_asset(&principal, id).await?;
    Ok(Json(resolution))
}

// ---- operability (Epic 10) ----

/// How to authenticate against this server.
///
/// Unauthenticated by necessity — it is what a client reads *before* it has a
/// token. See [`auth_config`] for why a configured issuer is withheld unless
/// this server would actually verify against it.
///
/// `OIDC_CONSOLE_CLIENT_ID` is deliberately its own variable rather than a
/// reuse of `OIDC_CLIENT_ID`. The latter is a confidential machine-to-machine
/// credential — `demo.sh` exchanges it for a token with a client secret — and a
/// browser needs the *public* single-page-app client. Publishing the M2M client
/// id to every anonymous caller would leak which credential is worth attacking,
/// and it would not work anyway: the two are different applications.
async fn auth_configuration_endpoint() -> Json<AuthConfig> {
    let oidc = oidc_config();
    Json(auth_config(
        auth_mode(signing_secret().is_some(), oidc.is_some()),
        oidc,
        std::env::var("OIDC_CONSOLE_CLIENT_ID")
            .ok()
            .filter(|value| !value.is_empty()),
    ))
}

/// Liveness. Deliberately checks nothing: a dependency outage must not
/// trigger a restart loop across the whole fleet.
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "alive", "version": env!("CARGO_PKG_VERSION") }))
}

/// Readiness. Three-valued, not two.
///
/// A required dependency down is `503`. An *optional* one down is `200
/// degraded`, because forcing that into "not ready" removes a healthy instance
/// from the load balancer and turns a degraded feature into an outage.
async fn ready(State(catalog): State<Catalog>) -> Response {
    let database = catalog.ping().await;
    let secured = signing_secret().is_some() || oidc_config().is_some();
    // Epic 19 Slice C: "a failed consumer makes readiness fail." Required,
    // not advisory — a subscription an operator registered and is relying
    // on for freshness must not silently stop consuming while `/ready`
    // keeps reporting green.
    let (streaming_ok, failed_subscriptions) = streaming::all_healthy();

    let (status, state) = if database.is_ok() && streaming_ok {
        (StatusCode::OK, if secured { "ready" } else { "degraded" })
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unready")
    };

    (
        status,
        Json(json!({
            "status": state,
            "checks": {
                "database": { "required": true, "ok": database.is_ok() },
                // Running open is a legitimate posture for a local demo, but a
                // server that is accidentally open must say so rather than look
                // identical to a secured one.
                "authentication": { "required": false, "ok": secured },
                "streaming": {
                    "required": true,
                    "ok": streaming_ok,
                    "failedSubscriptions": failed_subscriptions,
                },
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod auth_configuration {
    use super::*;

    mod which_mode_a_configuration_selects {
        use super::*;

        #[test]
        fn oidc_alone_is_oidc() {
            assert_eq!(auth_mode(false, true), AuthMode::Oidc);
        }

        #[test]
        fn a_shared_secret_alone_is_the_shared_secret() {
            assert_eq!(auth_mode(true, false), AuthMode::SharedSecret);
        }

        #[test]
        fn neither_is_open() {
            assert_eq!(auth_mode(false, false), AuthMode::Open);
        }

        /// **The one that matters.** A deployment migrating to OIDC that has
        /// not yet removed `GRAPH_OWL_JWT_SECRET` looks entirely healthy —
        /// OIDC is configured, the console signs in against the provider — and
        /// would be quietly verifying against a shared secret that anyone who
        /// ever held it can still mint tokens with.
        ///
        /// Checking the cheaper secret first is the natural implementation and
        /// the wrong one.
        #[test]
        fn oidc_wins_when_both_are_configured_rather_than_the_cheaper_check() {
            assert_eq!(auth_mode(true, true), AuthMode::Oidc);
        }
    }

    /// What the console is told about how to sign in.
    ///
    /// **This exists because the console could not previously ask.** It ran an
    /// OIDC authorization-code flow unconditionally, against a tenant compiled
    /// into the bundle, whatever the server was actually configured to verify.
    /// Against a server in shared-secret mode that produces a sign-in loop with
    /// no error anywhere: the provider authenticates the person perfectly, the
    /// server rejects the resulting token because it is not HS256, and the
    /// console reads the `401` as "signed out" and returns them to the sign-in
    /// screen they just completed. Nothing is broken enough to log.
    mod what_the_console_is_told {
        use super::*;

        fn oidc_details() -> (String, String) {
            (
                "https://tenant.example/".to_string(),
                "https://graph-owl.dev/api".to_string(),
            )
        }

        #[test]
        fn oidc_mode_names_the_issuer_the_console_must_use() {
            let config = auth_config(AuthMode::Oidc, Some(oidc_details()), Some("spa-abc".into()));

            assert_eq!(config.mode, "oidc");
            assert_eq!(config.issuer.as_deref(), Some("https://tenant.example/"));
            assert_eq!(
                config.audience.as_deref(),
                Some("https://graph-owl.dev/api")
            );
            assert_eq!(config.client_id.as_deref(), Some("spa-abc"));
        }

        /// **The test that fixes the reported bug.** A server verifying HS256
        /// must not hand the console an issuer, *even when one is configured in
        /// its environment* — which is exactly the situation `demo.sh --secure`
        /// creates, because a developer's `.env` still holds the OIDC settings
        /// used by every other run.
        ///
        /// Sending the console to that provider would be sending it to earn a
        /// token this server is guaranteed to reject. The absence of an issuer
        /// is the signal that no interactive provider can help here.
        #[test]
        fn shared_secret_mode_names_no_issuer_even_when_one_is_configured() {
            let config = auth_config(
                AuthMode::SharedSecret,
                Some(oidc_details()),
                Some("spa".into()),
            );

            assert_eq!(config.mode, "sharedSecret");
            assert_eq!(config.issuer, None);
            assert_eq!(config.audience, None);
            assert_eq!(config.client_id, None);
        }

        /// Same rule, and the same reason: an open server accepts everyone as
        /// the system principal, so a sign-in would be a ceremony that changes
        /// nothing and can only fail.
        #[test]
        fn open_mode_names_no_issuer_even_when_one_is_configured() {
            let config = auth_config(AuthMode::Open, Some(oidc_details()), Some("spa".into()));

            assert_eq!(config.mode, "open");
            assert_eq!(config.issuer, None);
            assert_eq!(config.audience, None);
            assert_eq!(config.client_id, None);
        }

        /// OIDC with no console client id configured still reports the mode.
        /// The console falls back to its build-time value, which is the setup
        /// every deployment before this endpoint existed was already running.
        #[test]
        fn oidc_without_a_console_client_id_still_reports_oidc() {
            let config = auth_config(AuthMode::Oidc, Some(oidc_details()), None);

            assert_eq!(config.mode, "oidc");
            assert_eq!(config.client_id, None);
            assert_eq!(config.issuer.as_deref(), Some("https://tenant.example/"));
        }

        /// The three names the console switches on. A typo here is invisible
        /// server-side and sends the console down its fallback path forever.
        #[test]
        fn each_mode_has_its_own_wire_name() {
            let names: std::collections::HashSet<_> =
                [AuthMode::Oidc, AuthMode::SharedSecret, AuthMode::Open]
                    .into_iter()
                    .map(|mode| auth_config(mode, None, None).mode)
                    .collect();

            assert_eq!(
                names.len(),
                3,
                "a shared name makes two modes indistinguishable"
            );
        }

        /// The body is read by an unauthenticated browser, so what it omits
        /// matters more than what it carries. `null` fields are dropped rather
        /// than serialized: a `"clientId": null` reads as "configured, empty".
        #[test]
        fn a_mode_needing_no_provider_serializes_to_the_mode_alone() {
            let body = serde_json::to_value(auth_config(
                AuthMode::SharedSecret,
                Some(oidc_details()),
                None,
            ))
            .expect("serializes");

            assert_eq!(body, serde_json::json!({ "mode": "sharedSecret" }));
        }
    }

    mod roles_the_provider_asserts {
        use super::*;
        use serde_json::json;

        fn claims(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
            value.as_object().expect("an object").clone()
        }

        #[test]
        fn a_configured_claim_contributes_its_roles() {
            let extra = claims(json!({ "https://graph-owl.dev/roles": ["steward", "reader"] }));

            assert_eq!(
                roles_from_claims(&extra, "https://graph-owl.dev/roles"),
                vec!["steward", "reader"]
            );
        }

        /// **Off by default, and this is the test that keeps it off.** An
        /// identity provider deciding what this catalog authorizes is a
        /// reasonable arrangement and a terrible default, because it is
        /// invisible to anyone reading the policies.
        #[test]
        fn no_configured_claim_contributes_nothing_however_many_roles_the_token_carries() {
            let extra = claims(json!({
                "roles": ["admin"],
                "permissions": ["admin"],
                "https://graph-owl.dev/roles": ["admin"]
            }));

            assert!(roles_from_claims(&extra, "").is_empty());
        }

        #[test]
        fn a_claim_the_token_does_not_carry_contributes_nothing() {
            let extra = claims(json!({ "sub": "auth0|abc" }));

            assert!(roles_from_claims(&extra, "roles").is_empty());
        }

        /// A provider emitting something other than an array of strings is not
        /// producing roles this understands. Inventing an interpretation would
        /// grant access on the strength of a guess.
        #[test]
        fn a_claim_that_is_not_an_array_of_strings_contributes_nothing() {
            for shape in [
                json!("steward"),
                json!({ "role": "steward" }),
                json!(7),
                json!(null),
            ] {
                let extra = claims(json!({ "roles": shape }));

                assert!(
                    roles_from_claims(&extra, "roles").is_empty(),
                    "{shape} should contribute nothing"
                );
            }
        }

        #[test]
        fn non_string_and_empty_entries_are_skipped_and_the_rest_survive() {
            let extra = claims(json!({ "roles": ["steward", 7, "", null, "reader"] }));

            assert_eq!(
                roles_from_claims(&extra, "roles"),
                vec!["steward", "reader"]
            );
        }

        /// Exact claim name. A prefix match would let `roles_v2` satisfy a
        /// configuration asking for `roles`.
        #[test]
        fn the_claim_name_is_matched_exactly() {
            let extra = claims(json!({ "roles_v2": ["admin"] }));

            assert!(roles_from_claims(&extra, "roles").is_empty());
        }
    }

    mod who_is_an_administrator_before_anyone_can_grant_a_role {
        use super::*;

        #[test]
        fn a_listed_subject_is_an_administrator() {
            assert!(is_bootstrap_admin("auth0|abc", "auth0|abc"));
        }

        #[test]
        fn one_of_several_listed_subjects_matches() {
            assert!(is_bootstrap_admin("auth0|b", "auth0|a,auth0|b,auth0|c"));
        }

        #[test]
        fn surrounding_whitespace_is_not_part_of_a_subject() {
            assert!(is_bootstrap_admin("auth0|b", "auth0|a, auth0|b , auth0|c"));
        }

        #[test]
        fn an_unlisted_subject_is_not_an_administrator() {
            assert!(!is_bootstrap_admin("auth0|intruder", "auth0|a,auth0|b"));
        }

        /// Matching is exact. A prefix or a substring granting admin would mean
        /// `auth0|a` in the list elevates `auth0|attacker`.
        #[test]
        fn a_prefix_or_substring_does_not_match() {
            assert!(!is_bootstrap_admin("auth0|abc", "auth0|ab"));
            assert!(!is_bootstrap_admin("auth0|ab", "auth0|abc"));
        }

        /// The negatives that stop a trailing comma, or an unset variable,
        /// becoming a grant. An empty entry must match nothing at all — not
        /// "the subject whose id is the empty string", and certainly not
        /// everyone.
        #[test]
        fn nothing_configured_elevates_nobody() {
            for configured in ["", " ", ",", ",,", " , "] {
                assert!(
                    !is_bootstrap_admin("auth0|abc", configured),
                    "{configured:?} must not elevate anyone"
                );
            }
        }

        #[test]
        fn an_empty_subject_never_matches_even_an_empty_entry() {
            assert!(!is_bootstrap_admin("", ""));
            assert!(!is_bootstrap_admin("", "auth0|a,,auth0|b"));
        }
    }

    mod what_an_operator_is_warned_about {
        use super::*;

        /// Both configured is not an error — the stronger one is used — but it
        /// is always a mistake: the secret is dead weight at best, and a live
        /// credential somebody believes is in use at worst.
        #[test]
        fn both_configured_is_ambiguous() {
            assert!(is_ambiguous_auth_config(true, true));
        }

        /// And the negatives, so the warning cannot be implemented as "always
        /// warn" — which is the same as never warning.
        #[test]
        fn a_single_configured_mode_is_not_ambiguous() {
            assert!(!is_ambiguous_auth_config(true, false));
            assert!(!is_ambiguous_auth_config(false, true));
            assert!(!is_ambiguous_auth_config(false, false));
        }
    }
}

#[cfg(test)]
mod extension_filter_parsing_tests {
    use super::{percent_decode, split_extension_filters};
    use graph_owl_storage::ExtensionOp;

    #[test]
    fn a_bare_name_is_equality_and_a_suffix_is_a_bound() {
        let (filters, rest) = split_extension_filters(
            "extension.costCenter=CC-1&extension.retentionDays.gte=30&kind=table",
        )
        .ok()
        .expect("a valid query");

        assert_eq!(
            filters,
            vec![
                (
                    "costCenter".to_string(),
                    ExtensionOp::Eq,
                    "CC-1".to_string()
                ),
                (
                    "retentionDays".to_string(),
                    ExtensionOp::Gte,
                    "30".to_string()
                ),
            ]
        );
        assert_eq!(rest, "kind=table", "everything else passes through");
    }

    /// **The property that makes `deny_unknown_fields` survive this feature.**
    /// A flattened serde map would have absorbed `ownr` too; splitting only the
    /// `extension.` prefix leaves the typo for the strict extractor to refuse.
    #[test]
    fn a_parameter_that_is_not_an_extension_filter_is_left_for_the_strict_extractor() {
        let (filters, rest) = split_extension_filters("ownr=alice").ok().expect("parses");

        assert!(filters.is_empty());
        assert_eq!(rest, "ownr=alice");
    }

    /// An unrecognised comparison is somebody meaning `gte`. Reading it as part
    /// of the property name answers with an empty page and no hint.
    #[test]
    fn an_unknown_comparison_suffix_is_refused_rather_than_read_as_a_name() {
        assert!(split_extension_filters("extension.retentionDays.gt=30").is_err());
    }

    /// And the negative: `lte` and `gte` are the two that must be *accepted*,
    /// or the rule above would be indistinguishable from bounds not working.
    #[test]
    fn both_bounds_are_accepted() {
        for (suffix, expected) in [("gte", ExtensionOp::Gte), ("lte", ExtensionOp::Lte)] {
            let (filters, _) = split_extension_filters(&format!("extension.d.{suffix}=1"))
                .ok()
                .expect("a valid bound");
            assert_eq!(filters[0].1, expected);
        }
    }

    #[test]
    fn a_filter_with_no_property_name_is_refused() {
        assert!(split_extension_filters("extension.=CC-1").is_err());
    }

    /// The `extension.*` pairs are peeled off before `serde_urlencoded` runs,
    /// so they carry their own decoding — without it every multi-word value
    /// silently matches nothing.
    #[test]
    fn values_are_percent_and_plus_decoded() {
        let (filters, _) = split_extension_filters("extension.owningTeam=Data%20Platform")
            .ok()
            .expect("parses");
        assert_eq!(filters[0].2, "Data Platform");

        let (filters, _) = split_extension_filters("extension.owningTeam=Data+Platform")
            .ok()
            .expect("parses");
        assert_eq!(filters[0].2, "Data Platform");
    }

    #[test]
    fn percent_decoding_leaves_ordinary_text_alone_and_survives_a_truncated_escape() {
        assert_eq!(percent_decode("CC-1"), "CC-1");
        assert_eq!(
            percent_decode("100%"),
            "100%",
            "a trailing % is not an escape"
        );
        // **The byte where the bounds check actually decides.** A `%` followed
        // by exactly one character is the only input that tells `i + 2 < len`
        // apart from `i + 2 <= len` — and the loose version indexes past the
        // end of the string, which is a panic on a query parameter anyone can
        // send. A trailing `%` alone does not distinguish them.
        assert_eq!(
            percent_decode("%2"),
            "%2",
            "one character is not an escape, and reading two would run off the end"
        );
        assert_eq!(percent_decode("%zz"), "%zz", "not hex, so not an escape");
        assert_eq!(
            percent_decode("%41"),
            "A",
            "and a real escape still decodes"
        );
    }

    #[test]
    fn an_empty_query_yields_nothing() {
        let (filters, rest) = split_extension_filters("").ok().expect("parses");
        assert!(filters.is_empty());
        assert!(rest.is_empty());
    }
}

// ---- Epic 32: agent capabilities ----

/// A grant as a human writes one.
///
/// **The agent is named in the path, not the body.** A body-supplied agent id
/// would let a caller who may write one grant write anybody's, and the path is
/// what a reviewer reads when auditing who changed what.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct AgentGrantRequest {
    /// Capability names from the closed set. **An unrecognised name is a `400`,
    /// never silently dropped**: a grant that quietly ignored half of what it
    /// was given would be a grant nobody wrote, and the caller would believe it
    /// had granted more than it did.
    capabilities: Vec<String>,
    #[serde(default)]
    scope_fqn_prefix: Option<String>,
    #[serde(default)]
    max_writes: Option<u32>,
    #[serde(default)]
    window_seconds: Option<u32>,
    #[serde(default)]
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ValidateBody for AgentGrantRequest {
    /// Validated against the **raw body**, before deserializing.
    ///
    /// That ordering is what lets an unrecognised capability be a `400` naming
    /// the offending entry rather than being silently dropped by serde. A grant
    /// that quietly ignored half of what it was given would be a grant nobody
    /// wrote, and the caller would believe it had granted more than it did.
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut problems = Vec::new();

        match value.get("capabilities") {
            Some(serde_json::Value::Array(names)) => {
                for (index, entry) in names.iter().enumerate() {
                    let Some(name) = entry.as_str() else {
                        problems.push(FieldError::new(
                            &format!("capabilities[{index}]"),
                            FieldErrorCode::Type,
                            "each capability must be a string".to_string(),
                        ));
                        continue;
                    };
                    if !graph_owl_authz::agent::AgentCapability::ALL
                        .iter()
                        .any(|capability| capability.as_str() == name)
                    {
                        problems.push(FieldError::new(
                            &format!("capabilities[{index}]"),
                            FieldErrorCode::Type,
                            format!(
                                "`{name}` is not a capability an agent can hold. \
                                 There is deliberately no delete, grant, policy, \
                                 role or certification capability."
                            ),
                        ));
                    }
                }
            }
            Some(_) => problems.push(FieldError::new(
                "capabilities",
                FieldErrorCode::Type,
                "must be an array of capability names".to_string(),
            )),
            None => problems.push(FieldError::new(
                "capabilities",
                FieldErrorCode::Required,
                "a grant has to say what it grants; to grant nothing, revoke it".to_string(),
            )),
        }

        // A limit of zero refuses every write while looking like a grant, which
        // is a confusing way to say "revoked" — and revoking has its own verb.
        for (field, label) in [("maxWrites", "writes"), ("windowSeconds", "seconds")] {
            if value.get(field).and_then(serde_json::Value::as_u64) == Some(0) {
                problems.push(FieldError::new(
                    field,
                    FieldErrorCode::Type,
                    format!(
                        "must be at least 1 {label}; to stop an agent entirely, \
                         revoke its grant"
                    ),
                ));
            }
        }

        if value
            .get("scopeFqnPrefix")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|prefix| prefix.trim().is_empty())
        {
            problems.push(FieldError::new(
                "scopeFqnPrefix",
                FieldErrorCode::Type,
                "an empty scope admits nothing; omit the field for estate-wide".to_string(),
            ));
        }

        problems
    }
}

/// Grant or replace an agent's capabilities. **Admins only, humans only.**
async fn set_agent_grant(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(agent_id): Path<String>,
    AppJson(body): AppJson<AgentGrantRequest>,
) -> Result<(StatusCode, Json<graph_owl_authz::agent::AgentGrant>), AppError> {
    // The agent has to exist as a principal before it can be trusted with
    // anything — decision 1's distinct-bot-principal rule, enforced rather than
    // assumed. A grant naming nobody would be a row the audit could not resolve.
    let agent = catalog.resolve_principal(&agent_id, &agent_id).await?;

    let capabilities = body
        .capabilities
        .iter()
        .filter_map(|name| {
            graph_owl_authz::agent::AgentCapability::ALL
                .into_iter()
                .find(|capability| capability.as_str() == name)
        })
        .collect();
    let default_limit = graph_owl_authz::agent::RateLimit::default();
    let grant = graph_owl_authz::agent::AgentGrant {
        id: Uuid::new_v4(),
        agent: graph_owl_core::ownership::EntityReference {
            id: agent.id.clone(),
            kind: graph_owl_core::ownership::OwnerKind::User,
            display_name: agent.name.clone(),
            inherited: false,
        },
        capabilities,
        scope: body
            .scope_fqn_prefix
            .clone()
            .map(|fqn_prefix| graph_owl_authz::agent::ScopeRef { fqn_prefix }),
        rate_limit: graph_owl_authz::agent::RateLimit {
            max_writes: body.max_writes.unwrap_or(default_limit.max_writes),
            window_seconds: body.window_seconds.unwrap_or(default_limit.window_seconds),
        },
        expires_at: body.expires_at,
        granted_by: principal.id.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    catalog.set_agent_grant(&principal, &grant).await?;
    Ok((StatusCode::OK, Json(grant)))
}

async fn get_agent_grant(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(agent_id): Path<String>,
) -> Result<Json<graph_owl_authz::agent::AgentGrant>, AppError> {
    // Who may see what an agent is trusted with is itself governance
    // information; a non-admin reading it learns the shape of the estate's
    // automation.
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    catalog
        .agent_grant(&agent_id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn list_agent_grants(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
) -> Result<Json<Vec<graph_owl_authz::agent::AgentGrant>>, AppError> {
    if !principal.is_admin {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.list_agent_grants().await?))
}

async fn revoke_agent_grant(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, AppError> {
    if catalog.revoke_agent_grant(&principal, &agent_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

/// An agent's writes — applied, proposed **and refused**.
async fn get_agent_activity(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(agent_id): Path<String>,
    AppQuery(query): AppQuery<ListQuery>,
) -> Result<Json<Page<graph_owl_authz::agent::AgentActivity>>, AppError> {
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.agent_activity(&agent_id, &page).await?))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ProposalListQuery {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    after: Option<String>,
}

async fn list_proposals(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    AppQuery(query): AppQuery<ProposalListQuery>,
) -> Result<Json<Page<graph_owl_authz::agent::Proposal>>, AppError> {
    use graph_owl_authz::agent::ProposalStatus;
    // An unrecognised status is a `400`, not an empty page: a typo'd filter that
    // silently matched nothing reads as an answer about the data rather than
    // about the request, which is the failure `00d` singles out.
    let status = match query.status.as_deref() {
        None => None,
        Some("open") => Some(ProposalStatus::Open),
        Some("accepted") => Some(ProposalStatus::Accepted),
        Some("rejected") => Some(ProposalStatus::Rejected),
        Some("superseded") => Some(ProposalStatus::Superseded),
        Some(other) => {
            return Err(AppError::Validation(vec![FieldError::new(
                "status",
                FieldErrorCode::Type,
                format!("`{other}` is not a proposal status"),
            )]));
        }
    };
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(
        catalog
            .list_proposals(query.agent_id.as_deref(), status, &page)
            .await?,
    ))
}

async fn get_proposal(
    State(catalog): State<Catalog>,
    Auth(_principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_authz::agent::Proposal>, AppError> {
    catalog
        .get_proposal(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// Accept a proposal: the change lands, **attributed to the agent**, with this
/// caller recorded as approver.
async fn accept_proposal(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<graph_owl_authz::agent::Proposal>, AppError> {
    Ok(Json(catalog.accept_proposal(&principal, id).await?))
}

async fn reject_proposal(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    catalog.reject_proposal(&principal, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
