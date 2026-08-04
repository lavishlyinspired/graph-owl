//! Epic 1 Slice J: the contract, generated from the code that serves it.
//!
//! **One route table, two consumers.** [`ROUTES`] is the list the spec is built
//! from, and [`crate::app`] is asserted against it — so a route cannot be
//! documented without existing, or exist without being documented. A spec
//! maintained beside the router is a spec that drifts from it, and the drift is
//! invisible until a client trusts the wrong half.
//!
//! Schemas are `utoipa::ToSchema` derives on the types that actually cross the
//! wire, so a field added to `Asset` appears in the contract without anyone
//! remembering to add it. That is the whole reason the derive lives on the
//! domain type rather than on a hand-written mirror of it.
//!
//! **What this is not**: `#[utoipa::path]` on every handler. Those macros
//! restate the method, the path and the status codes next to the function,
//! which is a second place for them to be wrong. The table keeps them in one.

use serde_json::{Value, json};

/// One HTTP operation.
pub struct Route {
    pub path: &'static str,
    pub method: &'static str,
    pub summary: &'static str,
    /// Schema name for the request body, if it takes one.
    pub request: Option<&'static str>,
    /// Schema name for the `2xx` body. `None` for `204`.
    pub response: Option<&'static str>,
    /// The success status. Named per route because `201` and `204` are part of
    /// the contract, not incidental.
    pub success: u16,
    /// Whether the operation resolves a `Principal`. Drives the documented
    /// `401`, and is the machine-readable form of "this endpoint is
    /// authenticated" — which is otherwise a thing a reader has to infer.
    pub authenticated: bool,
}

const fn route(
    method: &'static str,
    path: &'static str,
    summary: &'static str,
    request: Option<&'static str>,
    response: Option<&'static str>,
    success: u16,
    authenticated: bool,
) -> Route {
    Route {
        path,
        method,
        summary,
        request,
        response,
        success,
        authenticated,
    }
}

