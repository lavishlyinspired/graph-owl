//! Agent capabilities and grants — Epic 32.
//!
//! **This is the module that decides what an autonomous caller may write**, and
//! it is deliberately pure: no I/O, no storage, no HTTP. A security decision
//! spread across a query, a handler and a facade is one nobody can read in full,
//! and this one has to be readable in full.
//!
//! Four rules the rest of the epic rests on, each of which is a way this could
//! be catastrophically wrong:
//!
//! 1. **An agent cannot widen its own permissions.** There is no capability for
//!    managing grants, and [`authorize`] refuses grant management unconditionally
//!    — not "unless granted", *unconditionally*. An agent that can grant itself
//!    capability has no capability, only a delay.
//! 2. **Propose is the default; apply is the exception.** Most writes become a
//!    [`Proposal`] a human accepts. Direct application exists for exactly two
//!    narrow capabilities and is enumerated in a closed set.
//! 3. **There is no delete, policy, role, or certification capability**, and
//!    there never will be. [`AgentCapability::ALL`] is asserted by a test whose
//!    job is to make adding one require deleting a comment explaining why it must
//!    not exist.
//! 4. **Low confidence degrades to a proposal regardless of grant.** A grant says
//!    what an agent is trusted to do; confidence says whether *this particular
//!    conclusion* is worth asserting. The second overrides the first, never the
//!    other way round.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use graph_owl_core::ownership::EntityReference;

/// What an agent may be trusted to do.
///
/// **A closed set, and closed on purpose.** Every variant here was argued for;
/// the absences were argued for harder. See [`AgentCapability::ALL`] for the
/// membership test that guards it.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCapability {
    /// Suggest a description. Creates a proposal.
    ProposeDescription,
    /// Suggest tags. Creates a proposal.
    ProposeTags,
    /// Suggest an owner. Creates a proposal.
    ProposeOwner,
    /// **Write a description directly.** Grantable, narrow, and reversible.
    ApplyDescription,
    /// **Apply tags directly.** Grantable, narrow, and reversible.
    ApplyTags,
    /// Record what it learned. Subject to the confidence rule — see
    /// [`decide_memory_write`].
    RecordMemory,
    /// Record a structured investigation with evidence.
    RecordInvestigation,
    /// Create a glossary term. **Always as a draft**, never approved: naming
    /// something is a governance act and an agent proposing a name is useful
    /// where an agent ratifying one is not.
    CreateGlossaryTerm,
    /// Create a quality test. Results still come from outside — an agent that
    /// both writes the test and reports its result is grading its own work.
    CreateQualityTest,
    /// Assert lineage. **Always proposes**, whatever else is granted: a wrong
    /// lineage edge propagates silently through every impact analysis
    /// downstream of it, and the blast radius of the mistake is larger than the
    /// blast radius of the thing it describes.
    LinkLineage,
}

impl AgentCapability {
    /// Every capability that exists.
    ///
    /// **The absences are the specification.** There is deliberately no
    /// `Delete`, no `ManageGrants`, no `EditPolicy`, no `AssignRole`, and no
    /// `Certify`:
    ///
    /// - **Delete** — an agent action that cannot be undone is not shipped, and
    ///   a delete is the one write history cannot fully restore (decision 3).
    /// - **Grants, policy, roles** — an agent that can widen its own permissions
    ///   has none; it has a delay (decision 4). [`authorize`] refuses these
    ///   unconditionally, so this absence is enforced twice.
    /// - **Certify** — certification is a *human* accountability statement
    ///   (Epic 26 decision 4). An agent issuing one does not make the asset
    ///   trustworthy; it makes certification meaningless.
    ///
    /// A test asserts this array's exact membership. That test exists so adding
    /// a capability requires deleting the paragraph above, which is the point:
    /// scope creep in a security-sensitive enum should cost an argument, not a
    /// line.
    pub const ALL: [AgentCapability; 10] = [
        AgentCapability::ProposeDescription,
        AgentCapability::ProposeTags,
        AgentCapability::ProposeOwner,
        AgentCapability::ApplyDescription,
        AgentCapability::ApplyTags,
        AgentCapability::RecordMemory,
        AgentCapability::RecordInvestigation,
        AgentCapability::CreateGlossaryTerm,
        AgentCapability::CreateQualityTest,
        AgentCapability::LinkLineage,
    ];

