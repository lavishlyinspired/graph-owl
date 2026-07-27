//! Source connectors: the `Connector` trait, the run machinery, and the
//! Postgres reference implementation.
//!
//! **Status**: Epic 15, first vertical slice. Connectors beyond this one are
//! Python, out of process, pushing through the ingestion API — see
//! `plans/00j-language-boundaries.md`. What stays here is the governance part:
//! the trait, run scoping, and the ordering guarantee.

use async_trait::async_trait;
use graph_owl_core::AssetKind;
use serde::{Deserialize, Serialize};

pub mod postgres;

/// One record yielded by a source, before it becomes a catalog asset.
///
/// Carries the *path* rather than a parent id, because a source knows where a
/// thing sits in its own hierarchy and knows nothing about catalog ids. The
/// sink resolves the path to a parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub kind: AssetKind,
    /// Root-to-leaf, e.g. `["warehouse", "sales", "public", "orders"]`.
    pub path: Vec<String>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

impl SourceRecord {
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunScope {
    /// Schemas to include. Empty means all non-system schemas.
    pub include_schemas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub created: usize,
    pub failed: usize,
}

#[derive(Debug)]
pub enum ConnectorError {
    Connection(String),
    Introspection(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::Connection(message) => write!(f, "connection failed: {message}"),
            ConnectorError::Introspection(message) => write!(f, "introspection failed: {message}"),
        }
    }
}

#[async_trait]
pub trait Connector: Send + Sync {
    /// Stable type name, used in configuration and run history.
    fn type_name(&self) -> &'static str;

    /// Fails fast with a typed error rather than surfacing at first fetch.
    async fn test_connection(&self) -> Result<(), ConnectorError>;

    /// Yields records **parents before children**.
    ///
    /// The ordering is the connector's contract, not the sink's problem: a sink
    /// that had to buffer and topologically sort would need the whole source in
    /// memory before writing anything.
    async fn fetch(&self, scope: &RunScope) -> Result<Vec<SourceRecord>, ConnectorError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_name_is_the_last_path_segment() {
        let record = SourceRecord {
            kind: AssetKind::Table,
            path: vec![
                "warehouse".into(),
                "sales".into(),
                "public".into(),
                "orders".into(),
            ],
            description: None,
            properties: None,
        };
        assert_eq!(record.name(), "orders");
    }

    #[test]
    fn an_empty_path_yields_an_empty_name_rather_than_panicking() {
        let record = SourceRecord {
            kind: AssetKind::Service,
            path: Vec::new(),
            description: None,
            properties: None,
        };
        assert_eq!(record.name(), "");
    }
}