/// Every operation this server serves.
///
/// Ordered as `app()` registers them, so the two read the same way side by
/// side. `/health`, `/ready` and `/metrics` are unauthenticated by design — an
/// orchestrator's probe and a metrics scrape must not depend on the identity
/// provider being reachable.
pub static ROUTES: &[Route] = &[
    route(
        "post",
        "/tables",
        "Create a table",
        Some("CreateTable"),
        Some("Table"),
        201,
        true,
    ),
    route(
        "get",
        "/tables",
        "List tables",
        None,
        Some("Page_Table"),
        200,
        false,
    ),
    route(
        "get",
        "/tables/{id}",
        "Fetch a table",
        None,
        Some("Table"),
        200,
        false,
    ),
    route(
        "patch",
        "/tables/{id}",
        "Update a table",
        Some("TableUpdate"),
        Some("Table"),
        200,
        true,
    ),
    route(
        "delete",
        "/tables/{id}",
        "Delete a table",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/tables/{id}/relationships",
        "Relate two tables",
        Some("CreateRelationship"),
        Some("Relationship"),
        201,
        true,
    ),
    route(
        "get",
        "/tables/{id}/relationships",
        "List a table's relationships",
        None,
        Some("Relationship_Array"),
        200,
        false,
    ),
    route(
        "delete",
        "/relationships/{id}",
        "Delete a relationship",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/assets",
        "Create or update an asset by FQN",
        Some("UpsertAsset"),
        Some("Asset"),
        201,
        true,
    ),
    route(
        "get",
        "/assets",
        "List assets; `?owner=` filters by effective owner, `?unowned=true` is the ownership-gap report",
        None,
        Some("Page_Asset"),
        200,
        false,
    ),
    route(
        "get",
        "/assets/search",
        "Search assets",
        None,
        Some("Page_Asset"),
        200,
        false,
    ),
    route(
        "get",
        "/assets/roots",
        "List root assets",
        None,
        Some("Asset_Array"),
        200,
        false,
    ),
    route(
        "get",
        "/assets/stats",
        "Asset counts by kind",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/overview",
        "Everything the landing page needs",
        None,
        None,
        200,
        false,
    ),
    route(
        "post",
        "/graph/reconcile",
        "Repair the graph projection",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/sparql",
        "Run a SPARQL query",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/cypher",
        "Run a Cypher query",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/connectors/postgres/runs",
        "Catalogue a Postgres source",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/auth/config",
        "How to authenticate against this server",
        None,
        None,
        200,
        false,
    ),
    route("get", "/health", "Liveness", None, None, 200, false),
    route("get", "/ready", "Readiness", None, None, 200, false),
    route(
        "get",
        "/metrics",
        "Prometheus exposition",
        None,
        None,
        200,
        false,
    ),
    route(
        "post",
        "/lineage",
        "Assert that one asset feeds another",
        None,
        None,
        201,
        true,
    ),
    route(
        "delete",
        "/lineage/{id}",
        "Remove a lineage edge",
        None,
        None,
        204,
        true,
    ),
    route(
        "get",
        "/lineage/asset/{id}",
        "The lineage graph around an asset, bounded upstream and downstream",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/custom-properties",
        "Define an organization-specific property on an entity type",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/custom-properties",
        "Custom property definitions, optionally for one entity type",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/custom-properties/{id}",
        "Delete a custom property definition, refused while values exist",
        None,
        None,
        204,
        true,
    ),
    // Epic 21. The submission endpoint is what an out-of-process worker — PDF,
    // OCR, LLM — talks to, so it is the one path in this document most likely
    // to be read by somebody with no access to this repository.
    route(
        "post",
        "/extraction/runs",
        "Submit a parsed document and the claims drawn from it",
        None,
        None,
        201,
        true,
    ),
    route(
        "delete",
        "/extraction/runs/{id}",
        "Delete an extraction run and everything it produced",
        None,
        None,
        204,
        true,
    ),
    route(
        "get",
        "/extraction/queue",
        "Claims awaiting confirmation, each with the sentence it came from",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/extraction/claims/{id}/decision",
        "Confirm or reject a queued claim",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/reasoning/runs",
        "Derive everything the asserted graph implies, replacing the overlay",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/reasoning/explain",
        "Why a fact holds, recursively, down to the assertions under it",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/reasoning/derived",
        "What the reasoner concluded about one subject",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/validation/runs",
        "Validate the estate against every shape and replace the queue",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/validation/shapes/seed",
        "Write the shapes the core entity model ships with",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/validation/report",
        "Current constraint violations, worst first",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/validation/waivers",
        "Accept a violation, with a reason and an expiry",
        None,
        None,
        201,
        true,
    ),
    route(
        "delete",
        "/validation/waivers/{id}",
        "Withdraw a waiver, returning the finding to the queue",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/validation/assignments",
        "Put a finding on somebody's plate",
        None,
        None,
        201,
        true,
    ),
    route(
        "delete",
        "/validation/assignments/{id}",
        "Take a finding off somebody's plate",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/policies/dry-run",
        "What a policy would do to the estate, without saving it",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/policies",
        "Create or update a policy, replacing which roles it applies to",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/policies",
        "Every stored policy, with the roles it applies to",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/policies/{name}",
        "Remove a policy",
        None,
        None,
        204,
        true,
    ),
    // Epic 31. Bodies are documented by name where a schema exists; the recall
    // and contradiction reads return composed envelopes rather than a single
    // domain type, so they carry no `response` name — the same choice the other
    // composed reads in this table already make.
    // Epic 11 Slices A, B, F, G.
    route(
        "put",
        "/users/{id}",
        "Create or rename a user; grants no roles",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/users/{id}",
        "Delete a user; `409` with counts unless `reassignTo` is given",
        None,
        None,
        204,
        true,
    ),
    route(
        "delete",
        "/teams/{id}",
        "Delete a team; `409` with counts unless `reassignTo` is given",
        None,
        None,
        204,
        true,
    ),
    route(
        "get",
        "/teams/{id}/children",
        "Teams reporting into this one",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/glossaries",
        "Create a glossary",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/glossaries",
        "Every glossary",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/glossaries/{id}",
        "Fetch a glossary",
        None,
        None,
        200,
        false,
    ),
    route(
        "delete",
        "/glossaries/{id}",
        "Delete a glossary; `409` naming its term count unless `recursive=true`",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/glossaries/{id}/terms",
        "Create a term in a glossary",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/glossaries/{id}/terms",
        "Every term in a glossary",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/glossary-terms/search",
        "Search terms by name, synonym, abbreviation or definition",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/glossary-terms/{id}",
        "Fetch a term",
        None,
        None,
        200,
        false,
    ),
    route(
        "patch",
        "/glossary-terms/{id}",
        "Update a term's definition, synonyms or abbreviations",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/glossary-terms/{id}",
        "Delete a term",
        None,
        None,
        204,
        true,
    ),
    route(
        "post",
        "/glossary-terms/{id}/relations",
        "Assert a SKOS relation, owned by this term",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/glossary-terms/{id}/relations",
        "Every relation visible on this term, derived inverses included",
        None,
        None,
        200,
        false,
    ),
    route(
        "delete",
        "/glossary-terms/{id}/relations",
        "Retract a relation this term declared",
        None,
        None,
        204,
        true,
    ),
    route(
        "put",
        "/glossary-terms/{id}/reviewers",
        "Replace a term's assigned reviewers",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/glossary-terms/{id}/reviewers",
        "A term's assigned reviewers",
        None,
        None,
        200,
        false,
    ),
    route(
        "post",
        "/glossary-terms/{id}/transitions",
        "Move a term through its review workflow",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/glossary-terms/{id}/usage",
        "Attach a term to an asset or column, by FQN",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/glossary-terms/{id}/usage",
        "Every asset or column this term is attached to",
        None,
        None,
        200,
        false,
    ),
    route(
        "delete",
        "/glossary-terms/{id}/usage",
        "Detach a term from an asset or column",
        None,
        None,
        204,
        true,
    ),
    // `/business-metrics`, not `/metrics` — that path already names the
    // Prometheus exposition endpoint below.
    route(
        "post",
        "/business-metrics",
        "Create a metric",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/business-metrics",
        "Every metric, paginated",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/business-metrics/search",
        "Search metrics by name, definition, or defining term",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/business-metrics/{id}",
        "Fetch a metric",
        None,
        None,
        200,
        false,
    ),
    route(
        "patch",
        "/business-metrics/{id}",
        "Update a metric's definition, formula, unit, granularity or calculation type",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/business-metrics/{id}",
        "Delete a metric",
        None,
        None,
        204,
        true,
    ),
    route(
        "put",
        "/business-metrics/{id}/sources",
        "Replace a metric's declared sources, reconciled by source",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/users/{id}/follows",
        "What this user follows",
        None,
        Some("Page_Asset"),
        200,
        true,
    ),
    route(
        "put",
        "/assets/{id}/followers/{user_id}",
        "Follow an asset; idempotent, so a second follow is also 200",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/assets/{id}/followers/{user_id}",
        "Stop following an asset",
        None,
        None,
        204,
        true,
    ),
    // Epic 11 Slice C. `PUT` because the body is the complete owner list.
    route(
        "put",
        "/assets/{id}/owners",
        "Set who owns this asset; an empty list makes it unowned",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/assets/{id}/owners",
        "Who owns this asset",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/memories",
        "Write down organizational knowledge",
        None,
        Some("Memory"),
        201,
        true,
    ),
    route(
        "get",
        "/memories/{id}",
        "One memory, superseded or not",
        None,
        Some("Memory"),
        200,
        true,
    ),
    route(
        "post",
        "/memories/{id}/supersede",
        "Correct a memory, keeping the original readable",
        None,
        Some("Memory"),
        201,
        true,
    ),
    route(
        "post",
        "/memories/{id}/mentions",
        "Resolve a textual mention found in this memory against the catalog",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/merges/{id}/split",
        "Reverse a merge, restoring both entities",
        None,
        Some("MergeRecord"),
        200,
        true,
    ),
    route(
        "get",
        "/resolution/queue",
        "Ambiguous candidate pairs awaiting review, pending first",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/resolution/queue/bulk",
        "Confirm or reject several review-queue entries in one request",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/resolution/queue/{id}/confirm",
        "Confirm a queued pair; writes the merge",
        None,
        Some("Resolution"),
        200,
        true,
    ),
    route(
        "post",
        "/resolution/queue/{id}/reject",
        "Reject a queued pair; the decision persists",
        None,
        None,
        204,
        true,
    ),
    route(
        "get",
        "/assets/{id}/memories",
        "What we know about this asset, best first, each flagged for staleness",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/assets/{id}/contradictions",
        "Open disagreements about this asset; nothing is resolved and nothing is hidden",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/contradictions/reviews",
        "Confirm or dismiss a candidate contradiction; confirming does not close it",
        None,
        None,
        204,
        true,
    ),
    // Epic 16 Slice A. `207` because a batch has per-item outcomes: `200` would
    // claim the whole push succeeded, `400` that it failed, and neither is true
    // when 999 of 1000 landed.
    route(
        "post",
        "/ingest",
        "Push entities in one call; 207 with per-item status, partial success",
        None,
        None,
        207,
        true,
    ),
    // Epic 16 Slice C. `202` because decision 2 makes a batch a job: the only
    // honest synchronous answer to a 500k-row file is "I have started", and a
    // `200` would claim a result nobody has yet.
    route(
        "post",
        "/ingest/batch",
        "Upload a JSONL or CSV batch; 202 with a job handle to poll",
        None,
        None,
        202,
        true,
    ),
    route(
        "get",
        "/ingest/jobs/{id}",
        "How a batch job is doing: state, counts, and per-row failures",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/ingest/jobs/{id}",
        "Ask a batch job to stop; the response says what had landed",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/connectors/{connector}/test",
        "Try a connection before saving it; a refused connection is 200 with ok:false",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/connectors/{connector}/schema",
        "What a connector needs configured, as JSON Schema",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/connectors/configs",
        "Save a connector configuration; the credential is write-only",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/connectors/configs",
        "Every connector configuration, without credentials",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/webhooks/endpoints",
        "Register a webhook endpoint; the secret is write-only",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/webhooks/endpoints",
        "Every registered webhook endpoint, without secrets",
        None,
        None,
        200,
        true,
    ),
    // Not `authenticated`: the sender carries no bearer token, and this is
    // the mechanism-level meaning that flag has everywhere else in this
    // table (see `/health`, `/ready`, `/metrics`). The endpoint can still
    // answer `401` on a bad or missing signature — checked by
    // `Catalog::receive_webhook`, not by the bearer-token machinery this
    // flag documents — which is why that response is not listed here.
    route(
        "post",
        "/webhooks/receive/{path}",
        "Receive a webhook delivery; the raw body is verified before parsing",
        None,
        None,
        201,
        false,
    ),
    route(
        "post",
        "/webhooks/mappings",
        "Register a new version of a payload-to-draft mapping",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/webhooks/mappings/{name}",
        "The latest version of a mapping",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/webhooks/mappings/{name}/versions",
        "Every version of a mapping, newest first",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/webhooks/mappings/{name}/dry-run",
        "Apply a mapping to a sample payload without writing anything",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/webhooks/events/{id}",
        "The status of one inbound event",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/webhooks/dead-letters",
        "The dead-letter queue, filterable by endpoint and reason",
        None,
        None,
        200,
        true,
    ),
    route(
        "delete",
        "/webhooks/dead-letters",
        "Purge dead-lettered events older than a caller-named cutoff",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/webhooks/replay",
        "Replay a window of an endpoint's events, in sender-timestamp order",
        None,
        None,
        200,
        true,
    ),
    route(
        "post",
        "/teams",
        "Create or update a team, replacing its membership",
        None,
        None,
        201,
        true,
    ),
    route(
        "get",
        "/teams",
        "Every team, with its members",
        None,
        None,
        200,
        true,
    ),
    route(
        "put",
        "/users/{id}/roles",
        "Replace a user's roles, invalidating cached authorization",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/connectors/runs",
        "Recent connector runs, newest first",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/openapi.json",
        "This document",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/docs/",
        "Interactive documentation for this contract",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/assets/{id}",
        "Fetch an asset",
        None,
        Some("Asset"),
        200,
        false,
    ),
    route(
        "patch",
        "/assets/{id}",
        "Update an asset",
        Some("AssetUpdate"),
        Some("Asset"),
        200,
        true,
    ),
    // **200 with a count, not 204.** Found by Slice K: the contract said 204 and
    // the handler has always returned the cascade count, deliberately — "a
    // delete that silently tombstoned 400 columns and returned 204 would leave
    // an operator unable to tell whether it did what they meant". A generated
    // client would have been written against the wrong one.
    route(
        "delete",
        "/assets/{id}",
        "Soft-delete an asset and its subtree, reporting how many were tombstoned",
        None,
        Some("CascadeCount"),
        200,
        true,
    ),
    route(
        "get",
        "/assets/{id}/versions",
        "An asset's version history",
        None,
        Some("AssetVersion_Array"),
        200,
        false,
    ),
    route(
        "post",
        "/assets/{id}/restore",
        "Restore a soft-deleted asset",
        None,
        Some("Asset"),
        200,
        true,
    ),
    route(
        "post",
        "/assets/{id}/resolve",
        "Resolve this asset against its blocking-key candidates",
        None,
        Some("Resolution"),
        200,
        true,
    ),
    route(
        "get",
        "/assets/{id}/children",
        "An asset's children",
        None,
        Some("Asset_Array"),
        200,
        false,
    ),
    route(
        "get",
        "/assets/{id}/graph",
        "The neighbourhood around an asset",
        None,
        None,
        200,
        false,
    ),
    route(
        "get",
        "/assets/{id}/ancestors",
        "An asset's ancestors",
        None,
        Some("Asset_Array"),
        200,
        false,
    ),
    // ---- Epic 14 + 32: the agent surface ----
    //
    // `/mcp` carries JSON-RPC, so its request and response shapes are the
    // protocol's rather than this contract's — declared with no schema on
    // purpose. Documenting a `$ref` here would claim a stability the JSON-RPC
    // envelope does not have, and a generated client should not try to type it.
    route(
        "post",
        "/mcp",
        "MCP tools over JSON-RPC 2.0",
        None,
        None,
        200,
        true,
    ),
    route(
        "get",
        "/agents/grants",
        "Every agent grant",
        None,
        Some("AgentGrant_Array"),
        200,
        true,
    ),
    route(
        "get",
        "/agents/{agent_id}/grant",
        "What one agent may do",
        None,
        Some("AgentGrant"),
        200,
        true,
    ),
    route(
        "put",
        "/agents/{agent_id}/grant",
        "Grant or replace an agent's capabilities",
        Some("AgentGrantRequest"),
        Some("AgentGrant"),
        200,
        true,
    ),
    route(
        "delete",
        "/agents/{agent_id}/grant",
        "Revoke an agent's capabilities",
        None,
        None,
        204,
        true,
    ),
    route(
        "get",
        "/agents/{agent_id}/activity",
        "An agent's writes, including refused attempts",
        None,
        Some("Page_AgentActivity"),
        200,
        true,
    ),
    route(
        "get",
        "/proposals",
        "Agent proposals awaiting a human",
        None,
        Some("Page_Proposal"),
        200,
        true,
    ),
    route(
        "get",
        "/proposals/{id}",
        "One proposal",
        None,
        Some("Proposal"),
        200,
        true,
    ),
    route(
        "post",
        "/proposals/{id}/accept",
        "Accept a proposal — applied with the agent as author",
        None,
        Some("Proposal"),
        200,
        true,
    ),
    route(
        "post",
        "/proposals/{id}/reject",
        "Reject a proposal — nothing is applied",
        None,
        None,
        204,
        true,
    ),
];

