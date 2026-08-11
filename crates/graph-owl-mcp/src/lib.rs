//! MCP server: read capabilities over the context graph — Epic 14 Slice A.
//!
//! **Protocol and policy, no I/O.** The catalog is reached through
//! [`ContextSource`], which the composition root implements over `Catalog`.
//! That keeps the part carrying the security-relevant decisions — what a tool
//! declares, and what an agent is allowed to learn — testable without a
//! database, and it is the port shape the rest of this workspace uses.
//!
//! The one decision worth reading this file for: **denied and absent are the
//! same answer**. See [`Outcome::NotFound`].

pub mod budget;
pub mod catalog;
pub mod jsonrpc;
pub mod lineage;
pub mod trust;
pub mod write;

use async_trait::async_trait;
use serde::Serialize;

/// What a tool needs from the catalog.
///
/// Deliberately narrow. An MCP surface reaching everything the facade can would
/// be a second, unreviewed API — and this is the surface an *agent* drives,
/// which is where a too-wide capability is hardest to notice.
#[async_trait]
pub trait ContextSource: Send + Sync {
    /// The asset, **already filtered by the caller's policy**.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached. `Ok(None)`
    /// covers both "no such asset" and "not for you", and an implementation
    /// must not distinguish them — see [`Outcome::NotFound`].
    async fn asset_context(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<AssetContext>, SourceError>;

    /// What people have written down about this asset — Epic 31 over MCP.
    ///
    /// `Ok(None)` means the asset is unknown **or** withheld, and an
    /// implementation must not distinguish them, for the same reason
    /// [`Outcome::NotFound`] exists.
    ///
    /// `Ok(Some(vec![]))` is different and must stay different: the asset is
    /// visible and nothing has been recorded about it. An agent told "not found"
    /// for that will assume the asset does not exist and start inventing; told
    /// "nothing recorded", it can say so.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached.
    async fn recall(
        &self,
        principal: &str,
        fqn: &str,
        query: &str,
    ) -> Result<Option<Vec<MemoryContext>>, SourceError>;

    /// Ranked hits, **already filtered by policy** — and `total` counted after
    /// the filter, never before. See [`SearchResults::total`].
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached. There is no
    /// not-found here: a search matching nothing is a real, complete answer.
    async fn search(
        &self,
        principal: &str,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<SearchResults, SourceError>;

    /// A bounded lineage walk from `fqn`.
    ///
    /// `Ok(None)` means the asset is unknown **or** withheld, as everywhere
    /// else. A walk that reaches a denied *neighbour* is different: it succeeds
    /// with `policy_filtered` set, because the caller may see where they
    /// started.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached.
    async fn lineage(
        &self,
        principal: &str,
        fqn: &str,
        direction: Direction,
    ) -> Result<Option<lineage::LineageWalk>, SourceError>;

    /// What a change to `fqn` would affect.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached.
    async fn impact(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<lineage::ImpactReport>, SourceError>;

    /// How `fqn` is governed — classifications, masking, retention, and what
    /// the caller may do.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached.
    async fn governance(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<lineage::GovernanceContext>, SourceError>;

    /// Evaluate a graph query with the caller's policy compiled in.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached. A query that
    /// does not parse, or uses something unsupported, comes back as
    /// [`QueryFault`] — those are answers about the query, not failures to run
    /// it, and an agent acts on them differently.
    async fn query_graph(
        &self,
        principal: &str,
        query: &str,
    ) -> Result<Result<QueryAnswer, QueryFault>, SourceError>;

    /// A bounded neighbourhood walk from a catalog asset — Epic 105 P10,
    /// the first of the platform plan's eight intelligence tools
    /// (`traverse()`). **Deliberately scoped to catalog assets, not any
    /// graph subject.** `Catalog::asset_subgraph` checks the caller's
    /// visibility before walking (`00b` decision 7); a pack-domain
    /// subject (a GST invoice, a hospitality guest) has no such policy
    /// model yet — `plans/105-domain-neutrality.md`'s own recorded gap —
    /// so this tool must not reach one until that model exists, rather
    /// than silently serving an unauthorized read through an agent
    /// surface.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached. `Ok(None)`
    /// covers "no such asset" **and** "not visible to this principal",
    /// which must not be distinguished, for the same reason
    /// [`Outcome::NotFound`] exists everywhere else on this trait.
    async fn traverse(
        &self,
        principal: &str,
        fqn: &str,
        direction: Direction,
        max_hops: u32,
    ) -> Result<Option<TraversalContext>, SourceError>;
}

/// A bounded walk's answer, as an agent receives it — Epic 105 P10's
/// `traverse()` tool.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TraversalContext {
    pub nodes: Vec<TraversalNode>,
    pub edges: Vec<TraversalEdge>,
    /// The walk hit its bounds before exhausting the neighbourhood — a
    /// smaller answer than the true one, not a wrong one, and an agent
    /// that cannot tell the two apart presents the smaller as complete.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<budget::TruncationReason>,
}

/// One node the walk reached, its identity rendered as a full IRI so an
/// agent can pass it straight back into another tool call.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraversalNode {
    pub id: String,
}

/// One edge the walk followed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraversalEdge {
    pub from: String,
    pub to: String,
    pub relationship: String,
    /// The reasoner concluded this edge; nobody asserted it — carried
    /// through from [`graph_owl_traversal::EdgeRef::derived`] rather than
    /// dropped, the same reason a derived triple is tagged everywhere else
    /// this system renders one.
    pub derived: bool,
}

/// Which way a lineage walk goes.
///
/// Two questions, not one: "where did this come from" and "what breaks if I
/// change it" have different answers and different audiences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upstream,
    Downstream,
}

/// One search hit as an agent receives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub fully_qualified_name: String,
    pub kind: String,
    /// Enough description to choose between hits, not the whole thing — the
    /// agent asks for context on the one it picks.
    pub snippet: Option<String>,
    /// Carried per hit, because "there are eleven tables called `orders`" is
    /// only useful alongside "and one of them is certified".
    pub trust: crate::trust::TrustSummary,
}

/// A ranked, policy-filtered result set.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// **How many the caller may see — counted after the policy filter, never
    /// before.**
    ///
    /// A total taken before filtering is wrong twice over. It tells an agent
    /// there are ten results when it may have three, so the agent pages for
    /// seven that will never arrive and concludes the tool is broken; and the
    /// difference between the two numbers *is* the disclosure the policy exists
    /// to prevent — an exact count of the assets being hidden.
    pub total: usize,
    /// Something matched that the caller may not see.
    pub policy_filtered: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<budget::TruncationReason>,
}

/// Bindings from a graph query.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueryAnswer {
    /// One map per solution: variable name to its bound term, rendered.
    pub rows: Vec<std::collections::BTreeMap<String, String>>,
    /// The budget cut the scan short, so this may be incomplete. **Always set
    /// when it happened** — partial results presented as complete is the one
    /// outcome this crate refuses everywhere.
    pub truncated: bool,
}

/// Why a graph query did not run.
///
/// **Two kinds, deliberately separated.** A query the agent wrote wrongly is
/// its problem to fix; a query this engine does not yet support is ours, and an
/// agent told "malformed" for the second will rewrite a correct query forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryFault {
    /// It did not parse.
    Malformed(String),
    /// It parsed and asks for something this engine does not implement.
    Unsupported(String),
}

/// One recalled memory, as an agent receives it.
///
/// Four of these fields exist to stop an agent presenting something as more
/// authoritative than it is. Each is a flag rather than a caveat buried in prose,
/// because an agent summarising prose drops the caveat first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContext {
    pub kind: String,
    pub content: String,
    pub summary: Option<String>,
    pub confidence: f64,
    /// **Whether a person wrote this.**
    ///
    /// An agent that cannot tell its own earlier output from institutional
    /// knowledge reads its own guess back as fact and compounds it — once per
    /// retrieval, with the confidence growing each time because it keeps finding
    /// "the catalog says so". This is the flag that breaks that loop.
    pub human_authored: bool,
    /// `None` when the subject has not changed since this was written; otherwise
    /// what changed, in words.
    ///
    /// Same argument as [`AssetContext::policy_filtered`]: an agent that cannot
    /// tell current from stale presents stale as current, and the person reading
    /// it has no way to know.
    pub staleness: Option<String>,
    /// **This memory is party to an open disagreement.**
    ///
    /// Without it an agent picks one of two conflicting memories and presents it
    /// as the answer — which is software adjudicating institutional disagreement
    /// by omission, and this epic refuses to do that anywhere else.
    pub contradicted: bool,
}

/// Something went wrong reaching the catalog.
///
/// **Not** a policy decision — those are absences, not errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("the catalog could not be reached: {0}")]
    Unavailable(String),
}

/// One asset as an agent receives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetContext {
    pub fully_qualified_name: String,
    pub kind: String,
    pub description: Option<String>,
    /// Related assets the caller may see.
    pub related: Vec<String>,
    /// **Set when policy withheld something.**
    ///
    /// An agent that cannot tell a complete answer from a filtered one presents
    /// the filtered one as complete, and the person reading it has no way to
    /// know. This flag is the difference between a partial answer and a wrong
    /// one.
    pub policy_filtered: bool,
    /// What the agent should believe about this asset. Carried on every
    /// context, because retrieval without it is a fact with no weight attached
    /// — and an agent given facts and no confidence reports them all alike.
    pub trust: crate::trust::TrustSummary,
    /// The response did not fit its token budget whole. Distinct from
    /// `policy_filtered`: one is "you may not have it", the other is "it did
    /// not fit", and only the second is worth retrying.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<budget::TruncationReason>,
}

