//! SPARQL federation (`SERVICE`) — Epic 101.
//!
//! `spargebra` already parses `SERVICE` and `spareval` already takes a
//! [`spareval::DefaultServiceHandler`] — there is no executor to write here.
//! What this module supplies is that one trait implementation, and
//! everything in it is *what the implementation must enforce*, per the
//! plan's own three named dangers: an unbounded outbound call, a
//! data-exfiltration path wearing a query's clothes, and a remote answer
//! with no provenance the caller can assess.

use spareval::{DefaultServiceHandler, QuerySolutionIter};
use spargebra::algebra::GraphPattern;

/// Which endpoints a `SERVICE` clause may reach.
///
/// **Administrative configuration, never the query.** A `SERVICE
/// <https://anywhere>` naming an arbitrary URL is an outbound request
/// composed by whoever wrote the query; the allow-list is what keeps that
/// request bounded to endpoints a deployment has deliberately named.
///
/// Empty by default — the safe direction, matching every other
/// off-by-default capability in this facade (`auto_merge_enabled`,
/// `store_query_text`): a deployment that never configures federation gets
/// none, rather than an unbounded one by omission.
#[derive(Debug, Clone, Default)]
pub struct FederationAllowList {
    endpoints: Vec<String>,
}

impl FederationAllowList {
    /// An allow-list naming exactly these endpoints.
    #[must_use]
    pub fn new(endpoints: Vec<String>) -> Self {
        Self { endpoints }
    }

    /// Whether this endpoint may be reached.
    #[must_use]
    pub fn allows(&self, endpoint: &str) -> bool {
        self.endpoints.iter().any(|allowed| allowed == endpoint)
    }

    /// Every allow-listed endpoint, for an error message or an admin listing.
    #[must_use]
    pub fn endpoints(&self) -> &[String] {
        &self.endpoints
    }
}

/// Why a `SERVICE` clause could not be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationError {
    /// The endpoint is not on the deployment's allow-list.
    EndpointNotAllowed {
        /// The endpoint the query named.
        endpoint: String,
        /// The full allow-list, so the message is actionable rather than
        /// just naming what failed.
        allowed: Vec<String>,
    },
    /// Allow-listed, but the real HTTP join is not built yet — Slice B's
    /// job. A distinct variant so this placeholder can never be confused
    /// with a genuine allow-list refusal by anything reading the message.
    NotYetImplemented {
        /// The endpoint that was correctly allowed.
        endpoint: String,
    },
}

impl std::fmt::Display for FederationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EndpointNotAllowed { endpoint, allowed } => {
                let list = if allowed.is_empty() {
                    "none configured".to_string()
                } else {
                    allowed.join(", ")
                };
                write!(
                    f,
                    "SERVICE endpoint `{endpoint}` is not on the federation allow-list (allowed: {list})"
                )
            }
            Self::NotYetImplemented { endpoint } => {
                write!(
                    f,
                    "SERVICE endpoint `{endpoint}` is allow-listed, but federated queries are not yet implemented"
                )
            }
        }
    }
}

impl std::error::Error for FederationError {}

/// Answers `SERVICE` clauses against the allow-listed endpoints — Epic 101.
pub struct FederationServiceHandler {
    allow_list: FederationAllowList,
}

impl FederationServiceHandler {
    /// Answers `SERVICE` clauses against this allow-list.
    #[must_use]
    pub fn new(allow_list: FederationAllowList) -> Self {
        Self { allow_list }
    }
}

impl DefaultServiceHandler for FederationServiceHandler {
    type Error = FederationError;

    fn handle(
        &self,
        service_name: &oxrdf::NamedNode,
        _pattern: &GraphPattern,
        _base_iri: Option<&oxiri::Iri<String>>,
    ) -> Result<QuerySolutionIter<'static>, Self::Error> {
        let endpoint = service_name.as_str();
        if !self.allow_list.allows(endpoint) {
            return Err(FederationError::EndpointNotAllowed {
                endpoint: endpoint.to_string(),
                allowed: self.allow_list.endpoints().to_vec(),
            });
        }
        // Slice B fills in the real HTTP join. An allow-listed endpoint
        // reaching here — rather than being refused for the allow-list
        // reason — is this slice's own acceptance criterion.
        Err(FederationError::NotYetImplemented {
            endpoint: endpoint.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_allow_list_allows_nothing() {
        let list = FederationAllowList::default();
        assert!(!list.allows("https://dbpedia.org/sparql"));
    }

    #[test]
    fn an_allow_list_allows_only_what_it_names() {
        let list = FederationAllowList::new(vec!["https://dbpedia.org/sparql".to_string()]);
        assert!(list.allows("https://dbpedia.org/sparql"));
        assert!(!list.allows("https://not-allowed.example/sparql"));
    }

    #[test]
    fn the_refusal_names_the_endpoint_and_the_allow_list() {
        let error = FederationError::EndpointNotAllowed {
            endpoint: "https://not-allowed.example/sparql".to_string(),
            allowed: vec!["https://dbpedia.org/sparql".to_string()],
        };
        let message = error.to_string();
        assert!(
            message.contains("https://not-allowed.example/sparql"),
            "{message}"
        );
        assert!(message.contains("https://dbpedia.org/sparql"), "{message}");
    }

    /// An empty allow-list is not the same message as one that names an
    /// unrelated endpoint — "none configured" and "was not walgreens.example"
    /// point an operator at different fixes.
    #[test]
    fn an_empty_allow_list_says_so_rather_than_an_empty_list() {
        let error = FederationError::EndpointNotAllowed {
            endpoint: "https://dbpedia.org/sparql".to_string(),
            allowed: vec![],
        };
        assert!(error.to_string().contains("none configured"), "{error}");
    }
}