    /// The wire name, for refusals that have to name what was missing.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AgentCapability::ProposeDescription => "proposeDescription",
            AgentCapability::ProposeTags => "proposeTags",
            AgentCapability::ProposeOwner => "proposeOwner",
            AgentCapability::ApplyDescription => "applyDescription",
            AgentCapability::ApplyTags => "applyTags",
            AgentCapability::RecordMemory => "recordMemory",
            AgentCapability::RecordInvestigation => "recordInvestigation",
            AgentCapability::CreateGlossaryTerm => "createGlossaryTerm",
            AgentCapability::CreateQualityTest => "createQualityTest",
            AgentCapability::LinkLineage => "linkLineage",
        }
    }

    /// Whether holding this capability lets an agent write **without** a human
    /// in the loop.
    ///
    /// Exactly two, and the narrowness is the argument for allowing any: a
    /// description and a tag are both cheap to review after the fact, cheap to
    /// revert, and visible in history. Nothing structural, nothing that another
    /// system reads as an assertion about correctness.
    #[must_use]
    pub fn applies_directly(self) -> bool {
        matches!(
            self,
            AgentCapability::ApplyDescription | AgentCapability::ApplyTags
        )
    }
}

/// How many writes an agent may make in a window.
///
/// **Per agent, per capability, per window** — not a global cap. A single
/// runaway agent must not be able to spend everybody else's budget, and a
/// runaway *loop* is usually confined to one capability, so a per-capability
/// limit stops it while leaving the agent's other work alive.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub max_writes: u32,
    pub window_seconds: u32,
}

impl Default for RateLimit {
    /// **Sixty writes an hour.**
    ///
    /// Derived from what the limit is for rather than from a round number: it
    /// exists to stop a *loop*, not to pace deliberate work. A human steward
    /// reviewing an agent's output manages a few dozen items an hour at best, so
    /// an agent producing more than one a minute is already producing more than
    /// anybody will read — which is the point at which extra output stops being
    /// value and starts being a queue nobody drains. A looping agent hits this
    /// in under a minute.
    fn default() -> Self {
        Self {
            max_writes: 60,
            window_seconds: 3_600,
        }
    }
}

/// What a grant is restricted to.
///
/// `None` on [`AgentGrant::scope`] means the whole estate, which is a real and
/// sometimes correct answer — but it is spelled as an explicit `None` rather
/// than an empty list, so nobody reads "no scopes" as "no access".
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeRef {
    /// An FQN prefix. `warehouse.retail` admits `warehouse.retail.public.orders`
    /// and refuses `warehouse.finance.public.salaries`.
    pub fqn_prefix: String,
}

impl ScopeRef {
    /// Whether this scope admits an FQN.
    ///
    /// **Prefix matching is on whole segments**, so `warehouse.retail` does not
    /// admit `warehouse.retail_archive` — a scope that leaked into a
    /// similarly-named sibling would be a grant nobody wrote, and FQN segments
    /// are exactly the boundary the catalog already draws.
    #[must_use]
    pub fn admits(&self, fqn: &str) -> bool {
        if self.fqn_prefix.is_empty() {
            return false;
        }
        if fqn == self.fqn_prefix {
            return true;
        }
        fqn.strip_prefix(&self.fqn_prefix)
            .is_some_and(|rest| rest.starts_with('.'))
    }
}

/// What one agent may do, for how long, and how fast.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentGrant {
    pub id: Uuid,
    /// **A distinct bot principal, never a shared service account.**
    ///
    /// Attribution is the entire basis of trust here (decision 1). Two agents
    /// behind one identity means a bad conclusion cannot be traced to the thing
    /// that drew it, and every other agent inherits its reputation.
    pub agent: EntityReference,
    pub capabilities: Vec<AgentCapability>,
    /// `None` is the whole estate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
    #[serde(default)]
    pub rate_limit: RateLimit,
    /// **Grants can be time-boxed**, and a grant for a specific investigation
    /// should be. An expired grant refuses everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub granted_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Why a write was refused.
