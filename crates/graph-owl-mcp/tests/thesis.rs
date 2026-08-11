//! The thesis test — Epic 14 Slice G.
//!
//! **The claim this epic makes is that an agent, given only MCP access, can
//! answer a real question about an asset it has never seen.** Everything else
//! in the crate is machinery for that claim; this is the test of it.
//!
//! The question has three parts, and they are the three an engineer actually
//! asks before using a table:
//!
//! 1. What feeds this table?
//! 2. Who owns it?
//! 3. Is it safe to query?
//!
//! Two assertions matter more than the answers:
//!
//! - **No part takes more than three tool calls.** This is the design feedback
//!   Slice A's decision 5 asked for. Tools are supposed to be *task*-shaped, not
//!   endpoint-shaped; if answering "who owns this" takes five calls, they are
//!   endpoint-shaped and the agent is doing orchestration the facade should.
//! - **A restricted principal gets a narrower answer, and is told it is
//!   narrower.** An answer that is quietly smaller is a wrong answer.
//!
//! It runs against the [`ContextSource`] port rather than a transport, because
//! the claim is about the *tool surface* — whether the seven capabilities are
//! the right seven and shaped the right way. A JSON-RPC framing would add
//! plumbing to the test and test nothing this does not.

use std::sync::Mutex;

use async_trait::async_trait;
use graph_owl_mcp::{
    ANALYZE_IMPACT, AssetContext, ContextSource, Direction, EXPLAIN_LINEAGE, EvidenceContext,
    FactExplanation, GET_ASSET_CONTEXT, GET_GOVERNANCE_CONTEXT, MemoryContext, Outcome,
    QueryAnswer, QueryFault, RECALL_MEMORY, SEARCH_ASSETS, SearchHit, SearchResults, SourceError,
    TraversalContext, TraversalEdge, TraversalNode, call, lineage, trust,
};

/// A seeded catalog: a warehouse with a small lineage chain, one owner, one
/// certification, and one column somebody else may not see.
struct Seeded {
    /// Who is asking. `alice` may see everything; `contractor` may not see the
    /// staging table in the middle of the chain.
    calls: Mutex<Vec<String>>,
}

impl Seeded {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, what: &str) {
        self.calls.lock().expect("lock").push(what.to_string());
    }

    /// How many tool calls have been made, and reset.
    fn take_count(&self) -> usize {
        let mut calls = self.calls.lock().expect("lock");
        let count = calls.len();
        calls.clear();
        count
    }

    /// `contractor` may not see the staging table. Everybody may see the rest.
    fn may_see(principal: &str, fqn: &str) -> bool {
        !(principal == "contractor" && fqn == "warehouse.staging_orders")
    }

    fn owned_and_certified() -> trust::TrustSummary {
        trust::summarise(
            &trust::Observed {
                owner: Some("payments".to_string()),
                description: Some("customer orders, one row per order".to_string()),
                certified_by: Some("data-governance".to_string()),
                certification_expires_at: Some(chrono::Utc::now() + chrono::Duration::days(90)),
                tests_passing: Some(true),
                tests_last_run_at: Some(chrono::Utc::now()),
                has_lineage: true,
                ..trust::Observed::default()
            },
            chrono::Utc::now(),
        )
    }
}

