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
    AssetContext, ContextSource, MemoryContext, SourceError,
    trust::{Observed, summarise},
};

/// Serves MCP tools from the catalog, filtered by the caller's policy.
pub struct CatalogContext {
    catalog: Catalog,
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
    pub fn new(catalog: Catalog) -> Self {
        Self {
            catalog,
            masking: std::collections::HashSet::new(),
        }
    }

    /// Declare which classifications imply masking.
    #[must_use]
    pub fn masking(mut self, classifications: impl IntoIterator<Item = String>) -> Self {
        self.masking = classifications.into_iter().collect();
        self
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
        // The principal is resolved rather than trusted: an MCP session names a
        // subject, and what that subject may *see* is the catalog's decision,
        // not the protocol's.
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

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
            .map(|asset| crate::SearchHit {
                fully_qualified_name: asset.fully_qualified_name.clone(),
                kind: asset.kind.to_string(),
                snippet: asset.description.clone(),
                trust: summarise(&observe(asset, false), chrono::Utc::now()),
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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;
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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;
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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;
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
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

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
        self.catalog
            .lineage_graph(root, up, down)
            .await
            .map_err(|e| unavailable(&e))
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