///
/// **Each variant names what would fix it**, because the caller is a program
/// and "forbidden" gives it nothing to act on. An agent told which capability it
/// lacks can ask a human for that capability; an agent told "no" retries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("this action requires the `{0}` capability, which this agent has not been granted")]
    MissingCapability(&'static str),
    /// **The unconditional one.**
    #[error(
        "agents may never manage grants, policies, roles, or certifications — \
         this is not a missing capability that could be granted, it is outside \
         what any grant can contain"
    )]
    OutsideAnyGrant,
    #[error("this agent's grant expired at {0}")]
    Expired(DateTime<Utc>),
    #[error("`{fqn}` is outside this agent's granted scope of `{scope}`")]
    OutOfScope { fqn: String, scope: String },
    #[error(
        "this agent has made {made} writes of `{capability}` in the last \
         {window_seconds}s and its limit is {limit}; retry after {retry_after_seconds}s"
    )]
    RateLimited {
        capability: &'static str,
        made: u32,
        limit: u32,
        window_seconds: u32,
        retry_after_seconds: u64,
    },
    /// The agent may not read what it is trying to write. **Read gates write**:
    /// an agent that cannot see an asset must not be able to learn about it by
    /// writing to it and reading the error.
    #[error("this agent cannot read `{0}`, so it cannot write to it")]
    Unreadable(String),
}

/// Actions an agent can attempt that are **not** capabilities and never will be.
///
/// Modelled explicitly rather than left as "anything not in the enum", because
/// an absence cannot be tested and this list can. [`authorize`] refuses every
/// one of these unconditionally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForbiddenAction {
    /// Creating, modifying or deleting any [`AgentGrant`] — including its own.
    ManageGrants,
    EditPolicy,
    AssignRole,
    Certify,
    Delete,
}

impl ForbiddenAction {
    pub const ALL: [ForbiddenAction; 5] = [
        ForbiddenAction::ManageGrants,
        ForbiddenAction::EditPolicy,
        ForbiddenAction::AssignRole,
        ForbiddenAction::Certify,
        ForbiddenAction::Delete,
    ];
}

/// **An agent may never do these, whatever it holds.**
///
/// Takes the grant only so that a reader can see it is *ignored*. The signature
/// is the documentation: there is no argument that could make this return `Ok`.
///
/// # Errors
///
/// Always [`Refusal::OutsideAnyGrant`].
pub fn authorize_forbidden(_grant: &AgentGrant, _action: ForbiddenAction) -> Result<(), Refusal> {
    Err(Refusal::OutsideAnyGrant)
}

/// Whether this grant permits this capability against this FQN, now.
///
/// Checked in a deliberate order — **expiry, then capability, then scope** —
/// because the refusal an agent sees should be the most fundamental one. An
/// expired grant that also lacks the capability should say "expired": telling it
/// to request a capability it would still not be able to use sends it down a
/// path that ends in the same place.
///
/// # Errors
///
/// [`Refusal`] naming which rule refused and what would fix it.
pub fn authorize(
    grant: &AgentGrant,
    capability: AgentCapability,
    fqn: &str,
    now: DateTime<Utc>,
) -> Result<(), Refusal> {
    if let Some(expires_at) = grant.expires_at {
        // `<=` rather than `<`: a grant expiring exactly now has expired. The
        // boundary has to fall somewhere and the safe side is the closed one.
        if expires_at <= now {
            return Err(Refusal::Expired(expires_at));
        }
    }

    if !grant.capabilities.contains(&capability) {
        return Err(Refusal::MissingCapability(capability.as_str()));
    }

    if let Some(scope) = &grant.scope
        && !scope.admits(fqn)
    {
        return Err(Refusal::OutOfScope {
            fqn: fqn.to_string(),
            scope: scope.fqn_prefix.clone(),
        });
    }

    Ok(())
}

/// What happens to a write that was authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteDecision {
    /// It lands now, attributed to the agent, revertible through history.
    Apply,
    /// It becomes a [`Proposal`] for a human to accept.
    Propose,
}

/// Apply or propose?
///
/// **Propose is the default and `Apply` is the exception**, so this is written
/// as "is it one of the two direct-apply capabilities" rather than "is it one of
/// the proposing ones". The difference matters when a capability is added: a new
/// variant proposes until somebody deliberately decides otherwise, which is the
/// safe direction to be wrong in.
#[must_use]
pub fn decide_write(capability: AgentCapability) -> WriteDecision {
    if capability.applies_directly() {
        WriteDecision::Apply
    } else {
        WriteDecision::Propose
    }
}

/// The confidence at or above which an agent may assert rather than propose.
///
/// **Not a tuning knob — a statement about what an assertion means.** Below this
/// the agent is saying "probably", and a catalog that records "probably" as fact
/// is worse than one that records nothing, because a reader cannot tell the two
/// apart afterwards. 0.8 is where a stated confidence stops reading as a hedge:
/// four times in five is a claim somebody will act on, and the fifth is what the
/// human review exists to catch.
pub const ASSERTION_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Whether a memory at this confidence may be asserted.
///
/// **Decision 6, and it overrides the grant.** A grant says what an agent is
/// trusted to do; confidence says whether this particular conclusion is worth
/// asserting. An agent with `RecordMemory` and a 0.6 conclusion proposes it —
/// otherwise the grant would launder a guess into a fact, which is precisely the
/// failure the confidence field exists to prevent.
#[must_use]
pub fn decide_memory_write(confidence: f64) -> WriteDecision {
    if confidence >= ASSERTION_CONFIDENCE_THRESHOLD {
        WriteDecision::Apply
    } else {
        WriteDecision::Propose
    }
}

