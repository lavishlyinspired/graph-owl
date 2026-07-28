pub mod jwks;
pub mod observability;

use axum::{
    Json, Router,
    extract::{
        FromRequest, FromRequestParts, Path, Query, Request, State, rejection::JsonRejection,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use std::sync::Arc;

use graph_owl_api::SparqlBudget;
use graph_owl_api::{
    Catalog, CatalogError, CreateRelationship, CreateTable, UpsertAsset,
    validation::{FieldError, FieldErrorCode, ValidateBody, require_non_empty_string},
};
use graph_owl_connectors::{Connector, DeletionPlan, RunScope, postgres::PostgresConnector};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Principal, Relationship, Table, TableUpdate,
    page::{Page, PageRequest, PageRequestError},
};
use graph_owl_storage::{ConflictKind, StorageError};
use serde::de::DeserializeOwned;
use serde_json::json;
use uuid::Uuid;

pub fn app(catalog: Catalog) -> Router {
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
        // Unauthenticated by design: an orchestrator's probe must not depend
        // on the identity provider being reachable.
        .route("/health", get(health))
        .route("/ready", get(ready))
        // Unauthenticated for the same reason: a scrape must not depend on the
        // identity provider, or an auth outage blinds the monitoring that would
        // have shown it.
        .route("/metrics", get(observability::metrics_endpoint))
        .route(
            "/assets/{id}",
            get(get_asset).patch(update_asset).delete(delete_asset),
        )
        .route("/assets/{id}/versions", get(asset_versions))
        .route("/assets/{id}/restore", post(restore_asset))
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

    router
        // `layer`, not `route_layer`: this must run after routing so
        // `MatchedPath` is in the extensions and the metric label is the route
        // template rather than the concrete path.
        .layer(axum::middleware::from_fn(observability::observe))
        // Mounted LAST so the SPA fallback cannot swallow an unknown API path.
        // A fallback registered first turns every mistyped endpoint into a 200
        // text/html and the client sees a blank page instead of an error.
        .merge(graph_owl_ui::router())
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

    // Applied after resolution, never written back. See `is_bootstrap_admin`.
    if is_bootstrap_admin(
        &claims.sub,
        &std::env::var("GRAPH_OWL_ADMIN_SUBJECTS").unwrap_or_default(),
    ) {
        principal.is_admin = true;
    }
    Ok(Auth(principal))
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
            return verify_jwks(token, jwks, &catalog).await;
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
            return catalog
                .resolve_principal(&claims.sub, &name)
                .await
                .map(Auth)
                .map_err(AppError::from);
        }

        // No secret and no OIDC — open mode.
        Ok(Auth(Principal::system()))
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
            AppError::Internal(_) => "internal-error",
            AppError::NotFound => "not-found",
            AppError::PreconditionFailed { .. } => "version-conflict",
            AppError::Unauthenticated => "unauthenticated",
            AppError::Forbidden => "forbidden",
            AppError::TokenExpired => "token-expired",
            AppError::TokenInvalid(_) => "token-invalid",
            AppError::IllegalRelationship { .. } => "illegal-relationship",
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
            AppError::Internal(_) => "Internal server error",
            AppError::NotFound => "Resource not found",
            AppError::PreconditionFailed { .. } => "Version precondition failed",
            AppError::Unauthenticated => "Authentication required",
            AppError::Forbidden => "Forbidden",
            AppError::TokenExpired => "Token expired",
            AppError::TokenInvalid(_) => "Token invalid",
            AppError::IllegalRelationship { .. } => "Illegal relationship",
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
        response
    }
}

// ---- asset hierarchy (Epic 2) ----

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetListQuery {
    kind: Option<String>,
    limit: Option<usize>,
    after: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetSearchQuery {
    q: String,
    kind: Option<String>,
    limit: Option<usize>,
    after: Option<String>,
}

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
    AppQuery(query): AppQuery<AssetListQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(
        catalog.list_assets_for(&principal, kind, &page).await?,
    ))
}

async fn search_assets(
    State(catalog): State<Catalog>,
    Auth(principal): Auth,
    AppQuery(query): AppQuery<AssetSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    let page_result = catalog
        .search_assets_for(&principal, &query.q, kind, &page)
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

    Ok(Json(json!({
        "data": page_result.data,
        "paging": page_result.paging,
        "facets": {
            "kind": by_kind.iter().map(|(k, n)| json!({ "value": k, "count": n })).collect::<Vec<_>>(),
            "schema": by_schema.iter().map(|(k, n)| json!({ "value": k, "count": n })).collect::<Vec<_>>(),
        }
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
    })))
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
    let mut created = 0;
    let mut failures: Vec<serde_json::Value> = Vec::new();
    // What the source reported *and* the catalog accepted. Deletion is decided
    // against this, never against the fetched list: an asset that failed to
    // ingest is a write problem, and treating it as absent would convert a
    // transient error into a tombstone.
    let mut ingested: std::collections::HashSet<String> = std::collections::HashSet::new();
    for record in records {
        let path = record.path.join(".");
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

    Ok(Json(json!({
        "connector": connector.type_name(),
        "serviceName": payload.service_name,
        "created": created,
        "failed": failures.len(),
        "failures": failures,
        "deletions": deletions,
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

// ---- operability (Epic 10) ----

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

    let (status, state) = if database.is_ok() {
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
