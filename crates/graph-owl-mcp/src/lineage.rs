//! Lineage and impact for agents — Epic 14 Slice C.
//!
//! **The rule this module exists to enforce: a chain broken by policy is
//! reported as broken, never stitched.** If `A → B → C` and the caller may not
//! see `B`, presenting `A → C` fabricates a relationship that does not exist —
//! and it is worse than withholding, because the agent has no way to tell it
//! from a real edge and will reason from it. The honest answer is `A`, plus a
//! flag saying the walk stopped and why.
//!
//! Everything here is pure. Which assets a principal may see is Epic 13's
//! question; what to do at the boundary is this one.

use serde::Serialize;

/// One step in a lineage walk, as an agent receives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageStep {
    pub from_fqn: String,
    pub to_fqn: String,
    /// `feeds` or `derivedFrom` — flow and provenance are different questions
    /// and an agent asked "where did this come from" needs to know which it got.
    pub relationship: String,
    /// **Where this edge came from and how much to believe it.** A connector's
    /// assertion and a human's are both edges; only one of them was checked by
    /// somebody, and an agent that cannot tell will present them alike.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// An edge as the walk receives it, before policy is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RawEdge {
    pub from_fqn: String,
    pub to_fqn: String,
    pub relationship: String,
    pub source: String,
    pub query: Option<String>,
}

/// A bounded, policy-aware lineage walk.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineageWalk {
    pub steps: Vec<LineageStep>,
    /// **Set when the walk stopped at something the caller may not see.**
    ///
    /// Distinct from `truncated`, and the distinction matters: one says "there
    /// is more and you may not have it", the other says "there is more and it
    /// did not fit". An agent should retry the second and escalate the first.
    pub policy_filtered: bool,
    /// Set when the answer is incomplete for any reason other than policy.
    pub truncated: bool,
    /// Why, when it is. Always set when `truncated` is, so an agent that wants
    /// to know whether asking again could help does not have to guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<crate::budget::TruncationReason>,
    /// How deep the walk actually reached.
    pub depth_reached: usize,
}

/// How far a lineage walk goes before it reports truncation.
///
/// Five hops: enough to cross a raw → staging → mart → report chain with a step
/// to spare, and short enough that one tool call stays one bounded answer. An
/// agent that needs more asks again from the far end, which is a better shape
/// than one enormous response it has to page through.
pub const MAX_DEPTH: usize = 5;

/// Walk upstream from `start`, refusing to cross anything the caller cannot see.
///
/// `visible` answers Epic 13's question. `edges_from` supplies one node's
/// incoming edges.
///
/// **Three properties, each of which is a way this could be wrong:**
///
/// 1. **A denied node ends that branch and sets `policy_filtered`.** The walk
///    does not continue past it and does not connect its neighbours — stitching
///    `A → C` across a denied `B` fabricates an edge.
/// 2. **Cycles terminate.** A lineage loop is a modelling mistake somebody will
///    make, and an unbounded walk turns it into a hung tool call.
/// 3. **Depth truncation is reported**, never silent, for the same reason
///    entity loss is reported in [`crate::budget`].
pub fn walk_upstream<V, E>(start: &str, visible: V, edges_from: E) -> LineageWalk
where
    V: Fn(&str) -> bool,
    E: Fn(&str) -> Vec<RawEdge>,
{
    let mut walk = LineageWalk::default();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    seen.insert(start.to_string());
    let mut frontier = vec![start.to_string()];

    for depth in 1..=MAX_DEPTH {
        let mut next = Vec::new();
        for node in &frontier {
            for edge in edges_from(node) {
                // **The boundary.** A denied upstream is not traversed and not
                // reported as an edge — its existence is what the policy
                // withholds, and naming it in a step would leak it.
                if !visible(&edge.from_fqn) {
                    walk.policy_filtered = true;
                    continue;
                }
                walk.steps.push(LineageStep {
                    from_fqn: edge.from_fqn.clone(),
                    to_fqn: edge.to_fqn.clone(),
                    relationship: edge.relationship.clone(),
                    source: edge.source.clone(),
                    query: edge.query.clone(),
                });
                // A node already seen is a cycle or a diamond; either way it
                // needs no second expansion.
                if seen.insert(edge.from_fqn.clone()) {
                    next.push(edge.from_fqn.clone());
                }
            }
        }
        if next.is_empty() {
            walk.depth_reached = depth.saturating_sub(1).max(walk.depth_reached);
            return walk;
        }
        walk.depth_reached = depth;
        if depth == MAX_DEPTH {
            // There is more, and it did not fit. Said out loud, because an
            // agent that believes it has the whole chain stops asking.
            walk.truncated = !next.is_empty();
            if walk.truncated {
                walk.truncation_reason = Some(crate::budget::TruncationReason::DepthReached);
            }
            return walk;
        }
        frontier = next;
    }
    walk
}

