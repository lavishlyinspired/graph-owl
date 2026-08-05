//! SPARQL federation (`SERVICE`) — Epic 101.
//!
//! `spargebra` already parses `SERVICE` and `spareval` already takes a
//! [`spareval::DefaultServiceHandler`] — there is no executor to write here.
//! What this module supplies is that one trait implementation, and
//! everything in it is *what the implementation must enforce*, per the
//! plan's own three named dangers: an unbounded outbound call, a
//! data-exfiltration path wearing a query's clothes, and a remote answer
//! with no provenance the caller can assess.
//!
//! **Runs on a blocking thread.** [`spareval::DefaultServiceHandler::handle`]
//! is a synchronous trait method, and `Catalog::execute_algebra` already
//! evaluates the whole query inside [`tokio::task::spawn_blocking`] for
//! exactly this reason — [`tokio::runtime::Handle::block_on`] is only safe
//! to call from a thread that is not itself an async worker, which a
//! `spawn_blocking` thread is by construction. This module never spawns its
//! own runtime or client; it borrows the ambient one.

use std::time::Duration;

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

/// Parses `GRAPH_OWL_FEDERATION_ENDPOINTS` — a comma-separated list of
/// allow-listed endpoint URLs — into the form
/// [`Catalog::with_federation_endpoints`](crate::Catalog::with_federation_endpoints)
/// takes. `None` or an empty string allow-lists nothing, matching
/// [`FederationAllowList`]'s own off-by-default posture: a deployment that
/// never sets the variable gets no federation, rather than an unbounded one
/// by omission. Blank entries (`"a,,b"`, a trailing comma) are dropped
/// rather than becoming an endpoint nobody intended to name — there is
/// nothing else meaningfully invalid about a comma-separated list of
/// strings, so unlike `Budget`/`Admission` this has no error case to report.
#[must_use]
pub fn endpoints_from_env(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// How long a single `SERVICE` call may take before it is abandoned.
///
/// **Ten seconds.** A well-behaved public SPARQL endpoint answers a bounded
/// pattern in well under one second; ten is an order of magnitude of slack
/// for a remote host under load, while still failing within the same order
/// of magnitude as the rest of this project's bounded operations rather
/// than holding a query open indefinitely — the plan's own first named
/// danger. Configurable per deployment via
/// [`Catalog::with_federation_timeout`], because the right number depends
/// on which endpoints an operator actually allow-lists.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// The HTTP call itself failed — refused, timed out, or answered with
    /// an error status.
    Request {
        /// The endpoint that could not be reached.
        endpoint: String,
        /// The underlying HTTP client's own message.
        detail: String,
    },
    /// The endpoint answered, but not with something this reads as SPARQL
    /// results — the wrong content, the wrong result form (a boolean where
    /// solutions were expected), or a body that does not parse.
    Parse {
        /// The endpoint whose response could not be read.
        endpoint: String,
        /// What was wrong with it.
        detail: String,
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
            Self::Request { endpoint, detail } => {
                write!(
                    f,
                    "SERVICE endpoint `{endpoint}` could not be reached: {detail}"
                )
            }
            Self::Parse { endpoint, detail } => {
                write!(
                    f,
                    "SERVICE endpoint `{endpoint}` did not answer with usable SPARQL results: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for FederationError {}

/// Answers `SERVICE` clauses against the allow-listed endpoints — Epic 101.
///
/// **Records every call it makes**, successful or not, in
/// [`Self::activity`]. `spareval` itself decides whether a failed call
/// fails the whole query or is swallowed for `SERVICE SILENT` (see
/// [`GraphPattern::Service`]'s `silent` field) — `handle` is never told
/// which — so this handler cannot label an entry "silenced" at the point it
/// records it. The caller can, though: if evaluation as a whole succeeds
/// despite a recorded failure, that failure can only have been silenced,
/// because a non-silent one would have propagated and prevented evaluation
/// from succeeding at all. See `Catalog::execute_algebra`.
pub struct FederationServiceHandler {
    allow_list: FederationAllowList,
    client: reqwest::Client,
    timeout: Duration,
    activity: std::sync::Mutex<Vec<(String, bool)>>,
}

impl FederationServiceHandler {
    /// Answers `SERVICE` clauses against this allow-list, reaching allowed
    /// endpoints through `client` and bounding each call by `timeout`.
    #[must_use]
    pub fn new(
        allow_list: FederationAllowList,
        client: reqwest::Client,
        timeout: Duration,
    ) -> Self {
        Self {
            allow_list,
            client,
            timeout,
            activity: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Every endpoint this handler was asked to reach during evaluation,
    /// paired with whether that specific call succeeded. An endpoint called
    /// more than once (once per outer binding a `SERVICE` clause joins
    /// against) may appear more than once, and with different outcomes.
    ///
    /// Poisoning (a panic while the lock was held elsewhere) is read as "no
    /// activity recorded" rather than propagated — losing the endpoint
    /// attribution on an already-exceptional path is preferable to a second,
    /// unrelated panic while reporting the first.
    #[must_use]
    pub fn activity(&self) -> Vec<(String, bool)> {
        self.activity
            .lock()
            .map(|activity| activity.clone())
            .unwrap_or_default()
    }

    fn record(&self, endpoint: &str, succeeded: bool) {
        if let Ok(mut activity) = self.activity.lock() {
            activity.push((endpoint.to_string(), succeeded));
        }
    }
}

impl DefaultServiceHandler for FederationServiceHandler {
    type Error = FederationError;

    fn handle(
        &self,
        service_name: &oxrdf::NamedNode,
        pattern: &GraphPattern,
        _base_iri: Option<&oxiri::Iri<String>>,
    ) -> Result<QuerySolutionIter<'static>, Self::Error> {
        let endpoint = service_name.as_str();
        if !self.allow_list.allows(endpoint) {
            self.record(endpoint, false);
            return Err(FederationError::EndpointNotAllowed {
                endpoint: endpoint.to_string(),
                allowed: self.allow_list.endpoints().to_vec(),
            });
        }

        // `SELECT *` rather than naming the pattern's own variables: the
        // pattern text is everything inside the `SERVICE { ... }` block,
        // and projecting every variable it binds is what a caller joining
        // against it needs — narrowing the projection here would drop a
        // variable the outer query still expects to see.
        let query_text = format!("SELECT * WHERE {{ {pattern} }}");

        // **Query via POST directly**, one of the three submission methods
        // the SPARQL 1.1 Protocol spec defines
        // (https://www.w3.org/TR/sparql11-protocol/#query-via-post-direct):
        // "Clients must set the content type header ... to
        // `application/sparql-query`" and place the unencoded query string
        // directly in the body. Chosen over query-via-GET (a `SERVICE`
        // clause's own pattern is caller-written and unbounded in length —
        // a query embedded in a URL can exceed a proxy's URL-length limit
        // where the same text in a body cannot) and over the spec's
        // URL-encoded-POST form (no parameters other than the query itself
        // are ever sent, so encoding it as a form field adds nothing).
        //
        // `Accept` names both JSON and XML, per the spec's own content
        // negotiation guidance ("clients should use HTTP content
        // negotiation...") — an endpoint is free to answer in whichever it
        // prefers, and the response is parsed according to what it actually
        // sent back, not according to what was requested.
        //
        // Safe because `handle` only ever runs inside the `spawn_blocking`
        // frame `Catalog::execute_algebra` wraps evaluation in — see the
        // module doc comment. `Handle::current()` panics if called from a
        // normal async task; it does not panic here.
        let runtime = tokio::runtime::Handle::current();
        let response = runtime
            .block_on(async {
                self.client
                    .post(endpoint)
                    .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
                    .header(
                        reqwest::header::ACCEPT,
                        "application/sparql-results+json, application/sparql-results+xml",
                    )
                    .timeout(self.timeout)
                    .body(query_text)
                    .send()
                    .await?
                    .error_for_status()
            })
            .map_err(|e| FederationError::Request {
                endpoint: endpoint.to_string(),
                detail: e.to_string(),
            });
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.record(endpoint, false);
                return Err(error);
            }
        };

        // The format actually answered, not the format asked for — an
        // endpoint that ignores `Accept` (or defaults to XML) must still be
        // read correctly rather than fail the JSON parser on XML bytes.
        let format = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(sparesults::QueryResultsFormat::from_media_type);
        let format = match format {
            Some(format) => format,
            None => {
                self.record(endpoint, false);
                return Err(FederationError::Parse {
                    endpoint: endpoint.to_string(),
                    detail: "the endpoint's response did not name a recognised SPARQL results \
                             content type"
                        .to_string(),
                });
            }
        };

        let bytes = runtime
            .block_on(response.bytes())
            .map_err(|e| FederationError::Request {
                endpoint: endpoint.to_string(),
                detail: e.to_string(),
            });
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                self.record(endpoint, false);
                return Err(error);
            }
        };

        let parser = sparesults::QueryResultsParser::from_format(format);
        let output =
            parser
                .for_reader(std::io::Cursor::new(bytes))
                .map_err(|e| FederationError::Parse {
                    endpoint: endpoint.to_string(),
                    detail: e.to_string(),
                });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                self.record(endpoint, false);
                return Err(error);
            }
        };

        match output {
            sparesults::ReaderQueryResultsParserOutput::Solutions(solutions) => {
                self.record(endpoint, true);
                Ok(solutions.into())
            }
            sparesults::ReaderQueryResultsParserOutput::Boolean(_) => {
                self.record(endpoint, false);
                Err(FederationError::Parse {
                    endpoint: endpoint.to_string(),
                    detail: "the endpoint answered a boolean result where solutions were expected"
                        .to_string(),
                })
            }
        }
    }
}