/// The schemas the contract references.
#[derive(utoipa::OpenApi)]
#[openapi(components(schemas(
    graph_owl_authz::agent::AgentGrant,
    graph_owl_authz::agent::AgentCapability,
    graph_owl_authz::agent::RateLimit,
    graph_owl_authz::agent::ScopeRef,
    graph_owl_authz::agent::Proposal,
    graph_owl_authz::agent::ProposalStatus,
    graph_owl_authz::agent::AgentActivity,
    graph_owl_authz::agent::ActivityOutcome,
    crate::AgentGrantRequest,
    graph_owl_core::Asset,
    graph_owl_core::AssetKind,
    graph_owl_core::AssetUpdate,
    graph_owl_core::AssetVersion,
    graph_owl_core::Table,
    graph_owl_core::TableUpdate,
    graph_owl_core::Relationship,
    graph_owl_core::page::Paging,
    graph_owl_core::envelope::EntityVersion,
    graph_owl_core::envelope::ChangeKind,
    graph_owl_core::envelope::FieldChange,
    graph_owl_core::envelope::ChangeDescription,
    graph_owl_api::CreateTable,
    graph_owl_api::CreateRelationship,
    graph_owl_api::UpsertAsset,
    graph_owl_core::memory::Memory,
    graph_owl_core::memory::MemoryKind,
    graph_owl_core::memory::MemoryLink,
    graph_owl_core::memory::LinkRelation,
    graph_owl_core::memory::Authorship,
    graph_owl_core::resolution::MergeRecord,
    graph_owl_core::resolution::Evidence,
    graph_owl_core::resolution::MergeDecidedBy,
    graph_owl_core::resolution::Resolution,
    graph_owl_core::resolution::Candidate,
)))]
struct Components;