/// What a change to an asset would affect.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImpactReport {
    /// Downstream assets, nearest first.
    pub affected_assets: Vec<String>,
    /// Contracts that promise something about them — the parties who would find
    /// out the hard way.
    pub affected_contracts: Vec<String>,
    /// Teams to tell. **Named**, because "twelve assets affected" is a number
    /// and "the payments team owns four of them" is an action.
    pub owning_teams: Vec<String>,
    pub policy_filtered: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<crate::budget::TruncationReason>,
}

/// A masked column, reported **as masked** rather than omitted.
///
/// **Omitting it is the tempting mistake and the wrong one.** An agent that
/// cannot see a column exists will not know to ask for access to it, will not
/// mention it to the person who could, and will confidently describe a table as
/// having fewer columns than it has. Saying "there is a column here and you may
/// not see its values" is strictly more useful and leaks nothing the schema
/// does not already imply.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskedColumn {
    pub name: String,
    /// Why, so an agent can tell "PII" from "commercially sensitive" and route
    /// the request to the right person.
    pub reason: String,
}

/// What an agent may know about how this asset is governed.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceContext {
    /// Tags and classifications applied, so an agent can reason about handling
    /// rather than merely being refused.
    pub classifications: Vec<String>,
    /// Columns present but withheld — see [`MaskedColumn`].
    pub masked_columns: Vec<MaskedColumn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// What the caller may *do*, so an agent can plan instead of probing.
    pub permitted_operations: Vec<String>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_reason: Option<crate::budget::TruncationReason>,
}

/// Which of an asset's columns are masked, given the labels on them.
///
/// **Masking and denial are different mechanisms, and this is the line between
/// them.** A *denied* asset's existence is withheld — asked for by name it
/// returns [`crate::Outcome::NotFound`], indistinguishable from one that never
/// existed, and nothing here changes that. A *masked* column's existence is
/// public and its values are restricted; saying so leaks nothing, because the
/// label declaring it sensitive is already readable.
///
/// `masking` is **supplied by the deployment, not guessed from tag names.**
/// There is no `masks: bool` on a classification yet, and inferring one from a
/// name like `PII` would put a security decision in a string comparison that no
/// plan justifies. An empty set means nothing is masked, which is the correct
/// default for a deployment that has not said otherwise.
#[must_use]
pub fn masked_columns_of<S: std::hash::BuildHasher>(
    labels: &[(String, String)],
    masking: &std::collections::HashSet<String, S>,
) -> Vec<MaskedColumn> {
    let mut masked: Vec<MaskedColumn> = Vec::new();
    for (column_fqn, tag_fqn) in labels {
        let Some(classification) = graph_owl_core::classification::classification_of(tag_fqn)
        else {
            continue;
        };
        if !masking.contains(classification) {
            continue;
        }
        // One entry per column, not per label: two masking tags on one column
        // is still one column, and an agent shown it twice will report two.
        if masked.iter().any(|entry| entry.name == *column_fqn) {
            continue;
        }
        masked.push(MaskedColumn {
            name: column_fqn.clone(),
            // The tag, not the classification: `PII.Sensitive` and `PII.Basic`
            // route to different conversations, and the whole point of the
            // reason is that somebody can act on it.
            reason: tag_fqn.clone(),
        });
    }
    masked
}

/// **Two surviving mutants here are equivalent, and that is the finding.**
///
/// `cargo mutants` reports `shorten_detail -> true` and `drop_entities -> true`
/// as MISSED. Neither is a test gap: a lever that reports a cut it did not make
/// is now absorbed by [`crate::budget::drain`]'s progress check, which sees the
/// payload has not shrunk and stops without crediting a truncation. The observable
/// behaviour is identical, so no behavioural test can distinguish them — killing
/// them would mean asserting how many times a lever was *called*, which is
/// testing the implementation rather than the contract.
///
/// Worth recording rather than chasing: before the progress check these same two
/// mutants were TIMEOUTs — an infinite loop. The fix converted a hang into an
/// equivalence, which is the outcome that was wanted.
impl crate::budget::Fits for GovernanceContext {
    fn shorten_detail(&mut self) -> bool {
        // Retention and domain are single short strings and are *rules*; there
        // is no prose here worth shedding.
        false
    }

