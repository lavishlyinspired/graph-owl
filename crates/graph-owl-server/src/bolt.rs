//! Adapts `graph-owl-bolt`'s ports onto `Catalog` — Epic 7d.
//!
//! Nothing here re-implements authentication, authorization, or query
//! execution. Both adapters call the same functions the HTTP surface calls,
//! which is the whole point of the port existing: a divergent identity or
//! authorization path is the one nobody audits.

use std::sync::Arc;

use graph_owl_api::{Catalog, CatalogError, CypherValue, SparqlBudget};
use graph_owl_bolt::{
    AuthError, Authenticator, BoltRow, Credentials, QueryEngine, QueryError, RecordReceiver,
    RecordValue, RunOutcome,
};
use graph_owl_core::Principal;
use tokio_stream::StreamExt as _;

use crate::authenticate_bearer_token;

/// Authenticates a Bolt `HELLO` through the identical bearer-token
/// verification the HTTP `Auth` extractor uses.
///
/// **Only `scheme: "bearer"` resolves to anything** — this catalog has no
/// password store, so `"basic"` and `"kerberos"` are refused by name rather
/// than silently falling through to open mode, which would let a client
/// bypass a configured JWKS/shared-secret deployment by asking for a scheme
/// nobody implements.
pub struct CatalogAuthenticator {
    catalog: Catalog,
    jwks: Option<Arc<crate::jwks::JwksClient>>,
}

impl CatalogAuthenticator {
    #[must_use]
    pub fn new(catalog: Catalog, jwks: Option<Arc<crate::jwks::JwksClient>>) -> Self {
        Self { catalog, jwks }
    }
}

#[async_trait::async_trait]
impl Authenticator for CatalogAuthenticator {
    async fn authenticate(&self, credentials: &Credentials) -> Result<Principal, AuthError> {
        let token_required = self.jwks.is_some() || crate::signing_secret().is_some();

        if !token_required {
            // Open mode: identical to HTTP's — every connection is the
            // system principal, regardless of what scheme was offered.
            return Ok(Principal::system());
        }

        if credentials.scheme != "bearer" {
            return Err(AuthError::new(format!(
                "this server has no `{}` authentication scheme; use `bearer`",
                credentials.scheme
            )));
        }
        let Some(token) = credentials.credentials.as_deref() else {
            return Err(AuthError::new("the `bearer` scheme requires `credentials`"));
        };

        authenticate_bearer_token(token, self.jwks.as_deref(), &self.catalog)
            .await
            .map_err(|err| AuthError::new(format!("{err:?}")))
    }
}

/// Runs `RUN`'s query through [`Catalog::cypher_stream`] — Epic 7b's Cypher
/// module, no second execution path — and republishes its rows as
/// [`graph_owl_bolt::BoltRow`]s.
pub struct CatalogQueryEngine {
    catalog: Catalog,
    budget: SparqlBudget,
    fetch_batch_size: usize,
}

impl CatalogQueryEngine {
    #[must_use]
    pub fn new(catalog: Catalog, budget: SparqlBudget, fetch_batch_size: usize) -> Self {
        Self {
            catalog,
            budget,
            fetch_batch_size,
        }
    }
}

fn bolt_row_of(row: graph_owl_api::CypherRow) -> BoltRow {
    BoltRow {
        values: row
            .values
            .into_iter()
            .map(|(name, value)| (name, record_value_of(value)))
            .collect(),
        lossy: row.lossy,
    }
}

fn record_value_of(value: CypherValue) -> RecordValue {
    match value {
        CypherValue::Null => RecordValue::Null,
        CypherValue::Boolean(b) => RecordValue::Boolean(b),
        CypherValue::Integer(n) => RecordValue::Integer(n),
        CypherValue::Float(f) => RecordValue::Float(f),
        CypherValue::String(s) => RecordValue::String(s),
        CypherValue::Node(node) => RecordValue::Node(node),
        CypherValue::Relationship(edge) => RecordValue::Relationship(edge),
    }
}

fn query_error_of(err: CatalogError) -> QueryError {
    match err {
        CatalogError::Validation(errors) => {
            let message = errors
                .into_iter()
                .map(|e| e.detail)
                .collect::<Vec<_>>()
                .join("; ");
            QueryError::Refused(message)
        }
        other => QueryError::Storage(format!("{other:?}")),
    }
}