/// The RFC 9457 problem document every error uses.
///
/// Hand-written rather than derived because [`crate::AppError`] is an enum that
/// renders *into* this shape rather than being it — there is no Rust type whose
/// schema this would be. `00d-api-conventions.md` is the definition; this
/// mirrors it, and the round-trip test in `tests/openapi.rs` is what keeps the
/// mirror honest.
fn problem_schema() -> Value {
    json!({
        "type": "object",
        "description": "RFC 9457 problem details. Every error response uses this shape.",
        "required": ["type", "title", "status"],
        "properties": {
            "type": { "type": "string", "format": "uri",
                      "description": "Stable URI naming the error kind. The only field a client should branch on." },
            "title": { "type": "string" },
            "status": { "type": "integer", "format": "int32" },
            "detail": { "type": "string" },
            "errors": {
                "type": "array",
                "description": "Field violations, all of them — never just the first.",
                "items": {
                    "type": "object",
                    "properties": {
                        "field": { "type": "string" },
                        "code": { "type": "string" },
                        "message": { "type": "string" }
                    }
                }
            }
        }
    })
}

fn page_of(item: &str) -> Value {
    json!({
        "type": "object",
        "required": ["data", "paging"],
        "properties": {
            "data": { "type": "array", "items": { "$ref": format!("#/components/schemas/{item}") } },
            "paging": { "$ref": "#/components/schemas/Paging" }
        }
    })
}

