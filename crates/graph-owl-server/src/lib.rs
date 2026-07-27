use axum::{
    Json, Router,
    extract::{
        FromRequest, FromRequestParts, Path, Query, Request, State, rejection::JsonRejection,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use graph_owl_api::{
    Catalog, CatalogError, CreateRelationship, CreateTable, UpsertAsset,
    validation::{FieldError, FieldErrorCode, ValidateBody, require_non_empty_string},
};
use graph_owl_connectors::{Connector, RunScope, postgres::PostgresConnector};
use graph_owl_core::{
    Asset, AssetKind, Principal, Relationship, Table, TableUpdate,
    page::{Page, PageRequest, PageRequestError},
};
use graph_owl_storage::{ConflictKind, StorageError};
use serde::de::DeserializeOwned;
use serde_json::json;
use uuid::Uuid;

pub fn app(catalog: Catalog) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
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
        .route("/connectors/postgres/runs", post(run_postgres_connector))
        .route("/assets/{id}", get(get_asset))
        .route("/assets/{id}/children", get(list_asset_children))
        .route("/assets/{id}/ancestors", get(asset_ancestors))
        .with_state(catalog)
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

impl<S> FromRequestParts<S> for Auth
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(
        _parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
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
        AssetKind::parse(value).map_err(|()| {
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
    AppQuery(query): AppQuery<AssetListQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.list_assets(kind, &page).await?))
}

async fn search_assets(
    State(catalog): State<Catalog>,
    AppQuery(query): AppQuery<AssetSearchQuery>,
) -> Result<Json<Page<Asset>>, AppError> {
    let kind = parse_kind(query.kind.as_deref())?;
    let page = PageRequest::new(query.limit, query.after.as_deref())?;
    Ok(Json(catalog.search_assets(&query.q, kind, &page).await?))
}

async fn get_asset(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Asset>, AppError> {
    catalog
        .get_asset(id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn list_roots(State(catalog): State<Catalog>) -> Result<Json<Vec<Asset>>, AppError> {
    Ok(Json(catalog.list_children(None).await?))
}

async fn list_asset_children(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Asset>>, AppError> {
    // A missing parent is a 404, not an empty list: "this has no children" and
    // "this does not exist" are different answers and a client acts on them
    // differently.
    if catalog.get_asset(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.list_children(Some(id)).await?))
}

async fn asset_ancestors(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Asset>>, AppError> {
    if catalog.get_asset(id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(Json(catalog.ancestors_of(id).await?))
}

async fn asset_stats(State(catalog): State<Catalog>) -> Result<Json<serde_json::Value>, AppError> {
    let counts = catalog.count_assets_by_kind().await?;
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
            Ok(_) => created += 1,
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

    Ok(Json(json!({
        "connector": connector.type_name(),
        "serviceName": payload.service_name,
        "created": created,
        "failed": failures.len(),
        "failures": failures,
    })))
}