#[async_trait]
impl ContextSource for Seeded {
    async fn asset_context(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<AssetContext>, SourceError> {
        self.record("asset_context");
        if !Self::may_see(principal, fqn) || fqn != "warehouse.orders" {
            return Ok(None);
        }
        Ok(Some(AssetContext {
            fully_qualified_name: fqn.to_string(),
            kind: "table".to_string(),
            description: Some("customer orders, one row per order".to_string()),
            related: vec!["warehouse.orders.order_id".to_string()],
            policy_filtered: false,
            trust: Self::owned_and_certified(),
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn recall(
        &self,
        _principal: &str,
        _fqn: &str,
        _query: &str,
    ) -> Result<Option<Vec<MemoryContext>>, SourceError> {
        self.record("recall");
        Ok(Some(Vec::new()))
    }

    async fn search(
        &self,
        _principal: &str,
        query: &str,
        _kind: Option<&str>,
        _limit: usize,
    ) -> Result<SearchResults, SourceError> {
        self.record("search");
        if !"warehouse.orders".contains(query) {
            return Ok(SearchResults::default());
        }
        Ok(SearchResults {
            hits: vec![SearchHit {
                fully_qualified_name: "warehouse.orders".to_string(),
                kind: "table".to_string(),
                snippet: Some("customer orders".to_string()),
                trust: Self::owned_and_certified(),
            }],
            total: 1,
            policy_filtered: false,
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
        self.record("lineage");
        if fqn != "warehouse.orders" || direction != Direction::Upstream {
            return Ok(None);
        }
        // raw_orders → staging_orders → orders. `contractor` may not see the
        // middle one, which is what makes this the policy-boundary case.
        let edges = |node: &str| match node {
            "warehouse.orders" => vec![lineage::RawEdge {
                from_fqn: "warehouse.staging_orders".to_string(),
                to_fqn: "warehouse.orders".to_string(),
                relationship: "feeds".to_string(),
                source: "connector".to_string(),
                query: None,
            }],
            "warehouse.staging_orders" => vec![lineage::RawEdge {
                from_fqn: "warehouse.raw_orders".to_string(),
                to_fqn: "warehouse.staging_orders".to_string(),
                relationship: "feeds".to_string(),
                source: "connector".to_string(),
                query: None,
            }],
            _ => Vec::new(),
        };
        Ok(Some(lineage::walk_upstream(
            fqn,
            |node| Self::may_see(principal, node),
            edges,
        )))
    }

    async fn impact(
        &self,
        _principal: &str,
        fqn: &str,
    ) -> Result<Option<lineage::ImpactReport>, SourceError> {
        self.record("impact");
        if fqn != "warehouse.orders" {
            return Ok(None);
        }
        Ok(Some(lineage::ImpactReport {
            affected_assets: vec!["reporting.revenue".to_string()],
            affected_contracts: vec!["revenue.freshness".to_string()],
            owning_teams: vec!["payments".to_string(), "analytics".to_string()],
            policy_filtered: false,
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn governance(
        &self,
        _principal: &str,
        fqn: &str,
    ) -> Result<Option<lineage::GovernanceContext>, SourceError> {
        self.record("governance");
        if fqn != "warehouse.orders" {
            return Ok(None);
        }
        Ok(Some(lineage::GovernanceContext {
            classifications: vec!["PII.Basic".to_string()],
            masked_columns: vec![lineage::MaskedColumn {
                name: "warehouse.orders.customer_email".to_string(),
                reason: "PII.Basic".to_string(),
            }],
            retention: Some("P7Y".to_string()),
            domain: Some("finance".to_string()),
            permitted_operations: vec!["read".to_string()],
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn query_graph(
        &self,
        _principal: &str,
        _query: &str,
    ) -> Result<Result<QueryAnswer, QueryFault>, SourceError> {
        self.record("query_graph");
        Ok(Ok(QueryAnswer::default()))
    }

    async fn run_pack_query(
        &self,
        _principal: &str,
        _pack: &str,
        _name: &str,
        _bindings: &std::collections::BTreeMap<String, String>,
    ) -> Result<Result<Option<QueryAnswer>, QueryFault>, SourceError> {
        self.record("run_pack_query");
        Ok(Ok(None))
    }

    async fn traverse(
        &self,
        principal: &str,
        fqn: &str,
        direction: Direction,
        _max_hops: u32,
    ) -> Result<Option<TraversalContext>, SourceError> {
        self.record("traverse");
        if !Self::may_see(principal, fqn)
            || fqn != "warehouse.orders"
            || direction != Direction::Upstream
        {
            return Ok(None);
        }
        Ok(Some(TraversalContext {
            nodes: vec![
                TraversalNode {
                    id: "warehouse.orders".to_string(),
                },
                TraversalNode {
                    id: "warehouse.staging_orders".to_string(),
                },
            ],
            edges: vec![TraversalEdge {
                from: "warehouse.staging_orders".to_string(),
                to: "warehouse.orders".to_string(),
                relationship: "feeds".to_string(),
                derived: false,
            }],
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn find_evidence(
        &self,
        _principal: &str,
        _finding_id: uuid::Uuid,
        _max_hops: u32,
    ) -> Result<Option<EvidenceContext>, SourceError> {
        self.record("find_evidence");
        Ok(None)
    }

    async fn explain(
        &self,
        _principal: &str,
        _subject: &graph_owl_core::flake::Sid,
        _predicate: &graph_owl_core::flake::Sid,
        _object: &graph_owl_core::flake::Sid,
    ) -> Result<Option<FactExplanation>, SourceError> {
        self.record("explain");
        Ok(None)
    }

    async fn reconcile(
        &self,
        _principal: &str,
        _pack: &str,
    ) -> Result<Option<graph_owl_api::ReconcileOutcome>, SourceError> {
        self.record("reconcile");
        Ok(None)
    }

    async fn analytics(
        &self,
        _principal: &str,
        _fqn: &str,
        _direction: Direction,
        _max_hops: u32,
    ) -> Result<Option<graph_owl_mcp::AnalyticsContext>, SourceError> {
        self.record("analytics");
        Ok(None)
    }

    async fn run_rule(
        &self,
        _principal: &str,
        _pack: &str,
        _label: &str,
    ) -> Result<Option<graph_owl_api::ReconcileOutcome>, SourceError> {
        self.record("run_rule");
        Ok(None)
    }

    async fn resolve_entity(
        &self,
        _principal: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<graph_owl_mcp::ResolvedEntityContext, SourceError> {
        self.record("resolve_entity");
        Ok(graph_owl_mcp::ResolvedEntityContext::default())
    }

    async fn calculate_risk(
        &self,
        _principal: &str,
        _pack: &str,
        _subject: &str,
    ) -> Result<Vec<graph_owl_api::Obligation>, SourceError> {
        self.record("calculate_risk");
        Ok(Vec::new())
    }
}

fn about(fqn: &str) -> serde_json::Value {
    serde_json::json!({ "fullyQualifiedName": fqn })
}

/// **Part one: what feeds this table?** One call.
#[tokio::test]
async fn an_agent_can_learn_what_feeds_a_table_it_has_never_seen() {
    let catalog = Seeded::new();

    let outcome = call(
        &catalog,
        Some("alice"),
        EXPLAIN_LINEAGE,
        &about("warehouse.orders"),
    )
    .await;

    let Outcome::Lineage(walk) = outcome else {
        panic!("expected Lineage, got {outcome:?}");
    };
    let sources: Vec<&str> = walk
        .steps
        .iter()
        .map(|step| step.from_fqn.as_str())
        .collect();
    assert!(
        sources.contains(&"warehouse.staging_orders") && sources.contains(&"warehouse.raw_orders"),
        "the whole chain, for a principal who may see it: {walk:?}"
    );
    assert_eq!(catalog.take_count(), 1, "one call to answer part one");
}

/// **Part two: who owns it?** One call — the same one that answers "what is
/// this", because ownership is context, not a separate lookup. A surface that
/// made this a second call would be endpoint-shaped.
#[tokio::test]
async fn an_agent_can_learn_who_owns_a_table_in_one_call() {
    let catalog = Seeded::new();

    let outcome = call(
        &catalog,
        Some("alice"),
        GET_ASSET_CONTEXT,
        &about("warehouse.orders"),
    )
    .await;

    let Outcome::Found(context) = outcome else {
        panic!("expected Found, got {outcome:?}");
    };
    assert!(
        context.trust.owner_known,
        "ownership rides on the context, not a second lookup: {:?}",
        context.trust
    );
    assert_eq!(catalog.take_count(), 1, "one call to answer part two");
}

/// **Part three: is it safe to query?** Two calls at most — trust says whether
/// it is certified and tested; governance says how it must be handled.
#[tokio::test]
async fn an_agent_can_learn_whether_a_table_is_safe_to_query() {
    let catalog = Seeded::new();

    let context = call(
        &catalog,
        Some("alice"),
        GET_ASSET_CONTEXT,
        &about("warehouse.orders"),
    )
    .await;
    let governance = call(
        &catalog,
        Some("alice"),
        GET_GOVERNANCE_CONTEXT,
        &about("warehouse.orders"),
    )
    .await;

    let Outcome::Found(context) = context else {
        panic!("expected Found, got {context:?}");
    };
    let Outcome::Governance(governance) = governance else {
        panic!("expected Governance, got {governance:?}");
    };
    assert!(
        matches!(
            context.trust.certification,
            trust::Certification::Certified { .. }
        ),
        "certification is what makes 'safe' answerable: {:?}",
        context.trust
    );
    assert_eq!(
        governance.masked_columns.len(),
        1,
        "and the handling rules say which column may not be read: {governance:?}"
    );
    assert_eq!(catalog.take_count(), 2, "two calls to answer part three");
}

/// **The architectural assertion.** All three parts, from a cold start, in no
/// more than three calls each.
///
/// This is the test that would fail if the tools drifted endpoint-shaped —
/// if "who owns this" needed a list, a get, and a resolve. It is deliberately
/// a bound rather than an exact count, because a fourth capability that
/// genuinely helps is fine and a fifth call to assemble one answer is not.
#[tokio::test]
async fn no_part_of_the_question_needs_more_than_three_calls() {
    const BUDGET: usize = 3;
    let catalog = Seeded::new();

    for part in [
        vec![EXPLAIN_LINEAGE],
        vec![GET_ASSET_CONTEXT],
        vec![GET_ASSET_CONTEXT, GET_GOVERNANCE_CONTEXT],
        vec![ANALYZE_IMPACT],
        vec![SEARCH_ASSETS, GET_ASSET_CONTEXT],
    ] {
        for tool in &part {
            let arguments = serde_json::json!({
                "fullyQualifiedName": "warehouse.orders",
                "query": "orders",
            });
            let outcome = call(&catalog, Some("alice"), tool, &arguments).await;
            assert!(
                !matches!(outcome, Outcome::NotFound | Outcome::Unauthenticated),
                "{tool} could not answer: {outcome:?}"
            );
        }
        assert!(
            catalog.take_count() <= BUDGET,
            "{part:?} exceeded the {BUDGET}-call budget — the tools have drifted \
             endpoint-shaped"
        );
    }
}

/// **The same question, a restricted principal: narrower, and flagged.**
///
/// `contractor` may not see the staging table in the middle of the chain. The
/// answer must stop there and say so — not quietly return a shorter chain, and
/// certainly not join `raw_orders` directly to `orders`, which is an edge
/// nobody asserted.
#[tokio::test]
async fn a_restricted_principal_gets_a_narrower_answer_and_is_told_so() {
    let catalog = Seeded::new();

    let outcome = call(
        &catalog,
        Some("contractor"),
        EXPLAIN_LINEAGE,
        &about("warehouse.orders"),
    )
    .await;

    let Outcome::Lineage(walk) = outcome else {
        panic!("expected Lineage, got {outcome:?}");
    };
    assert!(
        walk.policy_filtered,
        "a quietly smaller answer is a wrong answer: {walk:?}"
    );
    assert!(
        !walk.steps.iter().any(
            |step| step.from_fqn == "warehouse.raw_orders" && step.to_fqn == "warehouse.orders"
        ),
        "`raw_orders → orders` is an edge nobody asserted: {walk:?}"
    );
    assert!(
        !serde_json::to_string(&walk)
            .expect("serialize")
            .contains("staging_orders"),
        "and the denied node is not named: {walk:?}"
    );
}

/// The unrestricted answer to the same question **is** wider — otherwise the
/// test above would pass against a surface that returns nothing to anybody.
#[tokio::test]
async fn the_unrestricted_answer_to_the_same_question_is_wider() {
    let catalog = Seeded::new();

    let Outcome::Lineage(open) = call(
        &catalog,
        Some("alice"),
        EXPLAIN_LINEAGE,
        &about("warehouse.orders"),
    )
    .await
    else {
        panic!("expected Lineage for alice")
    };
    let Outcome::Lineage(restricted) = call(
        &catalog,
        Some("contractor"),
        EXPLAIN_LINEAGE,
        &about("warehouse.orders"),
    )
    .await
    else {
        panic!("expected Lineage for contractor")
    };

    assert!(
        open.steps.len() > restricted.steps.len(),
        "open {:?} vs restricted {:?}",
        open.steps,
        restricted.steps
    );
    assert!(!open.policy_filtered, "and alice's answer is not flagged");
}

/// Every part of the question is answerable with **declared** tools only. A
/// capability the manifest does not mention is one no agent will ever use.
#[tokio::test]
async fn every_tool_the_thesis_needs_is_declared() {
    let declared: Vec<&str> = graph_owl_mcp::tools()
        .into_iter()
        .map(|tool| tool.name)
        .collect();

    for needed in [
        SEARCH_ASSETS,
        GET_ASSET_CONTEXT,
        EXPLAIN_LINEAGE,
        ANALYZE_IMPACT,
        GET_GOVERNANCE_CONTEXT,
        RECALL_MEMORY,
    ] {
        assert!(declared.contains(&needed), "{needed} is not declared");
    }
}