fn array_of(item: &str) -> Value {
    json!({ "type": "array", "items": { "$ref": format!("#/components/schemas/{item}") } })
}

fn body(schema: &str) -> Value {
    json!({ "content": { "application/json": {
        "schema": { "$ref": format!("#/components/schemas/{schema}") } } } })
}

fn problem(status: u16, title: &str) -> (String, Value) {
    (
        status.to_string(),
        json!({
            "description": title,
            "content": { "application/problem+json": {
                "schema": { "$ref": "#/components/schemas/Problem" } } }
        }),
    )
}

/// The `OpenAPI` 3.1 document.
///
/// # Panics
///
/// If the derived components cannot be serialized, which would mean a
/// `ToSchema` derive produced something `serde_json` cannot represent — a build
/// -time impossibility surfaced as a startup failure rather than a silently
/// truncated contract.
#[must_use]
pub fn document() -> Value {
    use utoipa::OpenApi as _;

    let mut schemas = serde_json::to_value(Components::openapi())
        .ok()
        .and_then(|v| v.get("components")?.get("schemas").cloned())
        .unwrap_or_else(|| json!({}));

    if let Some(map) = schemas.as_object_mut() {
        map.insert("Problem".to_string(), problem_schema());
        // The cascade count a soft delete and a restore report. Hand-written
        // because the handlers build it inline; there is no Rust type whose
        // schema this would be.
        map.insert(
            "CascadeCount".to_string(),
            json!({
                "type": "object",
                "description": "How many assets the operation affected, including the subtree.",
                "properties": { "deleted": { "type": "integer", "format": "int64" } }
            }),
        );
        // Generic instantiations. `Page<T>` is one Rust type and several
        // contract types, and OpenAPI has no generics to express that.
        map.insert("Page_Asset".to_string(), page_of("Asset"));
        map.insert("Page_Table".to_string(), page_of("Table"));
        map.insert("Asset_Array".to_string(), array_of("Asset"));
        map.insert("Relationship_Array".to_string(), array_of("Relationship"));
        map.insert("AssetVersion_Array".to_string(), array_of("AssetVersion"));
        // Epic 32.
        map.insert("Page_Proposal".to_string(), page_of("Proposal"));
        map.insert("Page_AgentActivity".to_string(), page_of("AgentActivity"));
        map.insert("AgentGrant_Array".to_string(), array_of("AgentGrant"));
    }

    let mut paths = serde_json::Map::new();
    for route in ROUTES {
        let mut responses = serde_json::Map::new();
        let success = match route.response {
            Some(schema) => {
                let mut r = body(schema);
                r["description"] = json!(route.summary);
                r
            }
            None => json!({ "description": route.summary }),
        };
        responses.insert(route.success.to_string(), success);

        // Documented per route rather than blanket-applied: an endpoint that
        // cannot 401 must not claim it can, or a generated client grows a
        // branch that is dead for that call.
        if route.authenticated {
            let (code, value) = problem(401, "Authentication required");
            responses.insert(code, value);
        }
        if route.path.contains("{id}") {
            let (code, value) = problem(404, "No such resource, or not visible to this principal");
            responses.insert(code, value);
        }
        if route.request.is_some() {
            let (code, value) = problem(
                400,
                "The request body is invalid; every violation is listed",
            );
            responses.insert(code, value);
        }
        // Asked of `admission` rather than restated here, so the contract
        // cannot claim a `503` the middleware does not produce — or, worse,
        // omit one it does. A generated client that has no branch for the
        // refusal treats a shed request as a transport failure and retries it
        // immediately, which is the storm admission control exists to end.
        if crate::admission::class_of(route.path).is_some() {
            let (code, value) = problem(
                503,
                "At the concurrency limit for this path; refused rather than queued. \
                 `Retry-After` names the interval",
            );
            responses.insert(code, value);
        }

        let mut operation = json!({
            "summary": route.summary,
            "operationId": format!("{}{}", route.method,
                route.path.replace(['/', '{', '}'], "_")),
            "responses": Value::Object(responses),
        });
        // Named on the operation, not assumed. `security: []` on the open
        // routes is not the same as omitting it: omitting inherits the
        // document default, and an empty array explicitly says *this one takes
        // no credential* — which is the difference between "/health is
        // unauthenticated" and "nobody wrote it down".
        operation["security"] = if route.authenticated {
            json!([{ "bearerAuth": [] }])
        } else {
            json!([])
        };
        if let Some(schema) = route.request {
            operation["requestBody"] = json!({ "required": true, "content": {
                "application/json": { "schema": { "$ref": format!("#/components/schemas/{schema}") } } } });
        }
        if route.path.contains("{id}") {
            operation["parameters"] = json!([{
                "name": "id", "in": "path", "required": true,
                "schema": { "type": "string", "format": "uuid" }
            }]);
        }

        paths
            .entry(route.path.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("a path item is an object")
            .insert(route.method.to_string(), operation);
    }

    json!({
        "openapi": "3.1.0",
        // Relative, deliberately. The console, the API and this document are
        // served from one origin by one binary (`00f-ui-architecture.md`), so
        // an absolute URL here would name a host that is right for exactly one
        // deployment and wrong for every other.
        "servers": [{ "url": "/", "description": "This server" }],
        "info": {
            "title": "graph-owl",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "A knowledge graph engine for enterprise metadata. \
                            Errors are RFC 9457 problem documents; lists are \
                            keyset-paginated. See plans/00d-api-conventions.md."
        },
        "paths": Value::Object(paths),
        "components": {
            "schemas": schemas,
            // The spec documents a `401` on authenticated routes; without this
            // it never says *how* to avoid one, and a generated client has no
            // way to send a credential at all.
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT",
                    "description": "An OIDC access token (RS256, verified against \
                                    the issuer's JWKS), or an HS256 token when the \
                                    server is configured with a shared secret."
                }
            }
        }
    })
}