/// What a tool call produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Found(Box<AssetContext>),
    /// What is known about an asset, best first. **Empty is a real answer** —
    /// "nothing has been written down" is information, and it is not
    /// [`Outcome::NotFound`].
    Recalled(Vec<MemoryContext>),
    /// Ranked hits. **Empty is a real answer**, for the same reason
    /// `Recalled(vec![])` is: "nothing matched" and "no such thing to match
    /// against" are different statements.
    Searched(Box<SearchResults>),
    /// A bounded lineage walk.
    Lineage(Box<lineage::LineageWalk>),
    /// What a change would affect.
    Impact(Box<lineage::ImpactReport>),
    /// How an asset is governed.
    Governance(Box<lineage::GovernanceContext>),
    /// Bindings from a graph query.
    Bindings(Box<QueryAnswer>),
    /// A bounded neighbourhood walk — Epic 105 P10's `traverse()`.
    Traversed(Box<TraversalContext>),
    /// A write landed or became a proposal — Epic 32.
    Wrote(Box<write::WriteReceipt>),
    /// **An agent write was refused, readably.**
    ///
    /// Distinct from [`Outcome::BadRequest`]: the request was well-formed and
    /// the agent simply may not do it. The text names which rule refused and
    /// what would change the answer, because asking a human for a capability is
    /// a different next step from retrying.
    Refused(String),
    /// **The engine does not implement what the query asked for.**
    ///
    /// Distinct from [`Outcome::BadRequest`], which says the caller got it
    /// wrong. An agent told "malformed" for an unimplemented feature rewrites a
    /// correct query until it gives up; told "unsupported", it takes a
    /// different route the first time.
    Unsupported(String),
    /// **Absent and denied, indistinguishable.**
    ///
    /// A refusal naming an asset the caller cannot see tells them it exists,
    /// which is the fact the policy was written to withhold. "There is no
    /// `finance.salaries`" and "you may not see `finance.salaries`" must reach
    /// the agent as one answer, and the only way to guarantee that is to have
    /// one variant carrying no detail.
    NotFound,
    /// No principal, or one that did not verify.
    Unauthenticated,
    /// The arguments did not match the declared schema.
    BadRequest(String),
    /// The catalog is down.
    ///
    /// Distinct from `NotFound`, because "we could not look" and "it is not
    /// there" are opposite statements — and an agent that conflates them
    /// reports an absence it never checked, with the confidence of one it did.
    Unavailable(String),
}

/// A tool as MCP's discovery response declares it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeclaration {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema for the arguments, as MCP requires.
    pub input_schema: serde_json::Value,
}

/// The name the protocol addresses this tool by.
///
/// A constant because the declaration and the dispatcher must agree, and two
/// string literals are one typo away from a tool that advertises and cannot be
/// called.
pub const GET_ASSET_CONTEXT: &str = "get_asset_context";

/// The name the protocol addresses the recall tool by.
pub const RECALL_MEMORY: &str = "recall_memory";

/// Discovery — Epic 14 Slice C.
pub const SEARCH_ASSETS: &str = "search_assets";

/// Provenance and flow — Epic 14 Slice C.
pub const EXPLAIN_LINEAGE: &str = "explain_lineage";

/// Blast radius — Epic 14 Slice C.
pub const ANALYZE_IMPACT: &str = "analyze_impact";

/// Handling rules — Epic 14 Slice D.
pub const GET_GOVERNANCE_CONTEXT: &str = "get_governance_context";

/// The escape hatch — Epic 14 Slice D.
pub const QUERY_GRAPH: &str = "query_graph";

/// A bounded neighbourhood walk — Epic 105 P10, the first of the platform
/// plan's eight intelligence tools.
pub const TRAVERSE: &str = "traverse";

/// The most hops an agent may ask `traverse` to walk.
///
/// Six, matching `GET /findings/{id}/evidence-graph`'s own server-side cap
/// (`plans/105e-evidence-chain-walk.md`) — the same bound applied for the
/// same reason: the cap exists to protect the server, not to be polite to
/// the caller, and an agent-facing surface has no less reason to enforce
/// it than a human-facing one.
const MAX_TRAVERSE_HOPS: u32 = 6;

/// `traverse`'s default when the caller does not say — small enough that a
/// vague question does not accidentally request the whole graph.
const DEFAULT_TRAVERSE_HOPS: u32 = 2;

/// How many hits one search returns before it reports truncation.
///
/// Twenty-five: enough that a real ranking has room to be wrong about the first
/// few, and few enough that the response is still something an agent reads
/// rather than pages. Larger sets are a sign the query was too broad, and the
/// better fix is a narrower query than a longer answer.
const DEFAULT_SEARCH_LIMIT: usize = 25;

/// The most an agent may ask for in one search.
///
/// A cap rather than an error, because an agent asking for a thousand hits
/// wants "as many as you have" and refusing it teaches nothing. A hundred is
/// four screens of the default and still fits a budget alongside per-hit trust.
const MAX_SEARCH_LIMIT: usize = 100;

/// Everything this server offers.
///
/// A surface advertising tools it cannot serve teaches an agent to distrust the
/// manifest, and an agent that distrusts the manifest probes instead — the
/// behaviour a read-only surface least wants to encourage. So a tool appears here
/// only once [`call`] can serve it.
// A declaration table is one thing, not many: splitting it into per-tool
// functions would put the seven descriptions in seven places and make the
// question "what does this server offer" unanswerable by reading one thing.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn tools() -> Vec<ToolDeclaration> {
    vec![
        ToolDeclaration {
            name: RECALL_MEMORY,
            description: "Why this asset is the way it is: decisions, incidents, \
                      caveats and rationale people recorded about it. Each result \
                      says who wrote it, how sure they were, whether the asset has \
                      changed since, and whether anyone disagrees.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset to recall knowledge about.",
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional words to rank against. Omit to get \
                                        everything recorded about the asset.",
                    }
                },
                "required": ["fullyQualifiedName"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: GET_ASSET_CONTEXT,
            description: "Everything the catalog knows about one asset, filtered to \
                      what the caller is permitted to see.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset's fully qualified name, \
                                        e.g. warehouse.retail.public.orders",
                    }
                },
                "required": ["fullyQualifiedName"],
                // An agent that can pass an unrecognised field and be ignored keeps
                // passing it, and the version that gives it meaning changes
                // behaviour nobody asked to change.
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: SEARCH_ASSETS,
            description: "Find assets by name or description. Returns ranked hits, \
                      each with a trust summary saying whether it is certified, \
                      owned, and tested — so a choice between similarly-named \
                      tables can be made on more than the name.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Words to match against names and descriptions.",
                    },
                    "kind": {
                        "type": "string",
                        "description": "Optional asset kind to restrict to, \
                                        e.g. table, column, dashboard.",
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_SEARCH_LIMIT,
                        "description": "How many hits to return. Defaults to 25, \
                                        capped at 100.",
                    }
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: EXPLAIN_LINEAGE,
            description: "Where an asset's data comes from, or what it feeds. Each \
                      hop says who asserted it and how. A chain that runs into \
                      something you may not see stops there and says so — it is \
                      never joined across the gap.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset to walk from.",
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["upstream", "downstream"],
                        "description": "upstream (default) is where the data came \
                                        from; downstream is what it feeds.",
                    }
                },
                "required": ["fullyQualifiedName"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: ANALYZE_IMPACT,
            description: "What a change to this asset would affect: the assets \
                      downstream, the contracts that promise something about them, \
                      and the teams to tell.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset being changed.",
                    }
                },
                "required": ["fullyQualifiedName"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: GET_GOVERNANCE_CONTEXT,
            description: "How this asset must be handled: its classifications, which \
                      columns are masked and why, how long it is retained, and what \
                      you are permitted to do with it. Masked columns are named — \
                      you are told they exist even when you cannot read them.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset to describe the handling rules for.",
                    }
                },
                "required": ["fullyQualifiedName"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: QUERY_GRAPH,
            description: "Ask the graph directly, in SPARQL, for questions the other \
                      tools do not shape. Results are filtered to what you may see. \
                      Prefer the task-shaped tools where one fits — this one makes \
                      you do the traversal yourself.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A SPARQL SELECT or ASK query.",
                    }
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
        },
        ToolDeclaration {
            name: TRAVERSE,
            description: "Walk a bounded neighbourhood outward from a catalog asset — \
                      what is connected to it, within a hop limit. Scoped to catalog \
                      assets only; ask about a domain-pack entity (an invoice, a \
                      guest) through the tool shaped for that finding or evidence \
                      question instead.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The asset to walk outward from.",
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["upstream", "downstream"],
                        "description": "Defaults to upstream.",
                    },
                    "maxHops": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": i64::from(MAX_TRAVERSE_HOPS),
                        "description": "Defaults to 2, capped at 6.",
                    }
                },
                "required": ["fullyQualifiedName"],
                "additionalProperties": false,
            }),
        },
    ]
}

/// Who is calling. `None` is an unauthenticated session.
pub type Principal<'a> = Option<&'a str>;

/// Run one tool call.
///
/// Returns an [`Outcome`] rather than a `Result`: every failure an agent should
/// see is one of the variants. A `Result` would invite the composition root to
/// map errors onto protocol faults, and a protocol fault is distinguishable
/// from a not-found — which is exactly the leak this design prevents.
pub async fn call(
    source: &dyn ContextSource,
    principal: Principal<'_>,
    tool: &str,
    arguments: &serde_json::Value,
) -> Outcome {
    call_within(
        source,
        principal,
        tool,
        arguments,
        budget::TokenBudget::default(),
    )
    .await
}

