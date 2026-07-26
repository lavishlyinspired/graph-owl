use axum::{
    Json, Router,
    extract::{FromRequest, Path, Request, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use graph_owl_api::{Catalog, CreateTable};
use graph_owl_core::{Table, TableUpdate};
use graph_owl_storage::StorageError;
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
        .with_state(catalog)
}

async fn create_table(
    State(catalog): State<Catalog>,
    AppJson(payload): AppJson<CreateTable>,
) -> Result<(StatusCode, Json<Table>), AppError> {
    let table = catalog.create_table(payload).await?;
    Ok((StatusCode::CREATED, Json(table)))
}

async fn list_tables(State(catalog): State<Catalog>) -> Result<Json<Vec<Table>>, AppError> {
    let tables = catalog.list_tables().await?;
    Ok(Json(tables))
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
    Path(id): Path<Uuid>,
    AppJson(update): AppJson<TableUpdate>,
) -> Result<Json<Table>, AppError> {
    catalog
        .update_table(id, update)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn delete_table(
    State(catalog): State<Catalog>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if catalog.delete_table(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

/// Wraps [`Json`] to return `400 Bad Request` for any malformed or
/// semantically invalid body, rather than axum's default `422` for data errors.
struct AppJson<T>(T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection: JsonRejection| AppError::BadRequest(rejection.body_text()))?;
        Ok(AppJson(value))
    }
}

enum AppError {
    BadRequest(String),
    Conflict(String),
    Internal(String),
    NotFound,
}

impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Conflict(fqn) => AppError::Conflict(fqn),
            StorageError::Unexpected(message) => AppError::Internal(message),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::Conflict(fqn) => (
                StatusCode::CONFLICT,
                format!("table with fully_qualified_name '{fqn}' already exists"),
            ),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
            AppError::NotFound => (StatusCode::NOT_FOUND, "table not found".to_string()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