/// Where a proposal is in its life.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalStatus {
    Open,
    Accepted,
    Rejected,
    /// The underlying entity moved on before anybody decided. Distinct from
    /// `Rejected`: nobody judged this one, and an agent's track record must not
    /// count it against them.
    Superseded,
}

/// An agent's suggestion, awaiting a human.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: Uuid,
    /// Who suggested it. **Always the agent**, never the human who later accepts
    /// — see [`accepted_attribution`].
    pub proposed_by: EntityReference,
    pub target_fqn: String,
    pub capability: AgentCapability,
    /// The proposed value, shaped by the capability.
    pub change: serde_json::Value,
    /// **Why.** Required, because a suggestion an agent cannot justify is one a
    /// reviewer cannot evaluate, and a queue of unjustified suggestions is a
    /// queue nobody works.
    pub rationale: String,
    /// What the agent believed. Carried onto the proposal so a reviewer can
    /// triage by it.
    pub confidence: f64,
    pub status: ProposalStatus,
    /// The entity version this was proposed against. A proposal against a value
    /// that has since moved is stale — see [`is_stale`].
    pub base_version: graph_owl_core::envelope::EntityVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Whether the entity moved since this was proposed.
///
/// **A stale proposal is a `409`, not a silent overwrite.** The agent read a
/// value, reasoned about it, and suggested a change; if the value has since
/// moved, the reasoning was about something that no longer exists and applying
/// it would discard whatever the human did in between.
#[must_use]
pub fn is_stale(proposal: &Proposal, current: graph_owl_core::envelope::EntityVersion) -> bool {
    proposal.base_version != current
}

/// Who an accepted proposal is attributed to, and who approved it.
///
/// **The agent authored it; the human approved it.** Getting this backwards is
/// the single most damaging mistake available in this epic: it erases the
/// agent's track record, so nobody can tell which agent's suggestions turn out
/// well, and it credits the reviewer with work they only checked — which
/// destroys the incentive to check carefully, because a rubber stamp and a real
/// review look identical in the history.
#[must_use]
pub fn accepted_attribution(proposal: &Proposal, approver: &str) -> (String, String) {
    (proposal.proposed_by.id.clone(), approver.to_string())
}

/// How many writes remain in the window, and when the budget frees up.
///
/// `writes_in_window` is supplied by the caller, which is what makes this pure
/// and what makes the limit **survive a restart**: the count comes from the
/// durable activity log rather than from a counter in this process. An
/// in-memory counter would reset on deploy, which is exactly when a runaway
/// agent gets its budget back.
///
/// # Errors
///
/// [`Refusal::RateLimited`] naming what was spent, the limit, and how long to
/// wait — a caller that has to guess the retry interval will guess wrong in the
/// aggressive direction.
pub fn check_rate_limit(
    limit: RateLimit,
    capability: AgentCapability,
    writes_in_window: u32,
    oldest_write_age_seconds: Option<u64>,
) -> Result<(), Refusal> {
    if writes_in_window < limit.max_writes {
        return Ok(());
    }
    // The budget frees up when the oldest write in the window falls out of it.
    // Without one to measure from, the whole window is the honest answer.
    let retry_after_seconds = oldest_write_age_seconds
        .map_or(u64::from(limit.window_seconds), |age| {
            u64::from(limit.window_seconds).saturating_sub(age).max(1)
        });
    Err(Refusal::RateLimited {
        capability: capability.as_str(),
        made: writes_in_window,
        limit: limit.max_writes,
        window_seconds: limit.window_seconds,
        retry_after_seconds,
    })
}

/// What happened when an agent tried to write.
///
/// **Refusals are recorded too**, which is the whole reason this is an enum
/// rather than a boolean on a success row. An agent repeatedly attempting
/// un-granted writes is a signal worth seeing — it is either misconfigured or
/// doing something nobody intended, and an audit log of only successes cannot
/// show either.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityOutcome {
    Applied,
    Proposed,
    Refused,
}