/// [`call`], with the token budget stated rather than defaulted — Epic 14
/// Slice E.
///
/// Every payload is fitted before it is returned, and **the fitting is done
/// here rather than in each tool** so five tools cannot end up with five
/// truncation orderings. See [`budget::fit`] for the ordering and why it is
/// fixed.
// Likewise a dispatch table. Each arm is short; the length is the tool count.
// Extracting them would hide the one property worth reading this function for
// — that every arm authenticates first and every arm maps absence the same way.
#[allow(clippy::too_many_lines)]
pub async fn call_within(
    source: &dyn ContextSource,
    principal: Principal<'_>,
    tool: &str,
    arguments: &serde_json::Value,
    limit: budget::TokenBudget,
) -> Outcome {
    // Authentication first, **before the tool name is checked**. Replying "no
    // such tool" to an unauthenticated caller tells them which tools exist.
    let Some(principal) = principal else {
        return Outcome::Unauthenticated;
    };

    match tool {
        RECALL_MEMORY => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            // `query` is optional — "everything you know about this table" is a
            // real question — but a `query` of the wrong *type* is a mistake
            // worth naming rather than silently reading as absent, or an agent
            // sending `{"query": ["a","b"]}` gets unranked results and no idea
            // why.
            let query = match optional_text(arguments, "query") {
                Ok(query) => query,
                Err(problem) => return problem,
            };
            match source.recall(principal, fqn, query).await {
                // **Empty is `Recalled`, not `NotFound`.** "Nothing has been
                // written down about this table" and "there is no such table"
                // are opposite statements, and an agent that conflates them
                // fills the silence.
                Ok(Some(memories)) => Outcome::Recalled(memories),
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        GET_ASSET_CONTEXT => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            match source.asset_context(principal, fqn).await {
                Ok(Some(mut context)) => {
                    if let Some(reason) = budget::fit(&mut context, limit) {
                        context.truncated = true;
                        context.truncation_reason = Some(reason);
                    }
                    Outcome::Found(Box::new(context))
                }
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        SEARCH_ASSETS => {
            let query = match required_text(arguments, "query") {
                Ok(query) => query,
                Err(problem) => return problem,
            };
            let kind = match arguments.get("kind") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(kind)) => Some(kind.as_str()),
                Some(_) => {
                    return Outcome::BadRequest("`kind`, when given, must be a string".to_string());
                }
            };
            let how_many = match search_limit(arguments) {
                Ok(how_many) => how_many,
                Err(problem) => return problem,
            };
            match source.search(principal, query, kind, how_many).await {
                Ok(mut results) => {
                    if let Some(reason) = budget::fit(&mut results, limit) {
                        results.truncated = true;
                        results.truncation_reason = Some(reason);
                    }
                    Outcome::Searched(Box::new(results))
                }
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        EXPLAIN_LINEAGE => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            let direction = match direction_of(arguments) {
                Ok(direction) => direction,
                Err(problem) => return problem,
            };
            match source.lineage(principal, fqn, direction).await {
                Ok(Some(mut walk)) => {
                    if let Some(reason) = budget::fit(&mut walk, limit) {
                        walk.truncated = true;
                        walk.truncation_reason = Some(reason);
                    }
                    Outcome::Lineage(Box::new(walk))
                }
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        ANALYZE_IMPACT => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            match source.impact(principal, fqn).await {
                Ok(Some(mut report)) => {
                    if let Some(reason) = budget::fit(&mut report, limit) {
                        report.truncated = true;
                        report.truncation_reason = Some(reason);
                    }
                    Outcome::Impact(Box::new(report))
                }
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        GET_GOVERNANCE_CONTEXT => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            match source.governance(principal, fqn).await {
                Ok(Some(mut context)) => {
                    if let Some(reason) = budget::fit(&mut context, limit) {
                        context.truncated = true;
                        context.truncation_reason = Some(reason);
                    }
                    Outcome::Governance(Box::new(context))
                }
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        QUERY_GRAPH => {
            let query = match required_text(arguments, "query") {
                Ok(query) => query,
                Err(problem) => return problem,
            };
            match source.query_graph(principal, query).await {
                Ok(Ok(mut answer)) => {
                    if budget::fit(&mut answer, limit).is_some() {
                        answer.truncated = true;
                    }
                    Outcome::Bindings(Box::new(answer))
                }
                // The caller's mistake and ours, kept apart: see [`QueryFault`].
                Ok(Err(QueryFault::Malformed(detail))) => Outcome::BadRequest(detail),
                Ok(Err(QueryFault::Unsupported(detail))) => Outcome::Unsupported(detail),
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        TRAVERSE => {
            let fqn = match required_fqn(arguments) {
                Ok(fqn) => fqn,
                Err(problem) => return problem,
            };
            let direction = match direction_of(arguments) {
                Ok(direction) => direction,
                Err(problem) => return problem,
            };
            let max_hops = match traverse_hops(arguments) {
                Ok(max_hops) => max_hops,
                Err(problem) => return problem,
            };
            match source.traverse(principal, fqn, direction, max_hops).await {
                Ok(Some(mut context)) => {
                    if let Some(reason) = budget::fit(&mut context, limit) {
                        context.truncated = true;
                        context.truncation_reason = Some(reason);
                    }
                    Outcome::Traversed(Box::new(context))
                }
                Ok(None) => Outcome::NotFound,
                Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
            }
        }

        _ => Outcome::BadRequest(format!("no tool named `{tool}`")),
    }
}

/// The asset name every asset-shaped tool needs.
///
/// An empty name is a mistake, not a lookup. Passing it through returns
/// `NotFound` and teaches the agent the asset does not exist, when what
/// happened is that it never asked about one.
fn required_fqn(arguments: &serde_json::Value) -> Result<&str, Outcome> {
    required_text(arguments, "fullyQualifiedName")
}

fn required_text<'a>(arguments: &'a serde_json::Value, field: &str) -> Result<&'a str, Outcome> {
    let Some(text) = arguments.get(field).and_then(|value| value.as_str()) else {
        return Err(Outcome::BadRequest(format!(
            "`{field}` is required, as a string"
        )));
    };
    if text.is_empty() {
        return Err(Outcome::BadRequest(format!("`{field}` must not be empty")));
    }
    Ok(text)
}

fn optional_text<'a>(arguments: &'a serde_json::Value, field: &str) -> Result<&'a str, Outcome> {
    match arguments.get(field) {
        None | Some(serde_json::Value::Null) => Ok(""),
        Some(serde_json::Value::String(text)) => Ok(text.as_str()),
        Some(_) => Err(Outcome::BadRequest(format!(
            "`{field}`, when given, must be a string"
        ))),
    }
}

/// Which way to walk.
///
/// **An unrecognised value is refused, never defaulted.** Defaulting turns
/// `"descendants"` — a plausible thing for an agent to try — into an upstream
/// walk, which is the exact opposite of the question asked, returned with no
/// indication that anything was misunderstood.
fn direction_of(arguments: &serde_json::Value) -> Result<Direction, Outcome> {
    match arguments.get("direction") {
        None | Some(serde_json::Value::Null) => Ok(Direction::Upstream),
        Some(serde_json::Value::String(direction)) => match direction.as_str() {
            "upstream" => Ok(Direction::Upstream),
            "downstream" => Ok(Direction::Downstream),
            other => Err(Outcome::BadRequest(format!(
                "`direction` must be \"upstream\" or \"downstream\", not \"{other}\""
            ))),
        },
        Some(_) => Err(Outcome::BadRequest(
            "`direction`, when given, must be a string".to_string(),
        )),
    }
}

/// How many hits to ask for, **capped rather than refused**.
///
/// An agent asking for a thousand means "as many as you have", and an error
/// teaches it only that the tool is fussy. Zero is different: it asks for no
/// answer at all, which is never what anybody meant.
fn search_limit(arguments: &serde_json::Value) -> Result<usize, Outcome> {
    match arguments.get("limit") {
        None | Some(serde_json::Value::Null) => Ok(DEFAULT_SEARCH_LIMIT),
        Some(serde_json::Value::Number(number)) => {
            let Some(asked) = number.as_u64() else {
                return Err(Outcome::BadRequest(
                    "`limit` must be a positive whole number".to_string(),
                ));
            };
            if asked == 0 {
                return Err(Outcome::BadRequest(
                    "`limit` must be at least 1; a limit of zero asks for no answer".to_string(),
                ));
            }
            Ok(usize::try_from(asked)
                .unwrap_or(MAX_SEARCH_LIMIT)
                .min(MAX_SEARCH_LIMIT))
        }
        Some(_) => Err(Outcome::BadRequest(
            "`limit`, when given, must be a number".to_string(),
        )),
    }
}

/// `maxHops` for `traverse` — **capped, never refused**, the same posture
/// [`search_limit`] takes: an agent asking for more than the cap means "as
/// far as you'll go", not a malformed request.
fn traverse_hops(arguments: &serde_json::Value) -> Result<u32, Outcome> {
    match arguments.get("maxHops") {
        None | Some(serde_json::Value::Null) => Ok(DEFAULT_TRAVERSE_HOPS),
        Some(serde_json::Value::Number(number)) => {
            let Some(asked) = number.as_u64() else {
                return Err(Outcome::BadRequest(
                    "`maxHops` must be a positive whole number".to_string(),
                ));
            };
            if asked == 0 {
                return Err(Outcome::BadRequest(
                    "`maxHops` must be at least 1; a walk of zero hops asks for no answer"
                        .to_string(),
                ));
            }
            Ok(u32::try_from(asked)
                .unwrap_or(MAX_TRAVERSE_HOPS)
                .min(MAX_TRAVERSE_HOPS))
        }
        Some(_) => Err(Outcome::BadRequest(
            "`maxHops`, when given, must be a number".to_string(),
        )),
    }
}

impl budget::Fits for AssetContext {
    fn shorten_detail(&mut self) -> bool {
        self.description.take().is_some()
    }

    fn shorten_relations(&mut self) -> bool {
        self.related.pop().is_some()
    }

    /// **The asset itself is never dropped.** A context response with no asset
    /// in it is indistinguishable from `NotFound`, which would be a lie about
    /// something the caller is permitted to see — so the ladder ends here and
    /// an impossible budget returns a small, honest, over-budget answer.
    fn drop_entities(&mut self) -> bool {
        false
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl budget::Fits for SearchResults {
    fn shorten_detail(&mut self) -> bool {
        let mut changed = false;
        for hit in &mut self.hits {
            if hit.snippet.take().is_some() {
                changed = true;
            }
        }
        changed
    }

    /// A hit list has no second tier — every entry is an entity. Stated rather
    /// than left to fall through, so the ladder goes straight to the flagged
    /// loss instead of appearing to have tried something.
    fn shorten_relations(&mut self) -> bool {
        false
    }

    /// Drop from the tail: the ranking put the least relevant there, and
    /// **`total` deliberately does not move** — it is how the caller learns
    /// there is more to ask for.
    fn drop_entities(&mut self) -> bool {
        if self.hits.pop().is_none() {
            return false;
        }
        self.truncated = true;
        true
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl budget::Fits for QueryAnswer {
    fn shorten_detail(&mut self) -> bool {
        false
    }

    fn shorten_relations(&mut self) -> bool {
        false
    }

    fn drop_entities(&mut self) -> bool {
        if self.rows.pop().is_none() {
            return false;
        }
        self.truncated = true;
        true
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl budget::Fits for TraversalContext {
    fn shorten_detail(&mut self) -> bool {
        false
    }

    /// Edges first — every node stays present and nameable, the same
    /// "related-entity lists before entities themselves" ordering
    /// [`budget::fit`]'s own doc comment describes.
    fn shorten_relations(&mut self) -> bool {
        self.edges.pop().is_some()
    }

    /// **Removes the dropped node's own edges too**, so a truncated answer
    /// never names an edge pointing at a node the agent was not told
    /// about — the same invariant `graph_owl_traversal::Subgraph::
    /// without_dangling_edges` enforces server-side, kept here because
    /// this payload is shaped independently of that one.
    fn drop_entities(&mut self) -> bool {
        let Some(dropped) = self.nodes.pop() else {
            return false;
        };
        self.edges
            .retain(|edge| edge.from != dropped.id && edge.to != dropped.id);
        self.truncated = true;
        true
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A catalog that knows two assets: one `alice` may see, and one nobody
    /// may — which is the pair the security test needs.
    struct Fixture {
        /// Every `(principal, fqn)` it was asked about, so a test can assert
        /// the *question* — which the answer alone cannot show.
        asked: Mutex<Vec<(String, String)>>,
        broken: bool,
    }

    impl Fixture {
        fn working() -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                broken: false,
            }
        }
        fn broken() -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                broken: true,
            }
        }
        fn questions(&self) -> Vec<(String, String)> {
            self.asked.lock().expect("lock").clone()
        }
    }

    #[async_trait]
    impl ContextSource for Fixture {
        async fn asset_context(
            &self,
            principal: &str,
            fqn: &str,
        ) -> Result<Option<AssetContext>, SourceError> {
            self.asked
                .lock()
                .expect("lock")
                .push((principal.to_string(), fqn.to_string()));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            // `finance.salaries` exists and nobody may see it. That it exists
            // is the whole point — a fixture where the denied asset is simply
            // absent cannot tell the two answers apart either.
            if principal == "alice" && fqn == "warehouse.orders" {
                return Ok(Some(AssetContext {
                    fully_qualified_name: fqn.to_string(),
                    kind: "table".into(),
                    description: Some("customer orders".into()),
                    related: vec!["warehouse.customers".into()],
                    policy_filtered: false,
                    trust: unknown_trust(),
                    truncated: false,
                    truncation_reason: None,
                }));
            }
            Ok(None)
        }

        async fn recall(
            &self,
            principal: &str,
            fqn: &str,
            query: &str,
        ) -> Result<Option<Vec<MemoryContext>>, SourceError> {
            self.asked
                .lock()
                .expect("lock")
                .push((principal.to_string(), format!("{fqn}|{query}")));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if principal != "alice" {
                return Ok(None);
            }
            match fqn {
                // Visible, and something is recorded about it.
                "warehouse.orders" => Ok(Some(vec![
                    MemoryContext {
                        kind: "decision".into(),
                        content: "Refunds are excluded from revenue.".into(),
                        summary: None,
                        confidence: 1.0,
                        human_authored: true,
                        staleness: None,
                        contradicted: true,
                    },
                    MemoryContext {
                        kind: "incident".into(),
                        content: "The nightly load double-counted refunds.".into(),
                        summary: None,
                        confidence: 0.6,
                        human_authored: false,
                        staleness: Some("the asset has changed in a breaking way".into()),
                        contradicted: false,
                    },
                ])),
                // **Visible, and nothing recorded.** The case that must not
                // collapse into `NotFound`.
                "warehouse.customers" => Ok(Some(Vec::new())),
                _ => Ok(None),
            }
        }

        async fn search(
            &self,
            principal: &str,
            query: &str,
            kind: Option<&str>,
            limit: usize,
        ) -> Result<SearchResults, SourceError> {
            self.asked.lock().expect("lock").push((
                principal.to_string(),
                format!("search:{query}|{}|{limit}", kind.unwrap_or("*")),
            ));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if principal != "alice" || !"warehouse.orders".contains(query) {
                return Ok(SearchResults::default());
            }
            // Three assets match; `alice` may see two. **`total` is the two.**
            Ok(SearchResults {
                hits: vec![
                    SearchHit {
                        fully_qualified_name: "warehouse.orders".into(),
                        kind: "table".into(),
                        snippet: Some("customer orders".repeat(40)),
                        trust: unknown_trust(),
                    },
                    SearchHit {
                        fully_qualified_name: "warehouse.orders_archive".into(),
                        kind: "table".into(),
                        snippet: Some("archived orders".repeat(40)),
                        trust: unknown_trust(),
                    },
                ]
                .into_iter()
                .take(limit)
                .collect(),
                total: 2,
                policy_filtered: true,
                truncated: false,
                truncation_reason: None,
            })
        }

        async fn lineage(
            &self,
            principal: &str,
            fqn: &str,
            direction: Direction,
        ) -> Result<Option<lineage::LineageWalk>, SourceError> {
            self.asked.lock().expect("lock").push((
                principal.to_string(),
                format!("lineage:{fqn}|{direction:?}"),
            ));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if principal != "alice" || fqn != "warehouse.orders" {
                return Ok(None);
            }
            // Two directions with **different answers**, so a test can catch a
            // dispatcher that ignores the parameter.
            let (from, to) = match direction {
                Direction::Upstream => ("warehouse.raw_orders", "warehouse.orders"),
                Direction::Downstream => ("warehouse.orders", "reporting.revenue"),
            };
            Ok(Some(lineage::LineageWalk {
                steps: vec![lineage::LineageStep {
                    from_fqn: from.into(),
                    to_fqn: to.into(),
                    relationship: "feeds".into(),
                    source: "connector".into(),
                    query: Some("select ".to_string() + &"col, ".repeat(200)),
                }],
                policy_filtered: true,
                truncated: false,
                truncation_reason: None,
                depth_reached: 1,
            }))
        }

        async fn impact(
            &self,
            principal: &str,
            fqn: &str,
        ) -> Result<Option<lineage::ImpactReport>, SourceError> {
            self.asked
                .lock()
                .expect("lock")
                .push((principal.to_string(), format!("impact:{fqn}")));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if principal != "alice" || fqn != "warehouse.orders" {
                return Ok(None);
            }
            Ok(Some(lineage::ImpactReport {
                affected_assets: vec!["reporting.revenue".into()],
                affected_contracts: vec!["revenue.freshness".into()],
                owning_teams: vec!["payments".into()],
                policy_filtered: false,
                truncated: false,
                truncation_reason: None,
            }))
        }

        async fn governance(
            &self,
            principal: &str,
            fqn: &str,
        ) -> Result<Option<lineage::GovernanceContext>, SourceError> {
            self.asked
                .lock()
                .expect("lock")
                .push((principal.to_string(), format!("governance:{fqn}")));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if principal != "alice" || fqn != "warehouse.orders" {
                return Ok(None);
            }
            Ok(Some(lineage::GovernanceContext {
                classifications: vec!["PII".into()],
                masked_columns: vec![lineage::MaskedColumn {
                    name: "cust_ssn".into(),
                    reason: "PII.Sensitive".into(),
                }],
                retention: Some("P7Y".into()),
                domain: Some("finance".into()),
                permitted_operations: vec!["read".into()],
                truncated: false,
                truncation_reason: None,
            }))
        }

        async fn query_graph(
            &self,
            principal: &str,
            query: &str,
        ) -> Result<Result<QueryAnswer, QueryFault>, SourceError> {
            self.asked
                .lock()
                .expect("lock")
                .push((principal.to_string(), format!("query:{query}")));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            if query.contains("SERVICE") {
                return Ok(Err(QueryFault::Unsupported(
                    "federated queries (SERVICE) are not implemented; \
                     ask this server only about its own graph"
                        .into(),
                )));
            }
            if !query.contains("SELECT") {
                return Ok(Err(QueryFault::Malformed("expected SELECT or ASK".into())));
            }
            // A principal who may see nothing gets **empty bindings**, not an
            // error: the query ran, and its answer is that there is nothing.
            if principal != "alice" {
                return Ok(Ok(QueryAnswer::default()));
            }
            Ok(Ok(QueryAnswer {
                rows: vec![
                    [("s".to_string(), "warehouse.orders".to_string())]
                        .into_iter()
                        .collect(),
                ],
                truncated: false,
            }))
        }

        async fn traverse(
            &self,
            principal: &str,
            fqn: &str,
            direction: Direction,
            max_hops: u32,
        ) -> Result<Option<TraversalContext>, SourceError> {
            self.asked.lock().expect("lock").push((
                principal.to_string(),
                format!("traverse:{fqn}:{direction:?}:{max_hops}"),
            ));
            if self.broken {
                return Err(SourceError::Unavailable("the database is down".into()));
            }
            // Same pair as `asset_context`: `alice` may see `warehouse.orders`,
            // nobody may see `finance.salaries`, and a denial and an absence
            // are the same `Ok(None)` — this fixture proves the dispatch path
            // cannot tell the two apart from the response alone.
            if principal == "alice" && fqn == "warehouse.orders" {
                return Ok(Some(TraversalContext {
                    nodes: vec![
                        TraversalNode {
                            id: "warehouse.orders".into(),
                        },
                        TraversalNode {
                            id: "warehouse.customers".into(),
                        },
                    ],
                    edges: vec![TraversalEdge {
                        from: "warehouse.orders".into(),
                        to: "warehouse.customers".into(),
                        relationship: "references".into(),
                        derived: false,
                    }],
                    truncated: false,
                    truncation_reason: None,
                }));
            }
            Ok(None)
        }
    }

    /// A trust summary for a context whose trust is not what is under test.
    /// Deliberately the **bare** one — every gap, nothing known — so a test
    /// that accidentally depends on trust reads as suspicious rather than
    /// plausible.
    fn unknown_trust() -> crate::trust::TrustSummary {
        crate::trust::summarise(&crate::trust::Observed::default(), chrono::Utc::now())
    }

    fn args(fqn: &str) -> serde_json::Value {
        serde_json::json!({ "fullyQualifiedName": fqn })
    }

    /// Recall arguments, with an optional query.
    fn recall_args(fqn: &str, query: Option<&str>) -> serde_json::Value {
        match query {
            Some(query) => serde_json::json!({ "fullyQualifiedName": fqn, "query": query }),
            None => serde_json::json!({ "fullyQualifiedName": fqn }),
        }
    }

    mod recall_over_mcp {
        use super::*;

        #[tokio::test]
        async fn recall_returns_what_people_wrote_down() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", Some("refunds")),
            )
            .await;

            let Outcome::Recalled(memories) = outcome else {
                panic!("expected Recalled, got {outcome:?}");
            };
            assert_eq!(memories.len(), 2);
            assert_eq!(memories[0].content, "Refunds are excluded from revenue.");
        }

        // **Every flag that stops an agent overstating a memory has to survive
        // the dispatcher.** A tool that drops them serves confident-looking prose
        // with the caveats removed, which is worse than serving nothing.
        #[tokio::test]
        async fn the_flags_that_qualify_a_memory_reach_the_agent() {
            let source = Fixture::working();

            let Outcome::Recalled(memories) = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", None),
            )
            .await
            else {
                panic!("expected Recalled");
            };

            // A person wrote the first and an agent the second — the distinction
            // that stops an agent reading its own earlier guess back as
            // institutional fact.
            assert!(memories[0].human_authored);
            assert!(!memories[1].human_authored);
            // Fresh is `None`, so the field means something when it is set.
            assert!(memories[0].staleness.is_none());
            assert!(memories[1].staleness.is_some());
            // And the disagreement is visible, so the agent cannot settle it by
            // picking one and saying nothing.
            assert!(memories[0].contradicted);
            assert!(!memories[1].contradicted);
        }

        // **The distinction the whole tool rests on.** "Nothing has been written
        // down about this table" and "there is no such table" are opposite
        // statements, and an agent that conflates them fills the silence with
        // invention.
        #[tokio::test]
        async fn an_asset_with_nothing_recorded_is_not_a_not_found() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.customers", None),
            )
            .await;

            assert_eq!(outcome, Outcome::Recalled(Vec::new()));
        }

        // And the negative that makes the test above about *emptiness*: an asset
        // that is unknown or withheld is still `NotFound`, with nothing to tell
        // the two apart.
        #[tokio::test]
        async fn an_unknown_or_withheld_asset_is_not_found() {
            let source = Fixture::working();

            for fqn in ["warehouse.nonexistent", "finance.salaries"] {
                let outcome = call(
                    &source,
                    Some("alice"),
                    RECALL_MEMORY,
                    &recall_args(fqn, None),
                )
                .await;

                assert_eq!(outcome, Outcome::NotFound, "{fqn}");
            }
        }

        #[tokio::test]
        async fn another_principal_gets_nothing() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("bob"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", None),
            )
            .await;

            assert_eq!(outcome, Outcome::NotFound);
        }

        // Authentication is checked before the tool name, so an unauthenticated
        // caller cannot learn that a recall tool exists.
        #[tokio::test]
        async fn an_unauthenticated_caller_learns_nothing_about_the_tool() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                None,
                RECALL_MEMORY,
                &recall_args("warehouse.orders", None),
            )
            .await;

            assert_eq!(outcome, Outcome::Unauthenticated);
        }

        // An omitted query is a real question — "everything you know about this"
        // — and reaches the source as an empty one rather than being refused.
        #[tokio::test]
        async fn an_omitted_query_is_passed_through_as_empty() {
            let source = Fixture::working();

            call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", None),
            )
            .await;

            assert_eq!(
                source.questions(),
                vec![("alice".to_string(), "warehouse.orders|".to_string())]
            );
        }

        #[tokio::test]
        async fn a_query_that_is_given_reaches_the_source() {
            let source = Fixture::working();

            call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", Some("refunds")),
            )
            .await;

            assert_eq!(
                source.questions(),
                vec![("alice".to_string(), "warehouse.orders|refunds".to_string())]
            );
        }

        // An explicit `null` is "I have no query", which is what omitting it
        // means.
        #[tokio::test]
        async fn an_explicit_null_query_is_treated_as_absent() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &serde_json::json!({ "fullyQualifiedName": "warehouse.orders", "query": null }),
            )
            .await;

            assert!(matches!(outcome, Outcome::Recalled(_)));
        }

        // A `query` of the wrong *type* is named rather than silently read as
        // absent: an agent sending an array would otherwise get unranked results
        // and no idea why, and would keep sending it.
        #[tokio::test]
        async fn a_query_of_the_wrong_type_is_refused_by_name() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &serde_json::json!({
                    "fullyQualifiedName": "warehouse.orders",
                    "query": ["a", "b"],
                }),
            )
            .await;

            let Outcome::BadRequest(detail) = outcome else {
                panic!("expected BadRequest, got {outcome:?}");
            };
            assert!(detail.contains("query"), "{detail}");
        }

        #[tokio::test]
        async fn recall_needs_an_asset_to_recall_about() {
            let source = Fixture::working();

            for arguments in [
                serde_json::json!({}),
                serde_json::json!({ "fullyQualifiedName": "" }),
                serde_json::json!({ "fullyQualifiedName": 7 }),
            ] {
                let outcome = call(&source, Some("alice"), RECALL_MEMORY, &arguments).await;

                assert!(
                    matches!(outcome, Outcome::BadRequest(_)),
                    "{arguments} gave {outcome:?}"
                );
            }
        }

        // "We could not look" and "it is not there" are opposite statements, and
        // an agent that conflates them reports an absence it never checked.
        #[tokio::test]
        async fn a_catalog_that_is_down_is_not_an_absence() {
            let source = Fixture::broken();

            let outcome = call(
                &source,
                Some("alice"),
                RECALL_MEMORY,
                &recall_args("warehouse.orders", None),
            )
            .await;

            assert!(
                matches!(outcome, Outcome::Unavailable(_)),
                "got {outcome:?}"
            );
        }
    }

    mod what_the_manifest_declares {
        use super::*;

        #[test]
        fn every_declared_tool_is_one_that_can_be_called() {
            let declared = tools();
            let names: Vec<&str> = declared.iter().map(|tool| tool.name).collect();

            assert_eq!(
                names,
                vec![
                    RECALL_MEMORY,
                    GET_ASSET_CONTEXT,
                    SEARCH_ASSETS,
                    EXPLAIN_LINEAGE,
                    ANALYZE_IMPACT,
                    GET_GOVERNANCE_CONTEXT,
                    QUERY_GRAPH,
                    TRAVERSE,
                ],
                "the seven read capabilities Epic 14 promises, plus Epic 105 P10's \
                 first intelligence tool, and no others — a tool that appears \
                 here without a dispatch arm teaches an agent to distrust the \
                 manifest, and one that dispatches without appearing here is a \
                 capability no agent will ever find"
            );
        }

        // A tool declared twice, or two tools sharing a name, means the
        // dispatcher's `==` picks one and the other silently never runs.
        #[test]
        fn no_two_tools_share_a_name() {
            let declared = tools();
            let unique: std::collections::HashSet<&str> =
                declared.iter().map(|tool| tool.name).collect();

            assert_eq!(unique.len(), declared.len());
        }

        // The schema is what an agent generates arguments from, so `query` has to
        // be listed as optional rather than merely tolerated — an agent cannot
        // send a field it was never told about.
        #[test]
        fn recall_declares_its_optional_query() {
            let recall = tools()
                .into_iter()
                .find(|tool| tool.name == RECALL_MEMORY)
                .expect("declared");

            assert!(recall.input_schema["properties"]["query"].is_object());
            let required = recall.input_schema["required"]
                .as_array()
                .expect("required");
            assert_eq!(required.len(), 1, "only the asset is required");
            assert_eq!(required[0], "fullyQualifiedName");
        }

        /// The schema is what an agent generates arguments from. A required
        /// field it does not list is a call that fails every time, and the
        /// agent has no way to discover why.
        #[test]
        fn the_schema_names_the_argument_the_tool_actually_reads() {
            let schema = &tools()[0].input_schema;

            assert_eq!(schema["type"], "object");
            assert!(schema["properties"]["fullyQualifiedName"].is_object());
            assert_eq!(schema["required"][0], "fullyQualifiedName");
        }

        #[test]
        fn the_schema_refuses_arguments_it_does_not_declare() {
            assert_eq!(tools()[0].input_schema["additionalProperties"], false);
        }

        #[test]
        fn the_declaration_says_what_the_tool_is_for() {
            assert!(
                tools()[0].description.len() > 20,
                "an agent chooses a tool from this sentence"
            );
        }
    }

    mod who_may_ask {
        use super::*;

        #[tokio::test]
        async fn an_authenticated_caller_gets_what_it_may_see() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Found(context) = outcome else {
                panic!("expected the asset, got {outcome:?}")
            };
            assert_eq!(context.fully_qualified_name, "warehouse.orders");
            assert_eq!(context.kind, "table");
        }

        /// **No principal, no answer** — checked before the tool name, because
        /// replying "no such tool" to an unauthenticated caller tells them
        /// which tools exist.
        #[tokio::test]
        async fn an_unauthenticated_session_is_refused() {
            let source = Fixture::working();

            let outcome = call(&source, None, GET_ASSET_CONTEXT, &args("warehouse.orders")).await;

            assert_eq!(outcome, Outcome::Unauthenticated);
            assert!(
                source.questions().is_empty(),
                "the catalog was queried on behalf of nobody"
            );
        }

        #[tokio::test]
        async fn an_unauthenticated_caller_learns_nothing_from_a_bad_tool_name() {
            let source = Fixture::working();

            let outcome = call(&source, None, "delete_everything", &args("x")).await;

            assert_eq!(
                outcome,
                Outcome::Unauthenticated,
                "the reply distinguished a known tool from an unknown one"
            );
        }

        /// The principal reaches the catalog. A tool filtering on a principal
        /// it never passed down would filter on nothing.
        #[tokio::test]
        async fn the_caller_identity_is_passed_to_the_catalog() {
            let source = Fixture::working();

            call(
                &source,
                Some("bob"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            assert_eq!(
                source.questions()[0],
                ("bob".to_string(), "warehouse.orders".to_string())
            );
        }
    }

    mod absent_and_denied_are_one_answer {
        use super::*;

        /// **The security-relevant test.** `finance.salaries` exists and
        /// `alice` may not see it; `nowhere.at.all` does not exist. Both must
        /// reach the agent as the same answer, or the reply itself tells a
        /// caller which assets exist — the fact the policy withholds.
        #[tokio::test]
        async fn a_denied_asset_and_a_missing_one_are_indistinguishable() {
            let source = Fixture::working();

            let denied = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("finance.salaries"),
            )
            .await;
            let missing = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("nowhere.at.all"),
            )
            .await;

            assert_eq!(denied, Outcome::NotFound);
            assert_eq!(denied, missing);
        }

        /// And the negative that stops "always return `NotFound`" passing: the
        /// same principal, on an asset they may see, gets it.
        #[tokio::test]
        async fn a_permitted_asset_is_still_returned() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            assert!(matches!(outcome, Outcome::Found(_)), "{outcome:?}");
        }

        /// The refusal carries no detail. A message naming the asset defeats
        /// the design even when the variant is right.
        #[tokio::test]
        async fn the_refusal_names_nothing() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("finance.salaries"),
            )
            .await;

            assert!(
                !format!("{outcome:?}").contains("finance"),
                "the refusal named the asset it was hiding: {outcome:?}"
            );
        }

        /// **"We could not look" is not "it is not there."** An agent that
        /// conflates them reports an absence it never checked.
        #[tokio::test]
        async fn an_unreachable_catalog_is_not_reported_as_a_missing_asset() {
            let source = Fixture::broken();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            assert!(matches!(outcome, Outcome::Unavailable(_)), "{outcome:?}");
            assert_ne!(outcome, Outcome::NotFound);
        }
    }

    mod bad_calls {
        use super::*;

        #[tokio::test]
        async fn an_unknown_tool_is_refused_by_name() {
            let source = Fixture::working();

            let outcome = call(&source, Some("alice"), "drop_tables", &args("x")).await;

            let Outcome::BadRequest(detail) = outcome else {
                panic!("expected a bad request")
            };
            assert!(detail.contains("drop_tables"), "{detail}");
        }

        #[tokio::test]
        async fn a_call_without_the_required_argument_is_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &serde_json::json!({}),
            )
            .await;

            assert!(matches!(outcome, Outcome::BadRequest(_)), "{outcome:?}");
        }

        /// An empty name never reaches the catalog. Passing it through returns
        /// `NotFound`, which teaches the agent the asset does not exist when
        /// what happened is that it never named one.
        #[tokio::test]
        async fn an_empty_name_is_a_bad_request_rather_than_a_missing_asset() {
            let source = Fixture::working();

            let outcome = call(&source, Some("alice"), GET_ASSET_CONTEXT, &args("")).await;

            assert!(matches!(outcome, Outcome::BadRequest(_)), "{outcome:?}");
            assert!(
                source.questions().is_empty(),
                "an empty name reached the catalog"
            );
        }

        /// A wrongly-typed argument is refused rather than coerced. Reading
        /// `42` as `"42"` looks up an asset the caller did not name.
        #[tokio::test]
        async fn a_wrongly_typed_argument_is_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &serde_json::json!({ "fullyQualifiedName": 42 }),
            )
            .await;

            assert!(matches!(outcome, Outcome::BadRequest(_)), "{outcome:?}");
            assert!(source.questions().is_empty());
        }
    }

    mod a_filtered_answer_says_so {
        use super::*;

        #[test]
        fn a_context_can_report_that_policy_withheld_something() {
            let filtered = AssetContext {
                fully_qualified_name: "warehouse.orders".into(),
                kind: "table".into(),
                description: None,
                related: vec![],
                policy_filtered: true,
                trust: unknown_trust(),
                truncated: false,
                truncation_reason: None,
            };

            let json = serde_json::to_value(&filtered).expect("serialises");

            assert_eq!(json["policyFiltered"], true);
        }

        /// And it is off when nothing was withheld — a flag that is always set
        /// is a flag nobody reads.
        #[tokio::test]
        async fn an_unfiltered_answer_does_not_claim_to_be_filtered() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Found(context) = outcome else {
                panic!("expected the asset")
            };
            assert!(!context.policy_filtered);
        }

        #[test]
        fn the_wire_shape_is_camel_case_like_the_rest_of_the_api() {
            let json = serde_json::to_value(AssetContext {
                fully_qualified_name: "a.b".into(),
                kind: "table".into(),
                description: None,
                related: vec![],
                policy_filtered: false,
                trust: unknown_trust(),
                truncated: false,
                truncation_reason: None,
            })
            .expect("serialises");

            assert!(json["fullyQualifiedName"].is_string(), "{json}");
            assert!(json.get("fully_qualified_name").is_none(), "{json}");
        }
    }

    /// Epic 14 Slice C — discovery, provenance, and blast radius.
    mod the_discovery_tools {
        use super::*;

        #[tokio::test]
        async fn search_returns_ranked_hits_each_carrying_its_own_trust() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders" }),
            )
            .await;

            let Outcome::Searched(results) = outcome else {
                panic!("expected Searched, got {outcome:?}");
            };
            assert_eq!(results.hits.len(), 2);
            assert_eq!(results.hits[0].fully_qualified_name, "warehouse.orders");
        }

        /// **The total counts what the caller may see, not what matched.**
        ///
        /// A total taken before the policy filter is wrong twice: the agent
        /// pages for results that will never arrive, and the gap between the
        /// two numbers is an exact count of the assets being hidden.
        #[tokio::test]
        async fn the_total_counts_only_what_the_caller_may_see() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders" }),
            )
            .await;

            let Outcome::Searched(results) = outcome else {
                panic!("expected Searched, got {outcome:?}");
            };
            assert_eq!(
                results.total,
                results.hits.len(),
                "three matched, two are visible, and the total is two: {results:?}"
            );
            assert!(results.policy_filtered, "{results:?}");
        }

        /// A search matching nothing is an answer, not a `NotFound` — the same
        /// rule `recall_memory` follows for an asset nobody wrote about.
        #[tokio::test]
        async fn a_search_that_matches_nothing_is_an_empty_answer() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "nothing_matches_this" }),
            )
            .await;

            let Outcome::Searched(results) = outcome else {
                panic!("expected Searched, got {outcome:?}");
            };
            assert!(results.hits.is_empty());
            assert_eq!(results.total, 0);
        }

        #[tokio::test]
        async fn a_limit_beyond_the_cap_is_capped_rather_than_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders", "limit": 10_000 }),
            )
            .await;

            assert!(matches!(outcome, Outcome::Searched(_)), "{outcome:?}");
            assert!(
                source
                    .questions()
                    .iter()
                    .any(|(_, question)| question.ends_with(&format!("|{MAX_SEARCH_LIMIT}"))),
                "the cap reached the source: {:?}",
                source.questions()
            );
        }

        /// Zero is different from "a lot": it asks for no answer at all, which
        /// is never what anybody meant, so it is named rather than served.
        #[tokio::test]
        async fn a_limit_of_zero_is_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders", "limit": 0 }),
            )
            .await;

            let Outcome::BadRequest(why) = outcome else {
                panic!("expected BadRequest, got {outcome:?}");
            };
            assert!(why.contains("limit"), "{why}");
        }

        #[tokio::test]
        async fn lineage_walks_upstream_by_default() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                EXPLAIN_LINEAGE,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Lineage(walk) = outcome else {
                panic!("expected Lineage, got {outcome:?}");
            };
            assert_eq!(walk.steps[0].from_fqn, "warehouse.raw_orders");
        }

        /// The direction reaches the source. A dispatcher that dropped it would
        /// answer "where did this come from" when asked "what does this feed" —
        /// the opposite answer, returned with total confidence.
        #[tokio::test]
        async fn asking_downstream_walks_downstream() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                EXPLAIN_LINEAGE,
                &serde_json::json!({
                    "fullyQualifiedName": "warehouse.orders",
                    "direction": "downstream",
                }),
            )
            .await;

            let Outcome::Lineage(walk) = outcome else {
                panic!("expected Lineage, got {outcome:?}");
            };
            assert_eq!(walk.steps[0].to_fqn, "reporting.revenue");
        }

        /// **An unrecognised direction is refused, not defaulted.** Defaulting
        /// turns a plausible guess like `"descendants"` into an upstream walk —
        /// the opposite of the question, with nothing to say it was
        /// misunderstood.
        #[tokio::test]
        async fn an_unrecognised_direction_is_refused_rather_than_defaulted() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                EXPLAIN_LINEAGE,
                &serde_json::json!({
                    "fullyQualifiedName": "warehouse.orders",
                    "direction": "descendants",
                }),
            )
            .await;

            let Outcome::BadRequest(why) = outcome else {
                panic!("expected BadRequest, got {outcome:?}");
            };
            assert!(why.contains("descendants"), "name what was wrong: {why}");
        }

        #[tokio::test]
        async fn impact_names_the_assets_the_contracts_and_the_teams() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                ANALYZE_IMPACT,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Impact(report) = outcome else {
                panic!("expected Impact, got {outcome:?}");
            };
            assert_eq!(report.affected_assets, vec!["reporting.revenue"]);
            assert_eq!(report.affected_contracts, vec!["revenue.freshness"]);
            assert_eq!(
                report.owning_teams,
                vec!["payments"],
                "a count is a number; a team is an action"
            );
        }

        /// Every new tool inherits Slice A's rule: an asset the caller may not
        /// see and one that does not exist give the same answer.
        #[tokio::test]
        async fn a_denied_asset_is_not_found_on_every_tool() {
            let source = Fixture::working();

            for tool in [
                EXPLAIN_LINEAGE,
                ANALYZE_IMPACT,
                GET_GOVERNANCE_CONTEXT,
                TRAVERSE,
            ] {
                let denied = call(&source, Some("alice"), tool, &args("finance.salaries")).await;
                let absent = call(&source, Some("alice"), tool, &args("no.such.thing")).await;

                assert_eq!(denied, Outcome::NotFound, "{tool}");
                assert_eq!(
                    denied, absent,
                    "{tool} must not distinguish denied from absent"
                );
            }
        }

        /// And so does the authentication rule: no principal, no tool names.
        #[tokio::test]
        async fn an_unauthenticated_caller_learns_nothing_from_any_new_tool() {
            let source = Fixture::working();

            for tool in [
                SEARCH_ASSETS,
                EXPLAIN_LINEAGE,
                ANALYZE_IMPACT,
                GET_GOVERNANCE_CONTEXT,
                QUERY_GRAPH,
                TRAVERSE,
            ] {
                let outcome = call(&source, None, tool, &args("warehouse.orders")).await;
                assert_eq!(outcome, Outcome::Unauthenticated, "{tool}");
            }
            assert!(
                source.questions().is_empty(),
                "nothing reached the catalog: {:?}",
                source.questions()
            );
        }

        /// A catalog that cannot be reached is `Unavailable`, never `NotFound`
        /// — "we could not look" and "it is not there" are opposite statements.
        #[tokio::test]
        async fn an_unreachable_catalog_is_never_reported_as_absence() {
            let source = Fixture::broken();

            for tool in [
                SEARCH_ASSETS,
                EXPLAIN_LINEAGE,
                ANALYZE_IMPACT,
                GET_GOVERNANCE_CONTEXT,
                QUERY_GRAPH,
                TRAVERSE,
            ] {
                let outcome = call(
                    &source,
                    Some("alice"),
                    tool,
                    &serde_json::json!({
                        "fullyQualifiedName": "warehouse.orders",
                        "query": "SELECT * WHERE { ?s ?p ?o }",
                    }),
                )
                .await;
                assert!(
                    matches!(outcome, Outcome::Unavailable(_)),
                    "{tool}: {outcome:?}"
                );
            }
        }
    }

    /// Epic 14 Slice D — handling rules and the escape hatch.
    mod the_governance_tools {
        use super::*;

        /// **A masked column is named, with its reason.** An agent that cannot
        /// see the column exists will not know to ask for access to it.
        #[tokio::test]
        async fn governance_names_masked_columns_rather_than_omitting_them() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_GOVERNANCE_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Governance(context) = outcome else {
                panic!("expected Governance, got {outcome:?}");
            };
            assert_eq!(context.masked_columns.len(), 1);
            assert_eq!(context.masked_columns[0].name, "cust_ssn");
            assert_eq!(
                context.masked_columns[0].reason, "PII.Sensitive",
                "the reason is what routes the access request"
            );
            assert_eq!(context.permitted_operations, vec!["read"]);
        }

        #[tokio::test]
        async fn a_graph_query_returns_bindings() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                QUERY_GRAPH,
                &serde_json::json!({ "query": "SELECT ?s WHERE { ?s ?p ?o }" }),
            )
            .await;

            let Outcome::Bindings(answer) = outcome else {
                panic!("expected Bindings, got {outcome:?}");
            };
            assert_eq!(answer.rows.len(), 1);
        }

        /// **A query the principal cannot answer returns empty, not an error.**
        /// An error would tell them there was something there to be denied.
        #[tokio::test]
        async fn a_query_a_principal_cannot_answer_is_empty_not_an_error() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("mallory"),
                QUERY_GRAPH,
                &serde_json::json!({ "query": "SELECT ?s WHERE { ?s ?p ?o }" }),
            )
            .await;

            let Outcome::Bindings(answer) = outcome else {
                panic!("expected empty Bindings, got {outcome:?}");
            };
            assert!(answer.rows.is_empty());
            assert!(!answer.truncated, "empty is complete, not truncated");
        }

        /// **Unsupported is its own answer.** An agent told "malformed" for a
        /// feature this engine does not implement will rewrite a correct query
        /// until it gives up.
        #[tokio::test]
        async fn an_unsupported_feature_is_distinguished_from_a_malformed_query() {
            let source = Fixture::working();

            let unsupported = call(
                &source,
                Some("alice"),
                QUERY_GRAPH,
                &serde_json::json!({ "query": "SELECT ?s WHERE { SERVICE <http://x> {} }" }),
            )
            .await;
            let malformed = call(
                &source,
                Some("alice"),
                QUERY_GRAPH,
                &serde_json::json!({ "query": "not a query at all" }),
            )
            .await;

            let Outcome::Unsupported(why) = unsupported else {
                panic!("expected Unsupported, got {unsupported:?}");
            };
            assert!(
                why.contains("SERVICE"),
                "say what is unsupported so the agent can route around it: {why}"
            );
            assert!(
                matches!(malformed, Outcome::BadRequest(_)),
                "a query the agent wrote wrongly is its problem to fix: {malformed:?}"
            );
        }
    }

    /// Epic 105 P10 — `traverse()`, the first of the platform doc's eight
    /// intelligence tools.
    mod the_traversal_tool {
        use super::*;
        use crate::budget::Fits;

        #[tokio::test]
        async fn traverse_returns_the_bounded_neighbourhood() {
            let source = Fixture::working();

            let outcome = call(&source, Some("alice"), TRAVERSE, &args("warehouse.orders")).await;

            let Outcome::Traversed(context) = outcome else {
                panic!("expected Traversed, got {outcome:?}");
            };
            assert_eq!(context.nodes.len(), 2);
            assert_eq!(context.edges.len(), 1);
            assert_eq!(context.edges[0].relationship, "references");
            assert!(!context.truncated);
        }

        /// The defaults reach the source unchanged: upstream, two hops — the
        /// same defaults [`direction_of`] and [`traverse_hops`] document.
        #[tokio::test]
        async fn omitted_direction_and_hops_default_to_upstream_and_two() {
            let source = Fixture::working();

            call(&source, Some("alice"), TRAVERSE, &args("warehouse.orders")).await;

            let questions = source.questions();
            assert_eq!(
                questions.last(),
                Some(&(
                    "alice".to_string(),
                    "traverse:warehouse.orders:Upstream:2".to_string()
                ))
            );
        }

        /// An explicit direction and hop count reach the source as asked —
        /// proving the dispatcher forwards them rather than only ever using
        /// the defaults.
        #[tokio::test]
        async fn an_explicit_direction_and_hop_count_reach_the_source() {
            let source = Fixture::working();

            call(
                &source,
                Some("alice"),
                TRAVERSE,
                &serde_json::json!({
                    "fullyQualifiedName": "warehouse.orders",
                    "direction": "downstream",
                    "maxHops": 4,
                }),
            )
            .await;

            let questions = source.questions();
            assert_eq!(
                questions.last(),
                Some(&(
                    "alice".to_string(),
                    "traverse:warehouse.orders:Downstream:4".to_string()
                ))
            );
        }

        /// **Capped, not refused** — the same posture [`search_limit`] takes.
        /// An agent asking for more hops than the cap allows gets the cap, not
        /// an error scolding it for asking.
        #[tokio::test]
        async fn a_hop_count_over_the_cap_is_clamped_not_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                TRAVERSE,
                &serde_json::json!({ "fullyQualifiedName": "warehouse.orders", "maxHops": 99 }),
            )
            .await;

            assert!(matches!(outcome, Outcome::Traversed(_)), "{outcome:?}");
            let questions = source.questions();
            assert_eq!(
                questions.last(),
                Some(&(
                    "alice".to_string(),
                    "traverse:warehouse.orders:Upstream:6".to_string()
                ))
            );
        }

        /// A walk of zero hops asks for no answer at all — refused, not
        /// silently rounded up to one.
        #[tokio::test]
        async fn a_hop_count_of_zero_is_refused() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                TRAVERSE,
                &serde_json::json!({ "fullyQualifiedName": "warehouse.orders", "maxHops": 0 }),
            )
            .await;

            assert!(matches!(outcome, Outcome::BadRequest(_)), "{outcome:?}");
        }

        /// A denied asset and an absent one must be the same `NotFound` — the
        /// property every other tool on this trait already holds, extended to
        /// the newest one.
        #[tokio::test]
        async fn a_denied_asset_is_not_found_same_as_absent() {
            let source = Fixture::working();

            let denied = call(&source, Some("alice"), TRAVERSE, &args("finance.salaries")).await;
            let absent = call(&source, Some("alice"), TRAVERSE, &args("no.such.thing")).await;

            assert_eq!(denied, Outcome::NotFound);
            assert_eq!(
                denied, absent,
                "traverse must not distinguish denied from absent"
            );
        }

        /// **`shorten_detail` is called directly, not through `budget::fit`.**
        /// `TraversalContext` has no prose to shorten, so the method is a
        /// permanent `false` — and `fit`'s own shrink-check absorbs a mutant
        /// that flips it to `true` (nothing changes either way, so the
        /// ladder's next rung runs identically in both cases). That makes
        /// this specific lever unobservable through the dispatcher by
        /// construction, not a gap in the dispatcher tests above; calling it
        /// directly is the only way to state the contract at all.
        #[test]
        fn shorten_detail_never_claims_progress() {
            let mut context = TraversalContext {
                nodes: vec![TraversalNode {
                    id: "a".to_string(),
                }],
                edges: Vec::new(),
                truncated: false,
                truncation_reason: None,
            };

            assert!(!context.shorten_detail());
        }

        /// **`drop_entities`'s dangling-edge cleanup, called directly for the
        /// same reason.** `budget::fit`'s ladder always drains every edge via
        /// `shorten_relations` *before* it ever drops a node, so by the time
        /// a real dispatcher call reaches `drop_entities`, `edges` is already
        /// empty and this method's own retain-filter never runs against a
        /// non-empty list — architecturally, not by test omission.
        #[test]
        fn drop_entities_removes_edges_left_dangling_by_the_dropped_node() {
            let mut context = TraversalContext {
                nodes: vec![
                    TraversalNode {
                        id: "a".to_string(),
                    },
                    TraversalNode {
                        id: "b".to_string(),
                    },
                ],
                edges: vec![
                    TraversalEdge {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        relationship: "feeds".to_string(),
                        derived: false,
                    },
                    TraversalEdge {
                        from: "z".to_string(),
                        to: "c".to_string(),
                        relationship: "feeds".to_string(),
                        derived: false,
                    },
                ],
                truncated: false,
                truncation_reason: None,
            };

            let cut = context.drop_entities();

            assert!(cut);
            // `b` is the last node — the one `Vec::pop` removes — so the edge
            // naming it must go too; the edge naming neither endpoint that
            // was dropped is unrelated and must survive untouched.
            assert_eq!(
                context.nodes,
                vec![TraversalNode {
                    id: "a".to_string()
                }]
            );
            assert_eq!(context.edges.len(), 1, "{:?}", context.edges);
            assert_eq!(context.edges[0].from, "z");
            assert_eq!(context.edges[0].to, "c");
        }
    }

    /// Epic 14 Slice E — the budget, applied through the dispatcher.
    mod the_token_budget {
        use super::*;
        use crate::budget::Fits;

        /// **Edges before nodes, through the real dispatcher** — the same
        /// ladder position `RelationsShortened` occupies everywhere else on
        /// this trait, applied to `traverse`'s own edge list.
        ///
        /// **The budget is measured off the fixture, not guessed** — the
        /// same discipline `budget`'s own tests use: exactly what the
        /// payload costs once its one edge is gone, so this proves the
        /// ordering rather than happening to fit a hand-picked number.
        #[tokio::test]
        async fn a_traversal_over_budget_loses_edges_before_nodes() {
            let source = Fixture::working();
            let max_tokens = {
                let probe = TraversalContext {
                    nodes: vec![
                        TraversalNode {
                            id: "warehouse.orders".to_string(),
                        },
                        TraversalNode {
                            id: "warehouse.customers".to_string(),
                        },
                    ],
                    edges: Vec::new(),
                    truncated: false,
                    truncation_reason: None,
                };
                budget::estimate_tokens(&probe.render())
            };

            let outcome = call_within(
                &source,
                Some("alice"),
                TRAVERSE,
                &args("warehouse.orders"),
                budget::TokenBudget { max_tokens },
            )
            .await;

            let Outcome::Traversed(context) = outcome else {
                panic!("expected Traversed, got {outcome:?}");
            };
            assert_eq!(context.nodes.len(), 2, "every node survives: {context:?}");
            assert!(context.edges.is_empty(), "{context:?}");
            assert_eq!(
                context.truncation_reason,
                Some(budget::TruncationReason::RelationsShortened)
            );
        }

        /// And once every edge is gone, a budget still too small drops nodes
        /// — the same last-resort rung `EntitiesDropped` names everywhere
        /// else, reached here through `traverse` specifically.
        #[tokio::test]
        async fn a_traversal_budget_too_small_for_edges_alone_drops_nodes_too() {
            let source = Fixture::working();

            let outcome = call_within(
                &source,
                Some("alice"),
                TRAVERSE,
                &args("warehouse.orders"),
                budget::TokenBudget { max_tokens: 0 },
            )
            .await;

            let Outcome::Traversed(context) = outcome else {
                panic!("expected Traversed, got {outcome:?}");
            };
            assert!(context.nodes.len() < 2, "{context:?}");
            assert!(context.truncated, "{context:?}");
            assert_eq!(
                context.truncation_reason,
                Some(budget::TruncationReason::EntitiesDropped)
            );
        }

        /// **Detail before entities, through the real dispatcher.** The unit
        /// tests in [`budget`] prove the ladder; this proves it is wired in.
        #[tokio::test]
        async fn a_search_over_budget_loses_snippets_before_hits() {
            let source = Fixture::working();

            let outcome = call_within(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders" }),
                budget::TokenBudget { max_tokens: 400 },
            )
            .await;

            let Outcome::Searched(results) = outcome else {
                panic!("expected Searched, got {outcome:?}");
            };
            assert_eq!(results.hits.len(), 2, "both hits survived: {results:?}");
            assert!(results.hits.iter().all(|hit| hit.snippet.is_none()));
            assert_eq!(
                results.truncation_reason,
                Some(budget::TruncationReason::DetailShortened)
            );
        }

        /// And when a hit must go, **`total` stays put** — it is how the caller
        /// learns there is more to ask for.
        #[tokio::test]
        async fn a_dropped_hit_does_not_change_the_total() {
            let source = Fixture::working();

            let outcome = call_within(
                &source,
                Some("alice"),
                SEARCH_ASSETS,
                &serde_json::json!({ "query": "orders" }),
                budget::TokenBudget { max_tokens: 20 },
            )
            .await;

            let Outcome::Searched(results) = outcome else {
                panic!("expected Searched, got {outcome:?}");
            };
            assert!(results.hits.len() < 2, "{results:?}");
            assert_eq!(results.total, 2, "the total still says two exist");
            assert!(results.truncated);
            assert_eq!(
                results.truncation_reason,
                Some(budget::TruncationReason::EntitiesDropped)
            );
        }

        /// **The asset itself is never dropped.** A context response with no
        /// asset in it is indistinguishable from `NotFound`, which would be a
        /// lie about something the caller may see.
        #[tokio::test]
        async fn no_budget_is_small_enough_to_drop_the_asset_being_asked_about() {
            let source = Fixture::working();

            let outcome = call_within(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
                budget::TokenBudget { max_tokens: 0 },
            )
            .await;

            let Outcome::Found(context) = outcome else {
                panic!("expected the asset even at an impossible budget, got {outcome:?}");
            };
            assert_eq!(context.fully_qualified_name, "warehouse.orders");
            assert!(context.truncated, "and the loss is reported: {context:?}");
        }

        /// The default budget leaves ordinary answers alone. A flag set on
        /// every response is a flag nobody reads.
        #[tokio::test]
        async fn an_ordinary_answer_under_the_default_budget_is_not_truncated() {
            let source = Fixture::working();

            let outcome = call(
                &source,
                Some("alice"),
                GET_ASSET_CONTEXT,
                &args("warehouse.orders"),
            )
            .await;

            let Outcome::Found(context) = outcome else {
                panic!("expected the asset, got {outcome:?}");
            };
            assert!(!context.truncated, "{context:?}");
            assert_eq!(context.truncation_reason, None);
            assert_eq!(context.description, Some("customer orders".to_string()));
        }
    }

    /// Every declared tool must be callable, or the manifest is a lie — and an
    /// agent that finds one advertised tool unserved probes instead of trusting
    /// the rest.
    #[tokio::test]
    async fn every_declared_tool_is_served() {
        let source = Fixture::working();

        for declared in tools() {
            let outcome = call(
                &source,
                Some("alice"),
                declared.name,
                &serde_json::json!({
                    "fullyQualifiedName": "warehouse.orders",
                    "query": "SELECT ?s WHERE { ?s ?p ?o }",
                }),
            )
            .await;

            assert!(
                !matches!(&outcome, Outcome::BadRequest(why) if why.starts_with("no tool named")),
                "{} is advertised and not served",
                declared.name
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_tool_is_still_refused_by_name() {
        let source = Fixture::working();

        let outcome = call(&source, Some("alice"), "drop_everything", &args("a.b")).await;

        let Outcome::BadRequest(why) = outcome else {
            panic!("expected BadRequest, got {outcome:?}");
        };
        assert!(why.contains("drop_everything"), "{why}");
    }
}
