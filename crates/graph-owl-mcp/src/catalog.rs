//! The adapter: `ContextSource` over `Catalog` — Epic 14.
//!
//! Slices A and B were pure and served nothing. This is what makes them
//! reachable, and it carries one obligation the rest of the crate depends on:
//! **the catalog's fields must reach [`Observed`] without defaulting on the
//! way in.** A field that lost its `None` before arriving would have already
//! thrown away the distinction `trust` exists to preserve — and the loss is
//! invisible, because "no tests" and "tests passed" both render as a value.

use async_trait::async_trait;
use graph_owl_api::{Catalog, CatalogError};

use crate::{
    AssetContext, ContextSource, MemoryContext, SourceError, budget,
    trust::{Observed, summarise},
};

/// Serves MCP tools from the catalog, filtered by the caller's policy.
pub struct CatalogContext {
    catalog: Catalog,
    /// **The principal authentication established**, not one re-derived from an
    /// id.
    ///
    /// The port takes `principal: &str` because an implementor that owns its own
    /// identity store needs to resolve one. This implementor does not: the
    /// composition root already authenticated the caller, and re-resolving threw
    /// that away — in open mode it produced a *weaker* principal than the one
    /// that authenticated, because `Principal::system()` is a synthetic
    /// in-process admin while the stored `system` row is deliberately
    /// non-admin (it exists for attribution, not authorisation). The MCP surface
    /// could then read nothing at all.
    ///
    /// Authentication decides who the caller is. Nothing downstream re-decides.
    principal: graph_owl_core::Principal,
    /// Classification names whose tags mean "the values here are restricted".
    ///
    /// **Declared by the deployment, never inferred from a tag's name.** See
    /// [`crate::lineage::masked_columns_of`] for why a string comparison is the
    /// wrong place for a security decision. Empty by default: a deployment that
    /// has said nothing masks nothing.
    masking: std::collections::HashSet<String>,
}

impl CatalogContext {
    #[must_use]
    pub fn new(catalog: Catalog, principal: graph_owl_core::Principal) -> Self {
        Self {
            catalog,
            principal,
            masking: std::collections::HashSet::new(),
        }
    }

    /// Declare which classifications imply masking.
    #[must_use]
    pub fn masking(mut self, classifications: impl IntoIterator<Item = String>) -> Self {
        self.masking = classifications.into_iter().collect();
        self
    }

    /// The authenticated caller, checked against the id the port passed.
    ///
    /// **A mismatch is a refusal, not a silent substitution.** The two can only
    /// disagree if a caller reached this adapter claiming to be somebody other
    /// than the session it authenticated as, and answering for either identity
    /// would be wrong — one impersonates, the other silently ignores what was
    /// asked.
    fn authenticated(&self, claimed: &str) -> Result<graph_owl_core::Principal, SourceError> {
        if claimed != self.principal.id {
            return Err(SourceError::Unavailable(format!(
                "this session authenticated as `{}` and the call named `{claimed}`",
                self.principal.id
            )));
        }
        Ok(self.principal.clone())
    }
}

/// Any catalog failure is "we could not look", never "it is not there".
///
/// `CatalogError` is deliberately not `Display` — it carries field errors a
/// caller is meant to destructure — so the message is built here rather than
/// stringified by accident. The distinction matters more than the text: an
/// agent told `NotFound` for a failed read reports an absence it never checked.
fn unavailable(error: &CatalogError) -> SourceError {
    SourceError::Unavailable(format!("{error:?}"))
}

/// Read one property off an asset's free-form `properties`.
///
/// `None` when absent **or** when present-but-not-a-string. A property holding
/// the wrong type is a property nobody can act on, and coercing it would put a
/// rendered `{"a":1}` where a lifecycle name belongs.
fn text(asset: &graph_owl_core::Asset, key: &str) -> Option<String> {
    asset
        .properties
        .as_ref()?
        .get(key)?
        .as_str()
        .map(ToString::to_string)
}

fn flag(asset: &graph_owl_core::Asset, key: &str) -> Option<bool> {
    asset.properties.as_ref()?.get(key)?.as_bool()
}