    /// Classifications are the only thing that may go, and they go one at a
    /// time from the end.
    fn shorten_relations(&mut self) -> bool {
        if self.classifications.pop().is_none() {
            return false;
        }
        self.truncated = true;
        true
    }

    /// **Nothing else may be dropped, at any budget.**
    ///
    /// A masked column omitted to save tokens is precisely the failure this
    /// type exists to prevent — the agent then cannot see the column exists,
    /// so it never asks for access and confidently describes the table as
    /// having fewer columns than it has. `permittedOperations` is the same
    /// argument: an agent that cannot see what it may do probes to find out.
    ///
    /// So the ladder ends here, and an impossible budget returns a small,
    /// honest, over-budget answer rather than a plausible false one.
    fn drop_entities(&mut self) -> bool {
        false
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl crate::budget::Fits for LineageWalk {
    /// The SQL that produced an edge is the prose here — useful, and the first
    /// thing worth losing, because an agent can ask for one edge's query by
    /// naming the edge.
    fn shorten_detail(&mut self) -> bool {
        let mut changed = false;
        for step in &mut self.steps {
            if step.query.take().is_some() {
                changed = true;
            }
        }
        changed
    }

    /// A walk has no second tier: every step *is* an entity relationship. Said
    /// explicitly rather than left to fall through, so the ladder in
    /// [`crate::budget::fit`] moves straight to the honest, flagged loss.
    fn shorten_relations(&mut self) -> bool {
        false
    }

    /// Drop from the far end — the near edges are the ones the caller asked
    /// about, and a chain cut at the distant end still answers "what feeds this
    /// directly".
    fn drop_entities(&mut self) -> bool {
        if self.steps.pop().is_none() {
            return false;
        }
        self.truncated = true;
        self.truncation_reason = Some(crate::budget::TruncationReason::EntitiesDropped);
        true
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl crate::budget::Fits for ImpactReport {
    fn shorten_detail(&mut self) -> bool {
        false
    }

    /// **The affected assets are this report's "relations", and the contracts
    /// and teams are its entities** — an inversion of the usual mapping, and
    /// deliberate. "Twelve assets affected" is a number; "the payments team
    /// owns four of them and contract X promises freshness on two" is what
    /// someone acts on. So the long list of names goes first.
    fn shorten_relations(&mut self) -> bool {
        if self.affected_assets.pop().is_none() {
            return false;
        }
        self.truncated = true;
        true
    }

    fn drop_entities(&mut self) -> bool {
        // Contracts before teams: a team that is never told cannot act at all,
        // which is a worse outcome than a contract the agent has to look up.
        if self.affected_contracts.pop().is_some() || self.owning_teams.pop().is_some() {
            self.truncated = true;
            return true;
        }
        false
    }

    fn render(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{Fits, TokenBudget, TruncationReason, fit};

    fn edge(from: &str, to: &str) -> RawEdge {
        RawEdge {
            from_fqn: from.to_string(),
            to_fqn: to.to_string(),
            relationship: "feeds".to_string(),
            source: "connector".to_string(),
            query: None,
        }
    }

    /// `a → b → c`, walked upstream from `c`.
    fn chain() -> impl Fn(&str) -> Vec<RawEdge> {
        |node: &str| match node {
            "c" => vec![edge("b", "c")],
            "b" => vec![edge("a", "b")],
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_visible_chain_is_walked_whole() {
        let walk = walk_upstream("c", |_| true, chain());

        assert_eq!(walk.steps.len(), 2, "{walk:?}");
        assert!(!walk.policy_filtered);
        assert!(!walk.truncated);
    }

    /// **The security test this module exists for.** With `b` denied, the walk
    /// must not present `a → c`: that edge does not exist, and an agent given
    /// it will reason from a relationship nobody asserted.
    #[test]
    fn a_chain_broken_by_policy_is_reported_broken_never_stitched() {
        let walk = walk_upstream("c", |node| node != "b", chain());

        assert!(
            walk.policy_filtered,
            "the caller must be told the walk stopped: {walk:?}"
        );
        assert!(
            walk.steps.is_empty(),
            "nothing may be reported past the boundary: {walk:?}"
        );
        assert!(
            !walk
                .steps
                .iter()
                .any(|step| step.from_fqn == "a" && step.to_fqn == "c"),
            "`a → c` is an edge nobody asserted: {walk:?}"
        );
    }

    /// And the denied node is not named in the result — naming it would leak
    /// the existence the policy withholds, which is the same rule
    /// `Outcome::NotFound` follows.
    #[test]
    fn a_denied_node_is_not_named_in_the_walk() {
        let walk = walk_upstream("c", |node| node != "b", chain());

        let rendered = serde_json::to_string(&walk).expect("serialize");
        assert!(!rendered.contains("\"b\""), "{rendered}");
    }

    /// **A cycle terminates.** A lineage loop is a modelling mistake somebody
    /// will make, and an unbounded walk turns it into a hung tool call rather
    /// than an answer.
    #[test]
    fn a_cyclic_graph_terminates() {
        let cyclic = |node: &str| match node {
            "a" => vec![edge("b", "a")],
            "b" => vec![edge("a", "b")],
            _ => Vec::new(),
        };

        let walk = walk_upstream("a", |_| true, cyclic);

        assert!(walk.steps.len() <= 4, "it stopped: {walk:?}");
    }

    /// Depth truncation is reported rather than silent — an agent that believes
    /// it has the whole chain stops asking.
    #[test]
    fn a_walk_deeper_than_the_bound_reports_truncation() {
        // A chain longer than MAX_DEPTH: every node has one parent.
        let deep = |node: &str| {
            let n: usize = node.trim_start_matches('n').parse().unwrap_or(0);
            vec![edge(&format!("n{}", n + 1), node)]
        };

        let walk = walk_upstream("n0", |_| true, deep);

        assert!(walk.truncated, "{walk:?}");
        assert_eq!(
            walk.truncation_reason,
            Some(TruncationReason::DepthReached),
            "an agent deciding whether to ask again needs to know which limit \
             it hit: {walk:?}"
        );
        assert_eq!(walk.depth_reached, MAX_DEPTH);
        assert_eq!(walk.steps.len(), MAX_DEPTH);
    }

    /// Budget pressure on a walk shortens the per-edge SQL before it drops a
    /// hop — the same ordering [`crate::budget`] enforces everywhere, checked
    /// against a real payload rather than only the test double.
    #[test]
    fn a_walk_over_budget_loses_its_queries_before_a_hop() {
        let with_queries = |node: &str| {
            let n: usize = node.trim_start_matches('n').parse().unwrap_or(0);
            vec![RawEdge {
                from_fqn: format!("n{}", n + 1),
                to_fqn: node.to_string(),
                relationship: "feeds".to_string(),
                source: "connector".to_string(),
                query: Some("select ".to_string() + &"column_name, ".repeat(60)),
            }]
        };
        let mut walk = walk_upstream("n0", |_| true, with_queries);
        let hops = walk.steps.len();

        let reason = fit(&mut walk, TokenBudget { max_tokens: 400 });

        assert_eq!(reason, Some(TruncationReason::DetailShortened));
        assert_eq!(walk.steps.len(), hops, "every hop survived: {walk:?}");
        assert!(walk.steps.iter().all(|step| step.query.is_none()));
    }

    /// And when even that is not enough, the dropped hop is **flagged**. A
    /// shortened chain presented as complete is a wrong answer, not a small one.
    #[test]
    fn a_walk_that_must_drop_a_hop_says_so() {
        let mut walk = walk_upstream("c", |_| true, chain());

        let reason = fit(&mut walk, TokenBudget { max_tokens: 10 });

        assert_eq!(reason, Some(TruncationReason::EntitiesDropped));
        assert!(walk.truncated, "{walk:?}");
        assert_eq!(
            walk.truncation_reason,
            Some(TruncationReason::EntitiesDropped)
        );
    }

    /// **Impact keeps the teams longest.** Losing the list of affected assets
    /// leaves a report someone can still act on; losing the owning teams leaves
    /// a report nobody is told about.
    #[test]
    fn an_impact_report_sheds_asset_names_before_owning_teams() {
        let mut report = ImpactReport {
            affected_assets: (0..40).map(|n| format!("svc.db.public.t{n}")).collect(),
            affected_contracts: vec!["orders.freshness".to_string()],
            owning_teams: vec!["payments".to_string()],
            policy_filtered: false,
            truncated: false,
            truncation_reason: None,
        };

        // Measured off the report, not guessed: the cost once every asset name
        // has gone but the contracts and teams remain. A hand-picked number
        // would fail for a reason unrelated to the ordering under test.
        let max_tokens = {
            let probe = ImpactReport {
                affected_assets: Vec::new(),
                affected_contracts: vec!["orders.freshness".to_string()],
                owning_teams: vec!["payments".to_string()],
                policy_filtered: false,
                truncated: true,
                truncation_reason: None,
            };
            crate::budget::estimate_tokens(&probe.render())
        };

        let reason = fit(&mut report, TokenBudget { max_tokens });

        assert_eq!(reason, Some(TruncationReason::RelationsShortened));
        assert_eq!(report.owning_teams, vec!["payments".to_string()]);
        assert_eq!(
            report.affected_contracts,
            vec!["orders.freshness".to_string()]
        );
        assert!(report.truncated, "{report:?}");
    }

    /// **Contracts go before teams, and only once the asset names are gone.**
    ///
    /// The rung below the test above: when shedding every asset name still is
    /// not enough, a contract the agent can look up is a smaller loss than a
    /// team that is never told. Mutation testing missed this entirely — the
    /// ordering test above stops one rung short and never reaches
    /// `drop_entities`.
    #[test]
    fn an_impact_report_sheds_contracts_before_teams() {
        let mut report = ImpactReport {
            affected_assets: vec!["svc.db.public.t0".to_string()],
            affected_contracts: vec!["orders.freshness".to_string()],
            owning_teams: vec!["payments".to_string()],
            policy_filtered: false,
            truncated: false,
            truncation_reason: None,
        };

        let reason = fit(&mut report, TokenBudget { max_tokens: 0 });

        assert_eq!(reason, Some(TruncationReason::EntitiesDropped));
        assert!(report.affected_assets.is_empty(), "{report:?}");
        assert!(
            report.affected_contracts.is_empty(),
            "the contract gave way first: {report:?}"
        );
        assert!(
            report.owning_teams.is_empty(),
            "and at an impossible budget even the teams go — but last: {report:?}"
        );
    }

    /// The rung ordering *within* `drop_entities`: with a contract still
    /// present, one pull takes the contract and leaves the team.
    #[test]
    fn one_drop_takes_the_contract_and_leaves_the_team() {
        let mut report = ImpactReport {
            affected_contracts: vec!["orders.freshness".to_string()],
            owning_teams: vec!["payments".to_string()],
            ..ImpactReport::default()
        };

        assert!(report.drop_entities());

        assert!(report.affected_contracts.is_empty(), "{report:?}");
        assert_eq!(
            report.owning_teams,
            vec!["payments".to_string()],
            "a team that is never told cannot act at all: {report:?}"
        );
        assert!(report.truncated);
    }

    /// A report with nothing left says so, rather than reporting a cut it did
    /// not make — which is what `fit`'s termination depends on.
    #[test]
    fn an_exhausted_impact_report_reports_no_further_cut() {
        let mut report = ImpactReport::default();

        assert!(!report.drop_entities());
        assert!(!report.shorten_relations());
        assert!(!report.shorten_detail());
        assert!(
            !report.truncated,
            "and a lever that cut nothing does not flag a truncation"
        );
    }

    /// **`render` must render.** A `render` returning `null` costs two tokens,
    /// so every governance response would measure as fitting any budget and the
    /// ladder would never run — a truncation bug that reports success.
    #[test]
    fn a_governance_context_renders_its_own_content() {
        let context = GovernanceContext {
            classifications: vec!["PII.Basic".to_string()],
            masked_columns: vec![MaskedColumn {
                name: "svc.db.t.ssn".to_string(),
                reason: "PII.Sensitive".to_string(),
            }],
            retention: Some("P7Y".to_string()),
            domain: Some("finance".to_string()),
            permitted_operations: vec!["read".to_string()],
            truncated: false,
            truncation_reason: None,
        };

        let rendered = context.render();

        assert_eq!(rendered["maskedColumns"][0]["name"], "svc.db.t.ssn");
        assert_eq!(rendered["retention"], "P7Y");
        assert!(
            crate::budget::estimate_tokens(&rendered) > 10,
            "a payload that measures as nearly free is one the budget cannot \
             see: {rendered}"
        );
    }

    /// Same argument for the other two payloads.
    #[test]
    fn a_walk_and_an_impact_report_render_their_own_content() {
        let walk = walk_upstream("c", |_| true, chain());
        let rendered = walk.render();
        assert_eq!(rendered["steps"][0]["fromFqn"], "b");

        let report = ImpactReport {
            affected_assets: vec!["reporting.revenue".to_string()],
            ..ImpactReport::default()
        };
        assert_eq!(report.render()["affectedAssets"][0], "reporting.revenue");
    }

    /// A walk with nothing left to shed reports that it cannot shed more,
    /// rather than looping.
    #[test]
    fn an_empty_walk_has_no_levers_left() {
        let mut walk = LineageWalk::default();

        assert!(!walk.shorten_detail());
        assert!(!walk.shorten_relations());
        assert!(!walk.drop_entities());
    }

    /// And a walk that finishes inside the bound does **not** claim truncation,
    /// or the flag would be noise an agent learns to ignore.
    #[test]
    fn a_walk_that_ends_naturally_does_not_claim_truncation() {
        let walk = walk_upstream("c", |_| true, chain());

        assert!(!walk.truncated, "{walk:?}");
    }

    /// An asset with no upstream is a real answer, not an error.
    #[test]
    fn an_asset_with_no_upstream_walks_to_nothing() {
        let walk = walk_upstream("a", |_| true, |_: &str| Vec::new());

        assert!(walk.steps.is_empty());
        assert!(!walk.policy_filtered);
        assert!(!walk.truncated);
    }

    /// A diamond — two paths to one ancestor — reports both edges but expands
    /// the shared node once.
    #[test]
    fn a_diamond_reports_both_edges_and_expands_the_shared_node_once() {
        let diamond = |node: &str| match node {
            "d" => vec![edge("b", "d"), edge("c", "d")],
            "b" | "c" => vec![edge("a", node)],
            _ => Vec::new(),
        };

        let walk = walk_upstream("d", |_| true, diamond);

        assert_eq!(walk.steps.len(), 4, "both sides of the diamond: {walk:?}");
    }

    /// Provenance rides on every step: a connector's assertion and a human's
    /// are both edges, and only one of them was checked by somebody.
    #[test]
    fn every_step_carries_its_source() {
        let walk = walk_upstream("c", |_| true, chain());

        assert!(
            walk.steps.iter().all(|step| !step.source.is_empty()),
            "{walk:?}"
        );
    }

    // ---- governance ----

    /// **A masked column is reported as masked, not omitted.** An agent that
    /// cannot see a column exists will not know to ask for access to it.
    #[test]
    fn a_masked_column_is_named_with_its_reason() {
        let context = GovernanceContext {
            masked_columns: vec![MaskedColumn {
                name: "cust_ssn".to_string(),
                reason: "PII.Sensitive".to_string(),
            }],
            ..GovernanceContext::default()
        };

        let json = serde_json::to_value(&context).expect("serialize");

        assert_eq!(json["maskedColumns"][0]["name"], "cust_ssn");
        assert_eq!(
            json["maskedColumns"][0]["reason"], "PII.Sensitive",
            "the reason routes the access request to the right person: {json}"
        );
    }

    fn labelled(column: &str, tag: &str) -> (String, String) {
        (column.to_string(), tag.to_string())
    }

    fn masking(names: &[&str]) -> std::collections::HashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn a_column_under_a_masking_classification_is_reported_masked() {
        let labels = [labelled("svc.db.t.ssn", "PII.Sensitive")];

        let masked = masked_columns_of(&labels, &masking(&["PII"]));

        assert_eq!(masked.len(), 1);
        assert_eq!(masked[0].name, "svc.db.t.ssn");
        assert_eq!(
            masked[0].reason, "PII.Sensitive",
            "the tag, not the classification: PII.Sensitive and PII.Basic route \
             to different conversations"
        );
    }

    /// **And a column under an unlisted classification is not.** Masking is
    /// declared by the deployment, never inferred from a tag's name — a
    /// security decision resting on a string comparison no plan justifies is
    /// one that will be wrong somewhere.
    #[test]
    fn a_column_under_an_unlisted_classification_is_not_masked() {
        let labels = [labelled("svc.db.t.region", "Geography.EU")];

        let masked = masked_columns_of(&labels, &masking(&["PII"]));

        assert!(masked.is_empty(), "{masked:?}");
    }

    /// A deployment that has declared nothing masks nothing — the correct
    /// default, and the one that cannot surprise anybody.
    #[test]
    fn nothing_is_masked_when_no_classification_is_declared_masking() {
        let labels = [labelled("svc.db.t.ssn", "PII.Sensitive")];

        let masked = masked_columns_of(&labels, &masking(&[]));

        assert!(masked.is_empty(), "{masked:?}");
    }

    /// Two masking tags on one column is still one column. Shown twice, an
    /// agent reports two.
    #[test]
    fn a_column_with_two_masking_tags_is_reported_once() {
        let labels = [
            labelled("svc.db.t.ssn", "PII.Sensitive"),
            labelled("svc.db.t.ssn", "PII.Basic"),
        ];

        let masked = masked_columns_of(&labels, &masking(&["PII"]));

        assert_eq!(masked.len(), 1, "{masked:?}");
        assert_eq!(masked[0].reason, "PII.Sensitive", "the first one wins");
    }

    /// A malformed tag FQN carries no classification, so it cannot match one —
    /// and must not be treated as masked by default.
    #[test]
    fn a_tag_with_no_classification_masks_nothing() {
        let labels = [labelled("svc.db.t.ssn", "justatag")];

        let masked = masked_columns_of(&labels, &masking(&["PII", "justatag"]));

        assert!(masked.is_empty(), "{masked:?}");
    }

    /// **A masked column survives any budget.** Dropping one to save tokens
    /// produces exactly the failure this type exists to prevent: an agent that
    /// cannot see the column exists never asks for access to it, and describes
    /// the table as having fewer columns than it has.
    #[test]
    fn no_budget_is_small_enough_to_drop_a_masked_column() {
        let mut context = GovernanceContext {
            classifications: (0..40).map(|n| format!("classification.{n}")).collect(),
            masked_columns: vec![
                MaskedColumn {
                    name: "cust_ssn".to_string(),
                    reason: "PII.Sensitive".to_string(),
                },
                MaskedColumn {
                    name: "cust_dob".to_string(),
                    reason: "PII.Sensitive".to_string(),
                },
            ],
            permitted_operations: vec!["read".to_string()],
            ..GovernanceContext::default()
        };

        fit(&mut context, TokenBudget { max_tokens: 0 });

        assert_eq!(
            context.masked_columns.len(),
            2,
            "both masked columns must still be named: {context:?}"
        );
        assert_eq!(context.permitted_operations, vec!["read".to_string()]);
        assert!(
            context.classifications.is_empty(),
            "classifications were what gave way: {context:?}"
        );
        assert!(context.truncated, "and the loss is reported: {context:?}");
    }

    /// A governance context that fits is not marked truncated — the flag has to
    /// mean something or an agent learns to ignore it.
    #[test]
    fn a_governance_context_within_budget_is_not_marked_truncated() {
        let mut context = GovernanceContext {
            classifications: vec!["PII".to_string()],
            ..GovernanceContext::default()
        };

        let reason = fit(&mut context, TokenBudget { max_tokens: 8_000 });

        assert_eq!(reason, None);
        assert!(!context.truncated, "{context:?}");
        assert_eq!(context.classifications, vec!["PII".to_string()]);
    }

    #[test]
    fn the_wire_shapes_are_camel_case() {
        let walk = walk_upstream("c", |_| true, chain());
        let json = serde_json::to_value(&walk).expect("serialize");

        assert!(json.get("policyFiltered").is_some(), "{json}");
        assert!(json.get("depthReached").is_some(), "{json}");
        assert!(json.get("policy_filtered").is_none(), "{json}");
        assert!(json["steps"][0].get("fromFqn").is_some(), "{json}");

        let impact = serde_json::to_value(ImpactReport::default()).expect("serialize");
        assert!(impact.get("affectedAssets").is_some(), "{impact}");
        assert!(impact.get("owningTeams").is_some(), "{impact}");
    }
}