/// Delegates every call to a shared [`FederationServiceHandler`], so its
/// [`FederationServiceHandler::activity`] log survives past evaluation.
///
/// `QueryEvaluator::with_default_service_handler` takes its handler by
/// value, and `spareval` gives no way to read it back out afterward — the
/// evaluator owns whatever it is given. Wrapping an [`std::sync::Arc`]
/// clone here, rather than moving the handler itself in, is what lets
/// `Catalog::execute_algebra` still hold a clone to read `activity()` from
/// once evaluation finishes. (`Arc<FederationServiceHandler>` cannot
/// implement `DefaultServiceHandler` directly — `Arc` is not a
/// [fundamental type](https://doc.rust-lang.org/reference/glossary.html#fundamental-type-constructor),
/// so a foreign trait cannot be implemented for it here; a local newtype is
/// the standard way around that.)
pub struct SharedFederationServiceHandler(std::sync::Arc<FederationServiceHandler>);

impl SharedFederationServiceHandler {
    /// Wraps this shared handler so it can be handed to
    /// [`spareval::QueryEvaluator::with_default_service_handler`] while the
    /// caller keeps its own clone to read activity from afterward.
    #[must_use]
    pub fn new(handler: std::sync::Arc<FederationServiceHandler>) -> Self {
        Self(handler)
    }
}

impl DefaultServiceHandler for SharedFederationServiceHandler {
    type Error = FederationError;

    fn handle(
        &self,
        service_name: &oxrdf::NamedNode,
        pattern: &GraphPattern,
        base_iri: Option<&oxiri::Iri<String>>,
    ) -> Result<QuerySolutionIter<'static>, Self::Error> {
        self.0.handle(service_name, pattern, base_iri)
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

    #[test]
    fn env_parsing_with_no_variable_set_allows_nothing() {
        assert!(endpoints_from_env(None).is_empty());
    }

    #[test]
    fn env_parsing_an_empty_string_allows_nothing() {
        assert!(endpoints_from_env(Some("")).is_empty());
    }

    #[test]
    fn env_parsing_splits_trims_and_drops_blank_entries() {
        let endpoints = endpoints_from_env(Some(
            " https://a.example/sparql ,,https://b.example/sparql,",
        ));
        assert_eq!(
            endpoints,
            vec![
                "https://a.example/sparql".to_string(),
                "https://b.example/sparql".to_string(),
            ]
        );
    }
}