fn instant(asset: &graph_owl_core::Asset, key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = text(asset, key)?;
    chrono::DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// What the catalog holds about an asset, without deciding anything.
///
/// Every field is read as an `Option` and handed on as one. **No defaults
/// here**: `trust::summarise` is where absence becomes `Unknown`, and doing it
/// twice — or doing it here instead — is how "nobody tested this" quietly
/// becomes "this passed".
#[must_use]
pub fn observe(asset: &graph_owl_core::Asset, has_lineage: bool) -> Observed {
    Observed {
        lifecycle: text(asset, "lifecycle"),
        successor: text(asset, "successor"),
        // `Asset` carries no owner field today — Epic 11's `owner_id` lives on
        // the row and is not projected here — so this reads the property the
        // connectors write. **When the envelope gains one, read that instead**:
        // a typo in free-form JSON currently looks exactly like an owner.
        owner: text(asset, "owner"),
        description: asset.description.clone(),
        certified_by: text(asset, "certifiedBy"),
        certification_expires_at: instant(asset, "certificationExpiresAt"),
        tests_passing: flag(asset, "testsPassing"),
        tests_last_run_at: instant(asset, "testsLastRunAt"),
        has_lineage,
    }
}

#[async_trait]
impl ContextSource for CatalogContext {
    async fn asset_context(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<AssetContext>, SourceError> {
        let who = self.authenticated(principal)?;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };

        // **Policy decides visibility, and a denial is an absence.** The
        // filtered read is reused rather than a second rule written here: two
        // implementations of "may this principal see it" is one more than can
        // be kept in step, and this is the copy an agent drives.
        //
        // `NotFound` from the facade covers both "no such asset" and "not for
        // you" — which is the property `Outcome::NotFound` rests on.
        if self.catalog.get_asset_for(&who, asset.id).await.is_err() {
            return Ok(None);
        }

        // The difference between what exists and what this principal may see
        // *is* the withheld count. Asking the question twice is the only way to
        // know something was hidden, because a filtered read cannot report what
        // it removed.
        let all = self
            .catalog
            .list_children(Some(asset.id))
            .await
            .map_err(|e| unavailable(&e))?;
        let visible = self
            .catalog
            .list_children_for(&who, Some(asset.id))
            .await
            .map_err(|e| unavailable(&e))?;

        Ok(Some(AssetContext {
            fully_qualified_name: asset.fully_qualified_name.clone(),
            kind: asset.kind.to_string(),
            description: asset.description.clone(),
            related: visible
                .iter()
                .map(|child| child.fully_qualified_name.clone())
                .collect(),
            // **Set when policy withheld something**, not when nothing was
            // found. An agent that cannot tell a complete answer from a
            // filtered one presents the filtered one as complete.
            policy_filtered: visible.len() < all.len(),
            trust: summarise(&observe(&asset, false), chrono::Utc::now()),
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn recall(
        &self,
        principal: &str,
        fqn: &str,
        query: &str,
    ) -> Result<Option<Vec<MemoryContext>>, SourceError> {
        let who = self.authenticated(principal)?;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };
        // **The same visibility gate the context tool uses**, and reused rather
        // than restated: two implementations of "may this principal see it" is one
        // more than can be kept in step, and knowledge about an asset is at least
        // as sensitive as the asset.
        if self.catalog.get_asset_for(&who, asset.id).await.is_err() {
            return Ok(None);
        }

        // Current memories only. A superseded memory reaching an agent as an
        // unmarked peer of its own correction is the worst possible outcome here —
        // it is not stale, it is *withdrawn*, and there is no flag that makes
        // presenting both defensible.
        let recalled = self
            .catalog
            .recall(asset.id, query, false)
            .await
            .map_err(|e| unavailable(&e))?;

        // Read once for the whole set rather than per memory: they are all about
        // the same asset, so the queue is the same queue.
        let conflicts = self
            .catalog
            .contradictions_about(asset.id)
            .await
            .map_err(|e| unavailable(&e))?;
        let disputed: std::collections::HashSet<uuid::Uuid> = conflicts
            .iter()
            .flat_map(|conflict| [conflict.a, conflict.b])
            .collect();

        Ok(Some(
            recalled
                .into_iter()
                .map(|item| MemoryContext {
                    kind: format!("{:?}", item.memory.kind).to_lowercase(),
                    content: item.memory.content.clone(),
                    summary: item.memory.summary.clone(),
                    confidence: item.memory.confidence,
                    human_authored: matches!(
                        item.memory.authorship,
                        graph_owl_core::memory::Authorship::Human { .. }
                    ),
                    staleness: staleness_note(&item.staleness),
                    contradicted: disputed.contains(&item.memory.id),
                })
                .collect(),
        ))
    }

    async fn search(
        &self,
        principal: &str,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<crate::SearchResults, SourceError> {
        let who = self.authenticated(principal)?;

        // An unrecognised kind is an **empty answer, not an error**. The agent
        // asked a well-formed question about a category this catalog does not
        // model, and "nothing of that kind" is the true answer to it.
        let kind = match kind {
            None => None,
            Some(name) => match serde_json::from_value::<graph_owl_core::AssetKind>(
                serde_json::Value::String(name.to_string()),
            ) {
                Ok(kind) => Some(kind),
                Err(_) => return Ok(crate::SearchResults::default()),
            },
        };
        let filter = graph_owl_storage::AssetFilter {
            kind,
            ..graph_owl_storage::AssetFilter::default()
        };
        let page = graph_owl_core::page::PageRequest::new(Some(limit), None)
            .map_err(|error| SourceError::Unavailable(format!("{error:?}")))?;

        let visible = self
            .catalog
            .search_assets_for(&who, query, &filter, &page)
            .await
            .map_err(|e| unavailable(&e))?;
        // The same two-reads trick `asset_context` uses: a filtered read cannot
        // report what it removed, so the only way to know something was hidden
        // is to ask the question twice.
        let all = self
            .catalog
            .search_assets_for(&Self::everything(), query, &filter, &page)
            .await
            .map_err(|e| unavailable(&e))?;

        let hits: Vec<crate::SearchHit> = visible
            .data
            .iter()
            .map(|hit| crate::SearchHit {
                fully_qualified_name: hit.asset.fully_qualified_name.clone(),
                kind: hit.asset.kind.to_string(),
                // The real, highlighted excerpt where the query's own
                // storage-layer `ts_headline` produced one (Phase 2.4);
                // falling back to the whole description preserves this
                // tool's prior behaviour for a match that came from
                // `name`/FQN rather than prose, where there is nothing to
                // highlight but the description is still worth showing.
                snippet: hit
                    .snippet
                    .clone()
                    .or_else(|| hit.asset.description.clone()),
                trust: summarise(&observe(&hit.asset, false), chrono::Utc::now()),
            })
            .collect();

        Ok(crate::SearchResults {
            // **Counted after the filter.** The pre-filter number would tell the
            // agent to page for results that will never arrive, and the gap
            // between the two is an exact count of what is being hidden.
            total: hits.len(),
            policy_filtered: visible.data.len() < all.data.len(),
            hits,
            truncated: false,
            truncation_reason: None,
        })
    }

    async fn lineage(
        &self,
        principal: &str,
        fqn: &str,
        direction: crate::Direction,
    ) -> Result<Option<crate::lineage::LineageWalk>, SourceError> {
        let who = self.authenticated(principal)?;
        let Some(root) = self.visible_asset(&who, fqn).await? else {
            return Ok(None);
        };

        let (nodes, edges) = self.subgraph(root.id, direction).await?;
        let names = Self::name_index(&nodes);
        let visible = self.visible_names(&who, &nodes).await?;

        // **The unfiltered subgraph is read and then walked through the policy
        // rule, rather than filtered in the query.** The rule that matters here
        // is what happens *at* the boundary — a denied node ends the branch
        // instead of being bypassed — and a query that simply omitted the
        // denied rows would silently join across it. See
        // [`crate::lineage::walk_upstream`].
        let by_source: std::collections::HashMap<String, Vec<crate::lineage::RawEdge>> =
            Self::index_edges(&edges, &names, direction);

        Ok(Some(crate::lineage::walk_upstream(
            &root.fully_qualified_name,
            |name| visible.contains(name),
            |node| by_source.get(node).cloned().unwrap_or_default(),
        )))
    }

    async fn impact(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<crate::lineage::ImpactReport>, SourceError> {
        let who = self.authenticated(principal)?;
        let Some(root) = self.visible_asset(&who, fqn).await? else {
            return Ok(None);
        };

        let (nodes, _) = self.subgraph(root.id, crate::Direction::Downstream).await?;
        let visible = self.visible_names(&who, &nodes).await?;

        let mut affected_assets: Vec<String> = nodes
            .iter()
            .filter(|node| node.id != root.id)
            .map(|node| node.fully_qualified_name.clone())
            .filter(|name| visible.contains(name))
            .collect();
        affected_assets.sort();

        // Contracts and teams are read for the **affected** assets, not the root:
        // the promise that breaks is the one made about the thing downstream.
        let mut affected_contracts = Vec::new();
        let mut owning_teams: Vec<String> = Vec::new();
        for node in nodes
            .iter()
            .filter(|node| visible.contains(&node.fully_qualified_name))
        {
            for contract in self
                .catalog
                .list_contracts(Some(&node.fully_qualified_name))
                .await
                .map_err(|e| unavailable(&e))?
            {
                affected_contracts.push(contract.name);
                // The producer promised it; the consumers depend on it. Both
                // need telling, and listing only the producer would leave the
                // people who find out at 3am off the list.
                owning_teams.push(contract.producer);
                owning_teams.extend(contract.consumers);
            }
            owning_teams.extend(node.owners.iter().map(|owner| owner.id.clone()));
        }
        affected_contracts.sort();
        affected_contracts.dedup();
        owning_teams.sort();
        owning_teams.dedup();

        Ok(Some(crate::lineage::ImpactReport {
            // Something downstream exists that the caller may not see, so the
            // blast radius they are shown is smaller than the real one — which
            // they must be told, or they will under-communicate a change.
            policy_filtered: affected_assets.len() + 1 < nodes.len(),
            affected_assets,
            affected_contracts,
            owning_teams,
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn governance(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<crate::lineage::GovernanceContext>, SourceError> {
        let who = self.authenticated(principal)?;
        let Some(asset) = self.visible_asset(&who, fqn).await? else {
            return Ok(None);
        };

        let own_labels = self
            .catalog
            .labels_on(fqn)
            .await
            .map_err(|e| unavailable(&e))?;

        // Columns are read **unfiltered**, because masking is not denial: the
        // parent is visible, so the columns' *existence* is public and only
        // their values are restricted. An agent that cannot see a column exists
        // will not know to ask for access to it.
        let mut column_labels: Vec<(String, String)> = Vec::new();
        for child in self
            .catalog
            .list_children(Some(asset.id))
            .await
            .map_err(|e| unavailable(&e))?
        {
            for label in self
                .catalog
                .labels_on(&child.fully_qualified_name)
                .await
                .map_err(|e| unavailable(&e))?
            {
                column_labels.push((child.fully_qualified_name.clone(), label.tag_fqn));
            }
        }

        let domain = self
            .catalog
            .resolve_asset_domain(asset.id)
            .await
            .map_err(|e| unavailable(&e))?;

        Ok(Some(crate::lineage::GovernanceContext {
            classifications: own_labels
                .iter()
                .map(|label| label.tag_fqn.clone())
                .collect(),
            masked_columns: crate::lineage::masked_columns_of(&column_labels, &self.masking),
            // Retention lives in the organization's own fields until a first-class
            // policy exists; reading it from `extension` is how it is recorded
            // today, and reporting `None` when it is absent is the honest answer.
            retention: asset
                .extension
                .as_ref()
                .and_then(|bag| bag.get("retention"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            domain: domain.map(|assignment| assignment.fully_qualified_name),
            // **What the caller may do, so an agent can plan instead of
            // probing.** This surface is read-only until Epic 32, and saying so
            // is more useful than an empty list an agent has to interpret.
            permitted_operations: vec!["read".to_string()],
            truncated: false,
            truncation_reason: None,
        }))
    }

    async fn query_graph(
        &self,
        principal: &str,
        query: &str,
    ) -> Result<Result<crate::QueryAnswer, crate::QueryFault>, SourceError> {
        let who = self.authenticated(principal)?;

        match self
            .catalog
            .sparql(&who, query, None, graph_owl_api::SparqlBudget::default())
            .await
        {
            Ok(outcome) => Ok(Ok(crate::QueryAnswer {
                rows: outcome.rows,
                truncated: outcome.truncated,
            })),
            // **A query problem is an answer about the query, not a failure to
            // run it.** `Validation` is what the parser rejects; anything else
            // is the engine failing, which the agent must not read as "your
            // query was wrong".
            Err(CatalogError::Validation(problems)) => Ok(Err(crate::QueryFault::Malformed(
                problems
                    .iter()
                    .map(|problem| problem.detail.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
            ))),
            Err(error) => Err(unavailable(&error)),
        }
    }

    async fn traverse(
        &self,
        principal: &str,
        fqn: &str,
        direction: crate::Direction,
        max_hops: u32,
    ) -> Result<Option<crate::TraversalContext>, SourceError> {
        let who = self.authenticated(principal)?;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };

        // `Incoming`/`Outgoing` name which edges the walk follows, not which
        // direction is "up" — `Upstream` (what fed this asset) follows edges
        // that point *into* it.
        let mapped = match direction {
            crate::Direction::Upstream => graph_owl_traversal::Direction::Incoming,
            crate::Direction::Downstream => graph_owl_traversal::Direction::Outgoing,
        };
        let bounds = graph_owl_traversal::Bounds {
            max_hops: max_hops as usize,
            ..graph_owl_traversal::Bounds::default()
        };

        // `asset_subgraph` checks the caller's visibility internally
        // (`00b` decision 7) — a `NotFound` from it covers "no such asset"
        // **and** "not visible to this principal", which must not be told
        // apart here for the same reason [`Outcome::NotFound`] exists
        // everywhere else on this trait.
        let subgraph = match self
            .catalog
            .asset_subgraph(&who, asset.id, mapped, bounds, None)
            .await
        {
            Ok(subgraph) => subgraph,
            Err(CatalogError::NotFound) => return Ok(None),
            Err(error) => return Err(unavailable(&error)),
        };

        Ok(Some(crate::TraversalContext {
            nodes: subgraph
                .nodes
                .iter()
                .map(|sid| crate::TraversalNode { id: sid.id.clone() })
                .collect(),
            edges: subgraph
                .edges
                .iter()
                .map(|edge| crate::TraversalEdge {
                    from: edge.from.id.clone(),
                    to: edge.to.id.clone(),
                    relationship: edge.relationship.clone(),
                    derived: edge.derived,
                })
                .collect(),
            truncated: subgraph.truncated,
            // The graph's own bounds stopped the walk, not the MCP payload
            // budget — `DepthReached` is the one existing reason that means
            // "nothing was measured or cut, the walk simply stopped".
            // `call_within` may still overwrite this with a budget-driven
            // reason if the answer needs shortening further.
            truncation_reason: subgraph
                .truncated
                .then_some(budget::TruncationReason::DepthReached),
        }))
    }

    async fn find_evidence(
        &self,
        principal: &str,
        finding_id: uuid::Uuid,
        max_hops: u32,
    ) -> Result<Option<crate::EvidenceContext>, SourceError> {
        // Authentication only — this route is deliberately not
        // visibility-checked per finding, matching the pre-existing
        // `GET /findings/{id}/evidence-graph` this wraps (see the trait
        // doc comment for why that is not a new gap).
        self.authenticated(principal)?;

        let bounds = graph_owl_traversal::Bounds {
            max_hops: max_hops as usize,
            ..graph_owl_traversal::Bounds::default()
        };

        let graph = match self
            .catalog
            .finding_evidence_graph(finding_id, graph_owl_traversal::Direction::Both, bounds)
            .await
        {
            Ok(graph) => graph,
            Err(CatalogError::NotFound) => return Ok(None),
            Err(error) => return Err(unavailable(&error)),
        };

        // One provenance lookup per node, degrading to "sources unknown"
        // rather than failing the whole answer — the same posture the HTTP
        // route this wraps already takes.
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for sid in &graph.nodes {
            let sources = self.catalog.node_sources(sid).await.unwrap_or_default();
            nodes.push(crate::EvidenceNode {
                id: sid.id.clone(),
                iri: sid.to_iri(),
                sources,
            });
        }

        // Already-reached is excluded rather than duplicated — a node the
        // walk *did* find is not a near miss, it is just a node.
        let near_miss = match self.catalog.near_miss_node(finding_id).await {
            Ok(Some(sid)) if !graph.nodes.contains(&sid) => {
                let sources = self.catalog.node_sources(&sid).await.unwrap_or_default();
                Some(crate::EvidenceNode {
                    id: sid.id.clone(),
                    iri: sid.to_iri(),
                    sources,
                })
            }
            _ => None,
        };

        Ok(Some(crate::EvidenceContext {
            nodes,
            edges: graph
                .edges
                .iter()
                .map(|edge| crate::TraversalEdge {
                    from: edge.from.id.clone(),
                    to: edge.to.id.clone(),
                    relationship: edge.relationship.clone(),
                    derived: edge.derived,
                })
                .collect(),
            near_miss,
            truncated: graph.truncated,
            truncation_reason: graph
                .truncated
                .then_some(budget::TruncationReason::DepthReached),
        }))
    }
}

impl CatalogContext {
    /// A principal that may see everything, used **only** to count what a real
    /// principal could not — never to build a response.
    ///
    /// The count is the whole point: a filtered read cannot report what it
    /// removed, so `policyFiltered` is unanswerable without asking twice.
    fn everything() -> graph_owl_core::Principal {
        graph_owl_core::Principal {
            id: "system".to_string(),
            name: "system".to_string(),
            kind: graph_owl_core::PrincipalKind::Service,
            roles: Vec::new(),
            is_admin: true,
        }
    }

    /// The asset, or `None` when it is absent **or** withheld.
    async fn visible_asset(
        &self,
        who: &graph_owl_core::Principal,
        fqn: &str,
    ) -> Result<Option<graph_owl_core::Asset>, SourceError> {
        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };
        if self.catalog.get_asset_for(who, asset.id).await.is_err() {
            return Ok(None);
        }
        Ok(Some(asset))
    }

    /// The unfiltered lineage subgraph in one direction, to the walk's depth.
    async fn subgraph(
        &self,
        root: uuid::Uuid,
        direction: crate::Direction,
    ) -> Result<
        (
            Vec<graph_owl_core::Asset>,
            Vec<graph_owl_core::lineage::LineageEdge>,
        ),
        SourceError,
    > {
        let depth = crate::lineage::MAX_DEPTH;
        let (up, down) = match direction {
            crate::Direction::Upstream => (depth, 0),
            crate::Direction::Downstream => (0, depth),
        };
        // Epic 37a Slice C: `Catalog::lineage_graph` gained a node budget
        // after being measured, uncapped, taking 25.2s from one
        // well-connected asset at real scale — this MCP-facing walk hits
        // the identical code path and is bounded the same way, at the same
        // default (`graph-owl-server`'s `DEFAULT_LINEAGE_MAX_NODES`; not
        // shared as a constant across the two crates, since neither
        // depends on the other, but the number and its reasoning are one
        // and the same). Discarding `truncated` here rather than wiring it
        // into this adapter's own return shape is a deliberate, narrower
        // scope than the HTTP fix — this call is what stops an agent
        // tying up a request for tens of seconds; surfacing the flag
        // itself through `explain_lineage`'s response is separate,
        // unstarted follow-up work.
        let (nodes, edges, _truncated) = self
            .catalog
            .lineage_graph(root, up, down, 200)
            .await
            .map_err(|e| unavailable(&e))?;
        Ok((nodes, edges))
    }

    fn name_index(
        nodes: &[graph_owl_core::Asset],
    ) -> std::collections::HashMap<uuid::Uuid, String> {
        nodes
            .iter()
            .map(|node| (node.id, node.fully_qualified_name.clone()))
            .collect()
    }

    /// Which of these the principal may see, by name.
    async fn visible_names(
        &self,
        who: &graph_owl_core::Principal,
        nodes: &[graph_owl_core::Asset],
    ) -> Result<std::collections::HashSet<String>, SourceError> {
        let mut visible = std::collections::HashSet::new();
        for node in nodes {
            if self.catalog.get_asset_for(who, node.id).await.is_ok() {
                visible.insert(node.fully_qualified_name.clone());
            }
        }
        Ok(visible)
    }

    /// Edges keyed by the node the walk arrives at, pointing at where it goes
    /// next.
    ///
    /// `walk_upstream` always reads `from_fqn` as "the next node" and `to_fqn`
    /// as "where I am", so a downstream walk **swaps the endpoints** rather
    /// than needing a second walker. One walker means one place the policy
    /// boundary rule can be wrong.
    fn index_edges(
        edges: &[graph_owl_core::lineage::LineageEdge],
        names: &std::collections::HashMap<uuid::Uuid, String>,
        direction: crate::Direction,
    ) -> std::collections::HashMap<String, Vec<crate::lineage::RawEdge>> {
        let mut indexed: std::collections::HashMap<String, Vec<crate::lineage::RawEdge>> =
            std::collections::HashMap::new();
        for edge in edges {
            let (Some(from), Some(to)) =
                (names.get(&edge.from_asset_id), names.get(&edge.to_asset_id))
            else {
                continue;
            };
            let (near, far) = match direction {
                crate::Direction::Upstream => (to, from),
                crate::Direction::Downstream => (from, to),
            };
            indexed
                .entry(near.clone())
                .or_default()
                .push(crate::lineage::RawEdge {
                    from_fqn: far.clone(),
                    to_fqn: near.clone(),
                    relationship: edge.relationship.to_string(),
                    source: format!("{:?}", edge.details.source).to_lowercase(),
                    query: edge.details.query.clone(),
                });
        }
        indexed
    }
}

/// Staleness as a sentence an agent can pass on, or `None` when fresh.
///
/// Words rather than a code, because this text is going into a language model's
/// context and a bare `possiblyStale` gives it nothing to tell a reader. `None`
/// for fresh, so the flag is present exactly when it means something — a field
/// that is always set is a field that gets summarised away.
fn staleness_note(staleness: &graph_owl_core::memory::Staleness) -> Option<String> {
    use graph_owl_core::memory::Staleness;
    match staleness {
        Staleness::Fresh => None,
        Staleness::PossiblyStale { since } => Some(format!(
            "the asset has changed since this was written (now version {}.{}), \
             but only in a backward-compatible way",
            since.major, since.minor
        )),
        Staleness::Stale { since } => Some(format!(
            "the asset has changed in a breaking way since this was written \
             (now version {}.{}); what it describes may no longer exist",
            since.major, since.minor
        )),
        Staleness::SubjectUnknown => Some(
            "the asset this describes could not be resolved, so this could \
                  not be checked against it"
                .to_string(),
        ),
    }
}

/// Serves MCP write tools from the catalog — Epic 32.
///
/// **Separate from [`CatalogContext`] on purpose.** A deployment that wires only
/// the read adapter gets a surface with no code path to write, which is a
/// stronger guarantee than a runtime check.
pub struct CatalogWriter {
    catalog: Catalog,
    /// The authenticated agent — see [`CatalogContext::principal`] for why this
    /// is carried rather than re-derived.
    principal: graph_owl_core::Principal,
}

impl CatalogWriter {
    #[must_use]
    pub fn new(catalog: Catalog, principal: graph_owl_core::Principal) -> Self {
        Self { catalog, principal }
    }
}

#[async_trait]
impl crate::write::WriteSink for CatalogWriter {
    async fn write(
        &self,
        agent_id: &str,
        capability: graph_owl_authz::agent::AgentCapability,
        target_fqn: &str,
        change: serde_json::Value,
        rationale: &str,
        confidence: f64,
    ) -> Result<Result<crate::write::WriteReceipt, String>, SourceError> {
        use graph_owl_authz::agent::{ActivityOutcome, WriteDecision};

        if agent_id != self.principal.id {
            return Err(SourceError::Unavailable(format!(
                "this session authenticated as `{}` and the call named `{agent_id}`",
                self.principal.id
            )));
        }
        let who = self.principal.clone();

        // The facade's gate is the single place capability, scope, expiry and
        // the rate limit are decided, and it records the refusal itself.
        let gated = match self
            .catalog
            .gate_agent_write(&who, capability, target_fqn)
            .await
        {
            Ok(gated) => gated,
            Err(graph_owl_api::CatalogError::AgentRefused(refusal)) => {
                return Ok(Err(refusal.to_string()));
            }
            Err(error) => return Err(unavailable(&error)),
        };

        // **Confidence overrides the grant.** A capability that could apply
        // still proposes when the agent is not confident enough to assert —
        // decision 6, applied after authorization rather than inside it, because
        // they answer different questions.
        let by_confidence = graph_owl_authz::agent::decide_memory_write(confidence);
        let decision =
            if gated.decision == WriteDecision::Apply && by_confidence == WriteDecision::Apply {
                WriteDecision::Apply
            } else {
                WriteDecision::Propose
            };

        let reason = match (gated.decision, by_confidence) {
            (WriteDecision::Apply, WriteDecision::Apply) => format!(
                "`{}` is granted for direct application and confidence {confidence} \
                 is at or above the assertion threshold",
                capability.as_str()
            ),
            (WriteDecision::Apply, WriteDecision::Propose) => format!(
                "confidence {confidence} is below the assertion threshold of {}, \
                 so this became a proposal despite `{}` being granted",
                graph_owl_authz::agent::ASSERTION_CONFIDENCE_THRESHOLD,
                capability.as_str()
            ),
            _ => format!(
                "`{}` proposes rather than applies; a human decides",
                capability.as_str()
            ),
        };

        if decision == WriteDecision::Propose {
            let proposal = self
                .catalog
                .propose_as_agent(&who, capability, target_fqn, change, rationale, confidence)
                .await
                .map_err(|e| unavailable(&e))?;
            return Ok(Ok(crate::write::WriteReceipt {
                outcome: "proposed",
                proposal_id: Some(proposal.id.to_string()),
                target_fqn: target_fqn.to_string(),
                reason,
            }));
        }

        self.apply_directly(&who, capability, target_fqn, &change)
            .await?;
        self.catalog
            .record_agent_write(agent_id, capability, target_fqn, ActivityOutcome::Applied)
            .await
            .map_err(|e| unavailable(&e))?;

        Ok(Ok(crate::write::WriteReceipt {
            outcome: "applied",
            proposal_id: None,
            target_fqn: target_fqn.to_string(),
            reason,
        }))
    }
}

impl CatalogWriter {
    /// The two capabilities that land without a human.
    ///
    /// Written as an exhaustive match on the *granted* capability rather than a
    /// generic applier, so a capability gaining direct-apply status has to be
    /// added here deliberately — there is no path by which a new variant
    /// silently becomes applicable.
    async fn apply_directly(
        &self,
        who: &graph_owl_core::Principal,
        capability: graph_owl_authz::agent::AgentCapability,
        target_fqn: &str,
        change: &serde_json::Value,
    ) -> Result<(), SourceError> {
        use graph_owl_authz::agent::AgentCapability;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(target_fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Err(SourceError::Unavailable(format!(
                "`{target_fqn}` vanished between the gate and the write"
            )));
        };

        match capability {
            AgentCapability::ApplyDescription => {
                let description = change.get("description").and_then(|v| v.as_str());
                self.catalog
                    .update_asset(
                        who,
                        asset.id,
                        &graph_owl_core::AssetUpdate {
                            description: Some(description.map(ToString::to_string)),
                            ..graph_owl_core::AssetUpdate::default()
                        },
                        None,
                    )
                    .await
                    .map_err(|e| unavailable(&e))?;
                Ok(())
            }
            AgentCapability::ApplyTags => {
                let tags = change.get("tags").and_then(|v| v.as_array());
                for tag in tags.into_iter().flatten() {
                    let Some(tag_fqn) = tag.as_str() else {
                        continue;
                    };
                    self.catalog
                        .apply_tag(
                            who,
                            tag_fqn,
                            target_fqn,
                            graph_owl_core::classification::LabelType::Automated,
                            graph_owl_core::classification::LabelState::Suggested,
                        )
                        .await
                        .map_err(|e| unavailable(&e))?;
                }
                Ok(())
            }
            // Unreachable while `decide_write` and this match agree, and stated
            // rather than defaulted so that a disagreement is a loud failure
            // instead of a silent direct write.
            other => Err(SourceError::Unavailable(format!(
                "`{}` is not a direct-apply capability; this is a bug in the \
                 write path, not a refusal",
                other.as_str()
            ))),
        }
    }
}