/// One line in an agent's history.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivity {
    pub id: Uuid,
    pub agent_id: String,
    pub capability: AgentCapability,
    pub target_fqn: String,
    pub outcome: ActivityOutcome,
    /// Present on a refusal, naming which rule refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    pub at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::ownership::OwnerKind;

    fn agent() -> EntityReference {
        EntityReference {
            id: "agent-alpha".to_string(),
            kind: OwnerKind::User,
            display_name: "Alpha".to_string(),
            inherited: false,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-03T12:00:00Z")
            .expect("fixed instant")
            .with_timezone(&Utc)
    }

    fn grant(capabilities: Vec<AgentCapability>) -> AgentGrant {
        AgentGrant {
            id: Uuid::nil(),
            agent: agent(),
            capabilities,
            scope: None,
            rate_limit: RateLimit::default(),
            expires_at: None,
            granted_by: "asha".to_string(),
            created_at: now(),
            updated_at: now(),
        }
    }

    // ---- Slice D: the closed enum ----

    /// **The membership test.** Adding a capability requires changing this test,
    /// which requires reading why the absences are absences.
    ///
    /// Delete, grant management, policy, role and certification are missing on
    /// purpose. See [`AgentCapability::ALL`] before adding anything.
    #[test]
    fn the_capability_set_is_exactly_the_ten_documented_ones() {
        assert_eq!(AgentCapability::ALL.len(), 10);
        assert_eq!(
            AgentCapability::ALL,
            [
                AgentCapability::ProposeDescription,
                AgentCapability::ProposeTags,
                AgentCapability::ProposeOwner,
                AgentCapability::ApplyDescription,
                AgentCapability::ApplyTags,
                AgentCapability::RecordMemory,
                AgentCapability::RecordInvestigation,
                AgentCapability::CreateGlossaryTerm,
                AgentCapability::CreateQualityTest,
                AgentCapability::LinkLineage,
            ]
        );
    }

    /// And the names are stable and distinct — a refusal names the capability,
    /// so two capabilities sharing a name would send an agent asking for the
    /// wrong one.
    #[test]
    fn every_capability_has_its_own_wire_name() {
        let names: std::collections::HashSet<&str> = AgentCapability::ALL
            .iter()
            .map(|capability| capability.as_str())
            .collect();

        assert_eq!(names.len(), AgentCapability::ALL.len());
        assert!(names.iter().all(|name| !name.is_empty()));
    }

    /// **Exactly two capabilities apply directly.** The narrowness is the whole
    /// argument for permitting any direct application at all.
    #[test]
    fn only_description_and_tags_apply_without_a_human() {
        let direct: Vec<AgentCapability> = AgentCapability::ALL
            .into_iter()
            .filter(|capability| capability.applies_directly())
            .collect();

        assert_eq!(
            direct,
            vec![
                AgentCapability::ApplyDescription,
                AgentCapability::ApplyTags
            ]
        );
    }

    /// Everything else proposes — stated as its own assertion so that a new
    /// variant defaulting to `Apply` fails here rather than shipping.
    #[test]
    fn every_other_capability_proposes() {
        for capability in AgentCapability::ALL {
            if capability.applies_directly() {
                continue;
            }
            assert_eq!(
                decide_write(capability),
                WriteDecision::Propose,
                "{} must propose",
                capability.as_str()
            );
        }
    }

    /// **Lineage always proposes, whatever else is granted.** A wrong lineage
    /// edge propagates silently through every impact analysis downstream.
    #[test]
    fn lineage_always_proposes() {
        assert_eq!(
            decide_write(AgentCapability::LinkLineage),
            WriteDecision::Propose
        );
        assert!(!AgentCapability::LinkLineage.applies_directly());
    }

    // ---- Slice A: grants and refusal ----

    /// **The security-critical test.** An agent holding *every* capability is
    /// still refused grant management — because it is not a capability that
    /// could be granted, it is outside what any grant can contain.
    #[test]
    fn an_agent_holding_everything_may_still_not_touch_grants() {
        let omnipotent = grant(AgentCapability::ALL.to_vec());

        for action in ForbiddenAction::ALL {
            assert_eq!(
                authorize_forbidden(&omnipotent, action),
                Err(Refusal::OutsideAnyGrant),
                "{action:?} must be refused"
            );
        }
    }

    /// And the forbidden set is itself closed, for the same reason the
    /// capability set is.
    #[test]
    fn the_forbidden_set_is_exactly_the_five_documented_ones() {
        assert_eq!(
            ForbiddenAction::ALL,
            [
                ForbiddenAction::ManageGrants,
                ForbiddenAction::EditPolicy,
                ForbiddenAction::AssignRole,
                ForbiddenAction::Certify,
                ForbiddenAction::Delete,
            ]
        );
    }

    /// **No capability's name resembles a forbidden action**, so a grant cannot
    /// be written that looks like it confers one.
    #[test]
    fn no_capability_names_a_forbidden_action() {
        for capability in AgentCapability::ALL {
            let name = capability.as_str().to_ascii_lowercase();
            for forbidden in ["delete", "grant", "policy", "role", "certif"] {
                assert!(
                    !name.contains(forbidden),
                    "`{name}` reads like `{forbidden}`"
                );
            }
        }
    }

    #[test]
    fn a_granted_capability_is_permitted() {
        let held = grant(vec![AgentCapability::ProposeDescription]);

        assert_eq!(
            authorize(
                &held,
                AgentCapability::ProposeDescription,
                "warehouse.orders",
                now()
            ),
            Ok(())
        );
    }

    /// **A refusal names what was missing**, because the caller is a program and
    /// "forbidden" gives it nothing to act on.
    #[test]
    fn an_ungranted_capability_is_refused_by_name() {
        let held = grant(vec![AgentCapability::ProposeDescription]);

        let refusal = authorize(&held, AgentCapability::ApplyTags, "warehouse.orders", now());

        assert_eq!(
            refusal,
            Err(Refusal::MissingCapability("applyTags")),
            "the agent can ask a human for exactly this"
        );
    }

    #[test]
    fn an_expired_grant_refuses() {
        let mut held = grant(vec![AgentCapability::ProposeDescription]);
        held.expires_at = Some(now() - chrono::Duration::seconds(1));

        let refusal = authorize(
            &held,
            AgentCapability::ProposeDescription,
            "warehouse.orders",
            now(),
        );

        assert!(matches!(refusal, Err(Refusal::Expired(_))), "{refusal:?}");
    }

    /// The boundary is closed: a grant expiring exactly now has expired.
    #[test]
    fn a_grant_expiring_exactly_now_has_expired() {
        let mut held = grant(vec![AgentCapability::ProposeDescription]);
        held.expires_at = Some(now());

        assert!(matches!(
            authorize(
                &held,
                AgentCapability::ProposeDescription,
                "warehouse.orders",
                now()
            ),
            Err(Refusal::Expired(_))
        ));
    }

    /// And an unexpired grant does **not** refuse — or the test above would
    /// pass against a function that refuses everything.
    #[test]
    fn a_grant_expiring_later_still_works() {
        let mut held = grant(vec![AgentCapability::ProposeDescription]);
        held.expires_at = Some(now() + chrono::Duration::seconds(1));

        assert_eq!(
            authorize(
                &held,
                AgentCapability::ProposeDescription,
                "warehouse.orders",
                now()
            ),
            Ok(())
        );
    }

    /// **Expiry is checked before capability**, so an expired grant says
    /// "expired" rather than sending the agent to request a capability it still
    /// could not use.
    #[test]
    fn an_expired_grant_reports_expiry_rather_than_the_missing_capability() {
        let mut held = grant(vec![]);
        held.expires_at = Some(now() - chrono::Duration::seconds(1));

        assert!(matches!(
            authorize(&held, AgentCapability::ApplyTags, "warehouse.orders", now()),
            Err(Refusal::Expired(_))
        ));
    }

    // ---- scope ----

    #[test]
    fn a_scoped_grant_admits_inside_its_scope() {
        let mut held = grant(vec![AgentCapability::ApplyDescription]);
        held.scope = Some(ScopeRef {
            fqn_prefix: "warehouse.retail".to_string(),
        });

        assert_eq!(
            authorize(
                &held,
                AgentCapability::ApplyDescription,
                "warehouse.retail.public.orders",
                now()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_scoped_grant_refuses_outside_its_scope() {
        let mut held = grant(vec![AgentCapability::ApplyDescription]);
        held.scope = Some(ScopeRef {
            fqn_prefix: "warehouse.retail".to_string(),
        });

        let refusal = authorize(
            &held,
            AgentCapability::ApplyDescription,
            "warehouse.finance.public.salaries",
            now(),
        );

        assert!(
            matches!(refusal, Err(Refusal::OutOfScope { .. })),
            "{refusal:?}"
        );
    }

    /// **Scope matches on whole segments.** `warehouse.retail` must not admit
    /// `warehouse.retail_archive` — that would be a grant nobody wrote.
    #[test]
    fn a_scope_does_not_leak_into_a_similarly_named_sibling() {
        let scope = ScopeRef {
            fqn_prefix: "warehouse.retail".to_string(),
        };

        assert!(scope.admits("warehouse.retail"));
        assert!(scope.admits("warehouse.retail.public.orders"));
        assert!(!scope.admits("warehouse.retail_archive"));
        assert!(!scope.admits("warehouse.retailer.public.orders"));
        assert!(!scope.admits("warehouse.finance"));
    }

    /// An empty prefix admits nothing rather than everything. A scope row that
    /// somehow arrived blank must not silently become estate-wide access — that
    /// is the direction a bug must never fail in.
    #[test]
    fn an_empty_scope_admits_nothing() {
        let scope = ScopeRef {
            fqn_prefix: String::new(),
        };

        assert!(!scope.admits("warehouse.orders"));
        assert!(!scope.admits(""));
    }

    /// An unscoped grant reaches the whole estate — the `None`, not an empty
    /// `ScopeRef`.
    #[test]
    fn an_unscoped_grant_reaches_everything() {
        let held = grant(vec![AgentCapability::ApplyDescription]);

        assert_eq!(
            authorize(
                &held,
                AgentCapability::ApplyDescription,
                "anything.at.all",
                now()
            ),
            Ok(())
        );
    }

    // ---- Slice C: confidence overrides the grant ----

    /// **The degradation test.** A fully-granted agent with a 0.6 conclusion
    /// proposes it. Decision 6 overrides the grant, never the other way round.
    #[test]
    fn a_low_confidence_memory_proposes_even_when_recording_is_granted() {
        assert_eq!(decide_memory_write(0.6), WriteDecision::Propose);
        assert_eq!(decide_memory_write(0.79), WriteDecision::Propose);
    }

    #[test]
    fn a_confident_memory_is_asserted() {
        assert_eq!(decide_memory_write(0.8), WriteDecision::Apply);
        assert_eq!(decide_memory_write(1.0), WriteDecision::Apply);
    }

    /// The threshold is inclusive, and the boundary is where it says it is.
    #[test]
    fn the_confidence_boundary_is_closed_at_the_threshold() {
        assert_eq!(
            decide_memory_write(ASSERTION_CONFIDENCE_THRESHOLD),
            WriteDecision::Apply
        );
        assert_eq!(
            decide_memory_write(ASSERTION_CONFIDENCE_THRESHOLD - f64::EPSILON),
            WriteDecision::Propose
        );
    }

    // ---- Slice B: attribution ----

    fn proposal() -> Proposal {
        Proposal {
            id: Uuid::nil(),
            proposed_by: agent(),
            target_fqn: "warehouse.orders".to_string(),
            capability: AgentCapability::ProposeDescription,
            change: serde_json::json!({ "description": "customer orders" }),
            rationale: "the column comments all describe order fields".to_string(),
            confidence: 0.9,
            status: ProposalStatus::Open,
            base_version: graph_owl_core::envelope::EntityVersion { major: 1, minor: 0 },
            decided_by: None,
            decided_at: None,
            created_at: now(),
        }
    }

    /// **The attribution test.** The agent authored it; the human approved it.
    ///
    /// Backwards, this erases the agent's track record and credits the reviewer
    /// with work they only checked — so a rubber stamp and a real review become
    /// indistinguishable in the history.
    #[test]
    fn an_accepted_proposal_is_attributed_to_the_agent_not_the_approver() {
        let (author, approver) = accepted_attribution(&proposal(), "asha");

        assert_eq!(author, "agent-alpha", "the agent wrote it");
        assert_eq!(approver, "asha", "the human checked it");
        assert_ne!(author, approver);
    }

    /// **A proposal against a moved value is stale.** The agent reasoned about
    /// something that no longer exists; applying it would discard whatever the
    /// human did in between.
    #[test]
    fn a_proposal_against_a_moved_version_is_stale() {
        let open = proposal();

        assert!(!is_stale(
            &open,
            graph_owl_core::envelope::EntityVersion { major: 1, minor: 0 }
        ));
        assert!(is_stale(
            &open,
            graph_owl_core::envelope::EntityVersion { major: 1, minor: 1 }
        ));
        assert!(is_stale(
            &open,
            graph_owl_core::envelope::EntityVersion { major: 2, minor: 0 }
        ));
    }

    // ---- Slice E: rate limits ----

    #[test]
    fn a_write_within_the_limit_is_permitted() {
        let limit = RateLimit {
            max_writes: 3,
            window_seconds: 60,
        };

        assert_eq!(
            check_rate_limit(limit, AgentCapability::RecordMemory, 2, None),
            Ok(())
        );
    }

    /// **The loop test.** An agent making N+1 writes in a window is refused on
    /// the N+1th.
    #[test]
    fn the_write_after_the_limit_is_refused() {
        let limit = RateLimit {
            max_writes: 3,
            window_seconds: 60,
        };

        let refusal = check_rate_limit(limit, AgentCapability::RecordMemory, 3, None);

        let Err(Refusal::RateLimited {
            made,
            limit: reported,
            capability,
            ..
        }) = refusal
        else {
            panic!("expected RateLimited, got {refusal:?}");
        };
        assert_eq!(made, 3);
        assert_eq!(reported, 3);
        assert_eq!(capability, "recordMemory");
    }

    /// **The refusal says how long to wait.** A caller made to guess the retry
    /// interval guesses in the aggressive direction.
    #[test]
    fn a_rate_limit_refusal_carries_a_retry_interval() {
        let limit = RateLimit {
            max_writes: 3,
            window_seconds: 60,
        };

        let refusal = check_rate_limit(limit, AgentCapability::RecordMemory, 3, Some(45));

        let Err(Refusal::RateLimited {
            retry_after_seconds,
            ..
        }) = refusal
        else {
            panic!("expected RateLimited, got {refusal:?}");
        };
        assert_eq!(
            retry_after_seconds, 15,
            "the oldest write leaves the window in 15s"
        );
    }

    /// With nothing to measure from, the whole window is the honest answer —
    /// never zero, which would invite an immediate retry that also fails.
    #[test]
    fn a_retry_interval_is_never_zero() {
        let limit = RateLimit {
            max_writes: 1,
            window_seconds: 60,
        };

        for age in [None, Some(0), Some(60), Some(9_999)] {
            let refusal = check_rate_limit(limit, AgentCapability::RecordMemory, 1, age);
            let Err(Refusal::RateLimited {
                retry_after_seconds,
                ..
            }) = refusal
            else {
                panic!("expected RateLimited for age {age:?}");
            };
            assert!(retry_after_seconds >= 1, "age {age:?} gave 0");
        }
    }

    /// The default exists to stop a loop, not to pace deliberate work — so a
    /// human-speed rate must pass it comfortably.
    #[test]
    fn the_default_limit_does_not_bind_on_human_speed_work() {
        let limit = RateLimit::default();

        assert_eq!(
            check_rate_limit(limit, AgentCapability::RecordMemory, 30, None),
            Ok(()),
            "thirty writes an hour is review-able and must pass"
        );
        assert!(check_rate_limit(limit, AgentCapability::RecordMemory, 60, None).is_err());
    }

    // ---- Slice F: audit records refusals too ----

    /// **Refusals are recorded.** An agent repeatedly attempting un-granted
    /// writes is either misconfigured or doing something nobody intended, and a
    /// log of only successes shows neither.
    #[test]
    fn the_outcome_set_distinguishes_refused_from_applied_and_proposed() {
        let outcomes = [
            ActivityOutcome::Applied,
            ActivityOutcome::Proposed,
            ActivityOutcome::Refused,
        ];
        let rendered: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| serde_json::to_value(outcome).expect("serialize"))
            .collect();

        assert_eq!(rendered, vec!["applied", "proposed", "refused"]);
    }

    #[test]
    fn the_wire_shapes_are_camel_case() {
        let json = serde_json::to_value(grant(vec![AgentCapability::ApplyDescription]))
            .expect("serialize");

        assert!(json.get("rateLimit").is_some(), "{json}");
        assert!(json.get("grantedBy").is_some(), "{json}");
        assert!(json.get("rate_limit").is_none(), "{json}");
        assert_eq!(json["capabilities"][0], "applyDescription");

        let limit = serde_json::to_value(RateLimit::default()).expect("serialize");
        assert!(limit.get("maxWrites").is_some(), "{limit}");
        assert!(limit.get("windowSeconds").is_some(), "{limit}");

        let proposed = serde_json::to_value(proposal()).expect("serialize");
        assert!(proposed.get("targetFqn").is_some(), "{proposed}");
        assert!(proposed.get("proposedBy").is_some(), "{proposed}");
        assert!(proposed.get("baseVersion").is_some(), "{proposed}");
    }
}