/// Serve the contract at runtime, so a client never has to find the file.
pub async fn endpoint() -> axum::Json<Value> {
    axum::Json(document())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_declares_openapi_3_1() {
        assert_eq!(document()["openapi"], "3.1.0");
    }

    #[test]
    fn every_declared_route_appears_in_the_document() {
        let doc = document();
        for route in ROUTES {
            let operation = &doc["paths"][route.path][route.method];
            assert!(
                !operation.is_null(),
                "{} {} is missing from the spec",
                route.method,
                route.path
            );
            assert!(
                operation["responses"][route.success.to_string()].is_object(),
                "{} {} does not document its {} response",
                route.method,
                route.path,
                route.success
            );
        }
    }

    /// And the negative: the document must not invent operations the table does
    /// not declare. Without this, a generator emitting a fixed sample would
    /// satisfy the test above.
    #[test]
    fn the_document_contains_nothing_the_table_does_not_declare() {
        let doc = document();
        let declared: std::collections::HashSet<_> =
            ROUTES.iter().map(|r| (r.path, r.method)).collect();

        for (path, item) in doc["paths"].as_object().expect("paths") {
            for method in item.as_object().expect("a path item").keys() {
                assert!(
                    declared.contains(&(path.as_str(), method.as_str())),
                    "{method} {path} is in the spec and not in the route table"
                );
            }
        }
    }

    /// Every `$ref` has to resolve. A dangling reference is the failure mode of
    /// a hand-assembled document, and it produces a client that fails to
    /// generate rather than one that generates wrongly — so it is cheap to
    /// catch and expensive to ship.
    #[test]
    fn every_reference_resolves_to_a_defined_schema() {
        let doc = document();
        let schemas = doc["components"]["schemas"].as_object().expect("schemas");

        fn refs(value: &Value, found: &mut Vec<String>) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        if key == "$ref"
                            && let Some(name) = child.as_str()
                        {
                            found.push(name.to_string());
                        }
                        refs(child, found);
                    }
                }
                Value::Array(items) => items.iter().for_each(|i| refs(i, found)),
                _ => {}
            }
        }

        let mut found = Vec::new();
        refs(&doc, &mut found);
        assert!(
            !found.is_empty(),
            "a document with no references proves nothing"
        );

        for reference in found {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .unwrap_or_else(|| panic!("{reference} is not a components ref"));
            assert!(
                schemas.contains_key(name),
                "{reference} does not resolve; the spec would not generate a client"
            );
        }
    }

    #[test]
    fn an_authenticated_route_documents_its_401_and_an_open_one_does_not() {
        let doc = document();

        assert!(
            doc["paths"]["/assets"]["post"]["responses"]["401"].is_object(),
            "creating an asset requires a principal"
        );
        // The negative: `/health` must not claim an error it cannot produce, or
        // a generated client grows a branch that is dead for that call.
        assert!(
            doc["paths"]["/health"]["get"]["responses"]["401"].is_null(),
            "liveness is unauthenticated by design"
        );
    }

    #[test]
    fn a_body_taking_route_documents_the_validation_error() {
        let doc = document();

        assert!(doc["paths"]["/assets"]["post"]["responses"]["400"].is_object());
        assert!(doc["paths"]["/assets"]["post"]["requestBody"]["required"] == json!(true));
        assert!(doc["paths"]["/assets/roots"]["get"]["responses"]["400"].is_null());
    }

    #[test]
    fn the_problem_schema_names_the_fields_a_client_branches_on() {
        let problem = &document()["components"]["schemas"]["Problem"];

        for field in ["type", "title", "status"] {
            assert!(
                problem["properties"][field].is_object(),
                "a problem document without `{field}` is not RFC 9457"
            );
        }
        assert!(
            problem["properties"]["errors"].is_object(),
            "field violations"
        );
    }

    /// The domain type's own fields reach the contract. This is the property
    /// that makes the derive worth having: a field added to `Asset` appears
    /// here without anybody remembering to add it.
    #[test]
    fn the_asset_schema_comes_from_the_domain_type() {
        let asset = &document()["components"]["schemas"]["Asset"];

        for field in ["id", "kind", "name", "fullyQualifiedName", "version"] {
            assert!(
                asset["properties"][field].is_object(),
                "Asset.{field} is missing from the generated schema"
            );
        }
    }

    /// camelCase on the wire, per `00d`. A schema documenting `fully_qualified_name`
    /// would be a contract nobody can use against this server.
    #[test]
    fn schema_properties_are_camel_case_like_the_wire() {
        let asset = &document()["components"]["schemas"]["Asset"];

        assert!(asset["properties"]["fullyQualifiedName"].is_object());
        assert!(asset["properties"]["fully_qualified_name"].is_null());
    }
}