#[async_trait::async_trait]
impl QueryEngine for CatalogQueryEngine {
    async fn run(
        &self,
        principal: &Principal,
        query: &str,
    ) -> Result<(RunOutcome, RecordReceiver), QueryError> {
        let stream = self
            .catalog
            .cypher_stream(principal, query, self.budget, self.fetch_batch_size)
            .await
            .map_err(query_error_of)?;

        let outcome = RunOutcome {
            fields: stream.fields,
        };
        let rows = tokio_stream::wrappers::ReceiverStream::new(stream.rows)
            .map(|result| result.map(bolt_row_of).map_err(query_error_of));
        Ok((outcome, RecordReceiver::new(rows)))
    }
}

/// Why a `BOLT_*` environment variable could not be read as a limit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{variable}={value:?} is invalid: {reason}")]
pub struct BoltLimitsError {
    pub variable: &'static str,
    pub value: String,
    pub reason: &'static str,
}

/// Read [`graph_owl_bolt::BoltLimits`] from the environment, defaulting any
/// unset variable to [`graph_owl_bolt::BoltLimits::default`]'s value.
///
/// Takes a reader rather than calling `std::env::var` directly, the same
/// reason `admission::Admission::from_env` and `budget::Budget::from_env`
/// do: testable without a process-global mutation another test would race.
///
/// # Errors
///
/// [`BoltLimitsError`] naming the variable when a value is not a positive
/// integer — zero is refused for the same reason `admission` refuses it: a
/// limit of zero is indistinguishable from the service being permanently
/// broken, and is far more often a typo than an intent.
pub fn bolt_limits_from_env(
    read: impl Fn(&str) -> Option<String>,
) -> Result<graph_owl_bolt::BoltLimits, BoltLimitsError> {
    let default = graph_owl_bolt::BoltLimits::default();

    let positive = |variable: &'static str, fallback: usize| -> Result<usize, BoltLimitsError> {
        match read(variable) {
            None => Ok(fallback),
            Some(raw) => {
                let parsed: usize = raw.trim().parse().map_err(|_| BoltLimitsError {
                    variable,
                    value: raw.clone(),
                    reason: "expected a positive whole number",
                })?;
                if parsed == 0 {
                    return Err(BoltLimitsError {
                        variable,
                        value: raw,
                        reason: "zero would refuse or hang every connection",
                    });
                }
                Ok(parsed)
            }
        }
    };

    Ok(graph_owl_bolt::BoltLimits {
        max_connections: positive("BOLT_MAX_CONNECTIONS", default.max_connections)?,
        max_message_bytes: positive("BOLT_MAX_MESSAGE_BYTES", default.max_message_bytes)?,
        query_timeout: std::time::Duration::from_secs(positive(
            "BOLT_QUERY_TIMEOUT_SECS",
            default.query_timeout.as_secs() as usize,
        )? as u64),
        fetch_batch_size: positive("BOLT_FETCH_BATCH_SIZE", default.fetch_batch_size)?,
    })
}

/// The one call site `main.rs` needs: build a [`graph_owl_bolt::BoltServer`]
/// wired to `catalog`, resolving OIDC configuration the identical way
/// [`crate::app_with_admission`] does for HTTP.
///
/// **Builds its own [`crate::jwks::JwksClient`]**, separate from the one
/// HTTP's router layers in — both verify against the same issuer and
/// audience with the identical verification logic
/// ([`crate::authenticate_bearer_token`]), so this costs an independent key
/// cache and refetch schedule, not a divergent trust decision. Sharing one
/// client would need threading it through `app_with_admission`'s signature,
/// which is exercised by a large existing test surface — a real refactor,
/// not this slice's.
#[must_use]
pub fn build_server(
    catalog: Catalog,
    limits: graph_owl_bolt::BoltLimits,
    budget: SparqlBudget,
) -> Arc<graph_owl_bolt::BoltServer> {
    let jwks = crate::oidc_config()
        .map(|(issuer, audience)| Arc::new(crate::jwks::JwksClient::new(issuer, audience)));
    let auth = Arc::new(CatalogAuthenticator::new(catalog.clone(), jwks));
    let query = Arc::new(CatalogQueryEngine::new(
        catalog,
        budget,
        limits.fetch_batch_size,
    ));
    Arc::new(graph_owl_bolt::BoltServer::new(auth, query, limits))
}
