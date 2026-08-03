//! MCP write tools — Epic 32.
//!
//! **A separate port from [`crate::ContextSource`], deliberately.** A deployment
//! that wires reads and not writes gets a surface that *cannot* write — not one
//! that refuses to, which is a runtime promise, but one with no code path to do
//! it. Epic 14's read tools were shipped first precisely so this could be built
//! against a validated surface; keeping the traits apart is what stops the write
//! half from quietly becoming reachable wherever the read half is.
//!
//! Three rules, and each is a way this could be badly wrong:
//!
//! 1. **Propose is the default.** Only `applyDescription` and `applyTags` land
//!    without a human, and [`graph_owl_authz::agent::decide_write`] — not this
//!    module — decides which.
//! 2. **Confidence overrides the grant.** A 0.6 conclusion proposes even from a
//!    fully-granted agent. See [`graph_owl_authz::agent::decide_memory_write`].
//! 3. **An investigation without evidence is refused.** A finding nobody can
//!    check is a claim, and the catalog already has a word for those.

use async_trait::async_trait;
use graph_owl_authz::agent::AgentCapability;
use serde::Serialize;

use crate::{Outcome, SourceError};

/// What a write tool did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteReceipt {
    /// `applied` or `proposed`. **Always present**, because an agent that
    /// assumes its write landed will not follow up on the one that did not —
    /// and "I updated the description" is a different sentence from "I suggested
    /// a description" when it reaches a person.
    pub outcome: &'static str,
    /// The proposal's id when one was created, so an agent can reference it and
    /// a steward can be pointed at it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub target_fqn: String,
    /// **Why it went this way**, in words. An agent told only "proposed" cannot
    /// tell a low-confidence degradation from a capability it lacks, and those
    /// call for different next actions — improve the evidence, or ask a human
    /// for the capability.
    pub reason: String,
}

/// What a write tool needs from the catalog.
///
/// Every method takes the calling agent's id: **agents write as themselves**,
/// never through a shared service account, because attribution is the entire
/// basis of trust here and a shared identity destroys it.
#[async_trait]
pub trait WriteSink: Send + Sync {
    /// Propose or apply a metadata change, per the agent's grant.
    ///
    /// # Errors
    ///
    /// [`SourceError`] only when the catalog could not be reached. A refusal is
    /// [`Ok`] carrying the refusal text — it is an answer about the request, not
    /// a failure to process it, and an agent acts on the two differently.
    async fn write(
        &self,
        agent_id: &str,
        capability: AgentCapability,
        target_fqn: &str,
        change: serde_json::Value,
        rationale: &str,
        confidence: f64,
    ) -> Result<Result<WriteReceipt, String>, SourceError>;
}

/// Propose a metadata change.
pub const PROPOSE_METADATA_CHANGE: &str = "propose_metadata_change";
/// Record something learned.
pub const RECORD_MEMORY: &str = "record_memory";
/// Record a structured investigation with evidence.
pub const RECORD_INVESTIGATION: &str = "record_investigation";
/// Create a glossary term — always as a draft.
pub const CREATE_GLOSSARY_TERM: &str = "create_glossary_term";
/// Create a quality test.
pub const CREATE_QUALITY_TEST: &str = "create_quality_test";
/// Assert lineage — always proposes.
pub const LINK_LINEAGE: &str = "link_lineage";

/// The write tools, for the manifest.
///
/// Declared alongside the read tools only when a [`WriteSink`] is wired. A
/// surface advertising a tool it cannot serve teaches an agent to distrust the
/// manifest and probe instead — the behaviour a governance surface least wants.
#[must_use]
pub fn write_tools() -> Vec<crate::ToolDeclaration> {
    vec![
        crate::ToolDeclaration {
            name: PROPOSE_METADATA_CHANGE,
            description: "Suggest a description, tags, or an owner for an asset. \
                      Creates a proposal for a human to accept unless you hold a \
                      direct-apply capability. Say why — a suggestion that does \
                      not explain itself is one nobody can review.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": { "type": "string" },
                    "field": {
                        "type": "string",
                        "enum": ["description", "tags", "owner"],
                    },
                    "value": { "description": "The proposed value." },
                    "rationale": {
                        "type": "string",
                        "description": "Why you believe this. Required.",
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                    }
                },
                "required": ["fullyQualifiedName", "field", "value", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
        crate::ToolDeclaration {
            name: RECORD_MEMORY,
            description: "Record something you learned about an asset so the next \
                      agent — or person — does not have to rediscover it. Below \
                      0.8 confidence this becomes a proposal rather than an \
                      assertion, whatever you have been granted.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": { "type": "string" },
                    "content": { "type": "string" },
                    "rationale": {
                        "type": "string",
                        "description": "What led you to this. Required.",
                    },
                    "confidence": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "How sure you are. Required — an \
                                        unlabelled agent memory would rank \
                                        alongside a person's.",
                    }
                },
                "required": ["fullyQualifiedName", "content", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
        crate::ToolDeclaration {
            name: RECORD_INVESTIGATION,
            description: "Record what you investigated, what you found, and what \
                      you checked. Evidence is required: a finding nobody can \
                      verify is a claim, not an investigation.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": { "type": "string" },
                    "findings": { "type": "string" },
                    "evidence": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "The assets, tests or queries you examined.",
                    },
                    "rationale": { "type": "string" },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                },
                "required": ["fullyQualifiedName", "findings", "evidence", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
        crate::ToolDeclaration {
            name: CREATE_GLOSSARY_TERM,
            description: "Propose a glossary term. It is created as a draft and \
                      never as approved — naming something is a governance act, \
                      and an agent proposing a name is useful where an agent \
                      ratifying one is not.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": { "type": "string" },
                    "name": { "type": "string" },
                    "definition": { "type": "string" },
                    "rationale": { "type": "string" },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                },
                "required": ["fullyQualifiedName", "name", "definition", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
        crate::ToolDeclaration {
            name: CREATE_QUALITY_TEST,
            description: "Propose a data-quality test for an asset. Results come \
                      from your test runner, not from here — an agent that both \
                      writes the test and reports its result is grading its own \
                      work.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": { "type": "string" },
                    "name": { "type": "string" },
                    "expression": { "type": "string" },
                    "rationale": { "type": "string" },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                },
                "required": ["fullyQualifiedName", "name", "expression", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
        crate::ToolDeclaration {
            name: LINK_LINEAGE,
            description: "Suggest that one asset feeds another. Always creates a \
                      proposal, whatever you have been granted: a wrong lineage \
                      edge propagates silently through every impact analysis \
                      downstream of it.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "fullyQualifiedName": {
                        "type": "string",
                        "description": "The downstream asset.",
                    },
                    "upstreamFqn": { "type": "string" },
                    "rationale": { "type": "string" },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                },
                "required": ["fullyQualifiedName", "upstreamFqn", "rationale", "confidence"],
                "additionalProperties": false,
            }),
        },
    ]
}

/// Which capability a write tool needs.
///
/// `propose_metadata_change` depends on the field: proposing a description and
/// proposing an owner are different trusts, and a single `proposeAnything`
/// capability would make the scope of a grant unreadable.
#[must_use]
pub fn capability_for(tool: &str, field: Option<&str>) -> Option<AgentCapability> {
    match tool {
        PROPOSE_METADATA_CHANGE => match field {
            Some("description") => Some(AgentCapability::ProposeDescription),
            Some("tags") => Some(AgentCapability::ProposeTags),
            Some("owner") => Some(AgentCapability::ProposeOwner),
            _ => None,
        },
        RECORD_MEMORY => Some(AgentCapability::RecordMemory),
        RECORD_INVESTIGATION => Some(AgentCapability::RecordInvestigation),
        CREATE_GLOSSARY_TERM => Some(AgentCapability::CreateGlossaryTerm),
        CREATE_QUALITY_TEST => Some(AgentCapability::CreateQualityTest),
        LINK_LINEAGE => Some(AgentCapability::LinkLineage),
        _ => None,
    }
}

/// Whether a tool name is one of this module's.
#[must_use]
pub fn is_write_tool(tool: &str) -> bool {
    matches!(
        tool,
        PROPOSE_METADATA_CHANGE
            | RECORD_MEMORY
            | RECORD_INVESTIGATION
            | CREATE_GLOSSARY_TERM
            | CREATE_QUALITY_TEST
            | LINK_LINEAGE
    )
}

/// Run one write tool.
///
/// Argument validation happens here so that a malformed call is a `BadRequest`
/// naming the field, never a write with a defaulted value — an agent that sends
/// no confidence and gets one assigned has had a claim made on its behalf.
pub async fn call_write(
    sink: &dyn WriteSink,
    agent_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
) -> Outcome {
    let Some(fqn) = arguments
        .get("fullyQualifiedName")
        .and_then(|value| value.as_str())
        .filter(|fqn| !fqn.is_empty())
    else {
        return Outcome::BadRequest(
            "`fullyQualifiedName` is required, as a non-empty string".to_string(),
        );
    };

    // **Required, never defaulted.** An agent that omits confidence and gets one
    // assigned has had a claim made on its behalf — and the whole degradation
    // rule rests on this number being the agent's own statement.
    let Some(confidence) = arguments
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
    else {
        return Outcome::BadRequest(
            "`confidence` is required, as a number between 0 and 1; an \
             unstated confidence would rank your assertion alongside a person's"
                .to_string(),
        );
    };
    if !(0.0..=1.0).contains(&confidence) {
        return Outcome::BadRequest(format!(
            "`confidence` must be between 0 and 1, got {confidence}"
        ));
    }

    let Some(rationale) = arguments
        .get("rationale")
        .and_then(|value| value.as_str())
        .filter(|text| !text.trim().is_empty())
    else {
        return Outcome::BadRequest(
            "`rationale` is required; a suggestion that does not explain itself \
             is one nobody can review"
                .to_string(),
        );
    };

    let field = arguments.get("field").and_then(|value| value.as_str());
    let Some(capability) = capability_for(tool, field) else {
        return Outcome::BadRequest(match tool {
            PROPOSE_METADATA_CHANGE => {
                "`field` must be one of \"description\", \"tags\" or \"owner\"".to_string()
            }
            other => format!("no tool named `{other}`"),
        });
    };

    let change = match build_change(tool, arguments) {
        Ok(change) => change,
        Err(problem) => return Outcome::BadRequest(problem),
    };

    match sink
        .write(agent_id, capability, fqn, change, rationale, confidence)
        .await
    {
        Ok(Ok(receipt)) => Outcome::Wrote(Box::new(receipt)),
        // A refusal is an answer about the request, so it comes back as a
        // refusal the agent can read and act on rather than a transport failure.
        Ok(Err(refusal)) => Outcome::Refused(refusal),
        Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
    }
}

/// The tool's own payload, validated.
fn build_change(tool: &str, arguments: &serde_json::Value) -> Result<serde_json::Value, String> {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
    };

    match tool {
        PROPOSE_METADATA_CHANGE => {
            let field = arguments
                .get("field")
                .and_then(|value| value.as_str())
                .ok_or("`field` is required")?;
            let value = arguments.get("value").ok_or("`value` is required")?;
            Ok(serde_json::json!({ field: value }))
        }
        RECORD_MEMORY => Ok(serde_json::json!({
            "content": text("content").ok_or("`content` is required")?,
        })),
        RECORD_INVESTIGATION => {
            let evidence: Vec<String> = arguments
                .get("evidence")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToString::to_string))
                        .collect()
                })
                .unwrap_or_default();
            // **An investigation with no evidence is refused.** A finding nobody
            // can check is a claim, and the catalog already has a word for
            // those — this tool exists precisely to be the other thing.
            if evidence.is_empty() {
                return Err(
                    "`evidence` must name at least one asset, test or query you \
                     examined; a finding nobody can verify is a claim, not an \
                     investigation"
                        .to_string(),
                );
            }
            Ok(serde_json::json!({
                "findings": text("findings").ok_or("`findings` is required")?,
                "evidence": evidence,
            }))
        }
        CREATE_GLOSSARY_TERM => Ok(serde_json::json!({
            "name": text("name").ok_or("`name` is required")?,
            "definition": text("definition").ok_or("`definition` is required")?,
            // **Draft, always.** Set here rather than left to the applier, so
            // there is no path by which an agent's term arrives approved.
            "status": "draft",
        })),
        CREATE_QUALITY_TEST => Ok(serde_json::json!({
            "name": text("name").ok_or("`name` is required")?,
            "expression": text("expression").ok_or("`expression` is required")?,
        })),
        LINK_LINEAGE => Ok(serde_json::json!({
            "upstreamFqn": text("upstreamFqn").ok_or("`upstreamFqn` is required")?,
        })),
        other => Err(format!("no tool named `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records what reached it, so a test can assert the *capability* a tool
    /// asked for — which the receipt alone cannot show.
    struct Recording {
        calls: Mutex<Vec<(AgentCapability, String, serde_json::Value, f64)>>,
        refuse: Option<String>,
    }

    impl Recording {
        fn working() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                refuse: None,
            }
        }
        fn refusing(why: &str) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                refuse: Some(why.to_string()),
            }
        }
        fn last(&self) -> (AgentCapability, String, serde_json::Value, f64) {
            self.calls
                .lock()
                .expect("lock")
                .last()
                .cloned()
                .expect("a call")
        }
    }

    #[async_trait]
    impl WriteSink for Recording {
        async fn write(
            &self,
            _agent_id: &str,
            capability: AgentCapability,
            target_fqn: &str,
            change: serde_json::Value,
            _rationale: &str,
            confidence: f64,
        ) -> Result<Result<WriteReceipt, String>, SourceError> {
            self.calls.lock().expect("lock").push((
                capability,
                target_fqn.to_string(),
                change,
                confidence,
            ));
            if let Some(why) = &self.refuse {
                return Ok(Err(why.clone()));
            }
            // The sink is what consults the grant and the confidence rule; here
            // it mirrors `decide_write`/`decide_memory_write` so the dispatch
            // tests see a realistic answer.
            let applies = capability.applies_directly()
                && graph_owl_authz::agent::decide_memory_write(confidence)
                    == graph_owl_authz::agent::WriteDecision::Apply;
            Ok(Ok(WriteReceipt {
                outcome: if applies { "applied" } else { "proposed" },
                proposal_id: (!applies).then(|| "proposal-1".to_string()),
                target_fqn: target_fqn.to_string(),
                reason: "test".to_string(),
            }))
        }
    }

    fn args(extra: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "fullyQualifiedName": "warehouse.orders",
            "rationale": "because the column comments say so",
            "confidence": 0.9,
        });
        let (serde_json::Value::Object(base_map), serde_json::Value::Object(extra_map)) =
            (&mut base, extra)
        else {
            panic!("both must be objects")
        };
        for (key, value) in extra_map {
            base_map.insert(key, value);
        }
        base
    }

    // ---- capability routing ----

    /// **Each field is its own trust.** A single `proposeAnything` capability
    /// would make the scope of a grant unreadable.
    #[test]
    fn each_metadata_field_needs_its_own_capability() {
        assert_eq!(
            capability_for(PROPOSE_METADATA_CHANGE, Some("description")),
            Some(AgentCapability::ProposeDescription)
        );
        assert_eq!(
            capability_for(PROPOSE_METADATA_CHANGE, Some("tags")),
            Some(AgentCapability::ProposeTags)
        );
        assert_eq!(
            capability_for(PROPOSE_METADATA_CHANGE, Some("owner")),
            Some(AgentCapability::ProposeOwner)
        );
        assert_eq!(
            capability_for(PROPOSE_METADATA_CHANGE, Some("lineage")),
            None
        );
        assert_eq!(capability_for(PROPOSE_METADATA_CHANGE, None), None);
    }

    /// **No write tool maps to a capability that does not exist**, and every
    /// write tool maps to one — a tool with no capability would be an
    /// ungoverned write.
    #[test]
    fn every_write_tool_maps_to_a_real_capability() {
        for declared in write_tools() {
            let field = (declared.name == PROPOSE_METADATA_CHANGE).then_some("description");
            let capability = capability_for(declared.name, field);
            assert!(
                capability.is_some_and(|c| AgentCapability::ALL.contains(&c)),
                "{} maps to {capability:?}",
                declared.name
            );
        }
    }

    #[test]
    fn the_write_tools_are_recognised_and_read_tools_are_not() {
        for declared in write_tools() {
            assert!(is_write_tool(declared.name), "{}", declared.name);
        }
        assert!(!is_write_tool(crate::GET_ASSET_CONTEXT));
        assert!(!is_write_tool(crate::SEARCH_ASSETS));
        assert!(!is_write_tool("delete_everything"));
    }

    // ---- required arguments ----

    /// **Confidence is required, never defaulted.** An agent that omits it and
    /// gets one assigned has had a claim made on its behalf — and the whole
    /// degradation rule rests on the number being the agent's own statement.
    #[tokio::test]
    async fn a_write_without_confidence_is_refused_rather_than_defaulted() {
        let sink = Recording::working();
        let mut arguments = args(serde_json::json!({ "content": "the loader drops refunds" }));
        arguments
            .as_object_mut()
            .expect("object")
            .remove("confidence");

        let outcome = call_write(&sink, "agent-alpha", RECORD_MEMORY, &arguments).await;

        let Outcome::BadRequest(why) = outcome else {
            panic!("expected BadRequest, got {outcome:?}");
        };
        assert!(why.contains("confidence"), "{why}");
        assert!(
            sink.calls.lock().expect("lock").is_empty(),
            "nothing reached the catalog"
        );
    }

    #[tokio::test]
    async fn a_write_without_a_rationale_is_refused() {
        let sink = Recording::working();
        let mut arguments = args(serde_json::json!({ "content": "x" }));
        arguments.as_object_mut().expect("object")["rationale"] =
            serde_json::Value::String("   ".to_string());

        let outcome = call_write(&sink, "agent-alpha", RECORD_MEMORY, &arguments).await;

        let Outcome::BadRequest(why) = outcome else {
            panic!("expected BadRequest, got {outcome:?}");
        };
        assert!(
            why.contains("rationale"),
            "a suggestion that does not explain itself is one nobody can review: {why}"
        );
    }

    #[tokio::test]
    async fn a_confidence_outside_zero_to_one_is_refused() {
        let sink = Recording::working();
        for bad in [-0.1, 1.1, 42.0] {
            let mut arguments = args(serde_json::json!({ "content": "x" }));
            arguments.as_object_mut().expect("object")["confidence"] = serde_json::json!(bad);

            let outcome = call_write(&sink, "agent-alpha", RECORD_MEMORY, &arguments).await;

            assert!(
                matches!(outcome, Outcome::BadRequest(_)),
                "{bad} should be refused, got {outcome:?}"
            );
        }
    }

    // ---- Slice C: evidence and confidence ----

    /// **An investigation with no evidence is refused.** A finding nobody can
    /// verify is a claim, and this tool exists precisely to be the other thing.
    #[tokio::test]
    async fn an_investigation_without_evidence_is_refused() {
        let sink = Recording::working();

        for evidence in [serde_json::json!([]), serde_json::json!(null)] {
            let arguments = args(serde_json::json!({
                "findings": "the nightly load double-counts refunds",
                "evidence": evidence,
            }));

            let outcome = call_write(&sink, "agent-alpha", RECORD_INVESTIGATION, &arguments).await;

            let Outcome::BadRequest(why) = outcome else {
                panic!("expected BadRequest for {evidence:?}, got {outcome:?}");
            };
            assert!(why.contains("evidence"), "{why}");
        }
    }

    #[tokio::test]
    async fn an_investigation_with_evidence_is_accepted_and_carries_it() {
        let sink = Recording::working();
        let arguments = args(serde_json::json!({
            "findings": "the nightly load double-counts refunds",
            "evidence": ["warehouse.staging_orders", "test.orders.row_count"],
        }));

        let outcome = call_write(&sink, "agent-alpha", RECORD_INVESTIGATION, &arguments).await;

        assert!(matches!(outcome, Outcome::Wrote(_)), "{outcome:?}");
        let (capability, _, change, _) = sink.last();
        assert_eq!(capability, AgentCapability::RecordInvestigation);
        assert_eq!(change["evidence"][0], "warehouse.staging_orders");
        assert_eq!(change["evidence"][1], "test.orders.row_count");
    }

    /// **A low-confidence memory proposes**, even though the sink was given a
    /// capability that could apply. Decision 6 overrides the grant.
    #[tokio::test]
    async fn a_low_confidence_memory_comes_back_as_a_proposal() {
        let sink = Recording::working();
        let mut arguments = args(serde_json::json!({ "content": "maybe the loader is at fault" }));
        arguments.as_object_mut().expect("object")["confidence"] = serde_json::json!(0.6);

        let outcome = call_write(&sink, "agent-alpha", RECORD_MEMORY, &arguments).await;

        let Outcome::Wrote(receipt) = outcome else {
            panic!("expected Wrote, got {outcome:?}");
        };
        assert_eq!(receipt.outcome, "proposed");
        assert!(receipt.proposal_id.is_some(), "{receipt:?}");
    }

    /// **A glossary term is created as a draft, always.** Set at build time so
    /// there is no path by which an agent's term arrives approved.
    #[tokio::test]
    async fn a_glossary_term_is_always_a_draft() {
        let sink = Recording::working();
        let arguments = args(serde_json::json!({
            "name": "Net Revenue",
            "definition": "revenue after refunds",
            // Even asked for directly, this must not survive.
            "status": "approved",
        }));

        let outcome = call_write(&sink, "agent-alpha", CREATE_GLOSSARY_TERM, &arguments).await;

        assert!(matches!(outcome, Outcome::Wrote(_)), "{outcome:?}");
        let (_, _, change, _) = sink.last();
        assert_eq!(
            change["status"], "draft",
            "naming something is a governance act: {change}"
        );
    }

    /// **Lineage always proposes**, whatever the grant — a wrong lineage edge
    /// propagates silently through every impact analysis downstream.
    #[tokio::test]
    async fn linking_lineage_asks_for_the_capability_that_cannot_apply() {
        let sink = Recording::working();
        let arguments = args(serde_json::json!({ "upstreamFqn": "warehouse.staging_orders" }));

        let outcome = call_write(&sink, "agent-alpha", LINK_LINEAGE, &arguments).await;

        assert!(matches!(outcome, Outcome::Wrote(_)), "{outcome:?}");
        let (capability, _, change, _) = sink.last();
        assert_eq!(capability, AgentCapability::LinkLineage);
        assert!(
            !capability.applies_directly(),
            "and that capability can never apply directly"
        );
        assert_eq!(change["upstreamFqn"], "warehouse.staging_orders");
    }

    // ---- refusals ----

    /// **A refusal is an answer, not a transport failure.** The agent reads why
    /// and acts on it — asking a human for a capability is a different next step
    /// from retrying.
    #[tokio::test]
    async fn a_refusal_comes_back_readable_rather_than_as_an_error() {
        let sink = Recording::refusing(
            "this action requires the `recordMemory` capability, which this \
             agent has not been granted",
        );
        let arguments = args(serde_json::json!({ "content": "x" }));

        let outcome = call_write(&sink, "agent-alpha", RECORD_MEMORY, &arguments).await;

        let Outcome::Refused(why) = outcome else {
            panic!("expected Refused, got {outcome:?}");
        };
        assert!(
            why.contains("recordMemory"),
            "the agent can ask a human for exactly this: {why}"
        );
    }

    #[tokio::test]
    async fn an_unknown_write_tool_is_refused_by_name() {
        let sink = Recording::working();
        let arguments = args(serde_json::json!({ "content": "x" }));

        let outcome = call_write(&sink, "agent-alpha", "delete_everything", &arguments).await;

        let Outcome::BadRequest(why) = outcome else {
            panic!("expected BadRequest, got {outcome:?}");
        };
        assert!(why.contains("delete_everything"), "{why}");
    }

    /// The receipt says which happened, in words an agent can pass on. "I
    /// updated the description" is a different sentence from "I suggested a
    /// description" when it reaches a person.
    #[test]
    fn a_receipt_says_what_happened_on_the_wire() {
        let receipt = WriteReceipt {
            outcome: "proposed",
            proposal_id: Some("p-1".to_string()),
            target_fqn: "warehouse.orders".to_string(),
            reason: "confidence 0.6 is below the assertion threshold".to_string(),
        };

        let json = serde_json::to_value(&receipt).expect("serialize");

        assert_eq!(json["outcome"], "proposed");
        assert_eq!(json["proposalId"], "p-1");
        assert!(json.get("targetFqn").is_some(), "{json}");
        assert!(json.get("proposal_id").is_none(), "{json}");
    }

    /// An applied write carries no proposal id — a field that is always present
    /// is one an agent stops reading.
    #[test]
    fn an_applied_receipt_carries_no_proposal_id() {
        let receipt = WriteReceipt {
            outcome: "applied",
            proposal_id: None,
            target_fqn: "warehouse.orders".to_string(),
            reason: "applyDescription is granted".to_string(),
        };

        let json = serde_json::to_value(&receipt).expect("serialize");

        assert!(json.get("proposalId").is_none(), "{json}");
    }

    /// Every declared write tool requires a rationale and a confidence in its
    /// schema — the two fields the whole governance model rests on.
    #[test]
    fn every_write_tool_declares_rationale_and_confidence_as_required() {
        for declared in write_tools() {
            let required = declared.input_schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{} declares required fields", declared.name));
            for field in ["fullyQualifiedName", "rationale", "confidence"] {
                assert!(
                    required.iter().any(|entry| entry == field),
                    "{} must require `{field}`",
                    declared.name
                );
            }
        }
    }

    /// **No write tool's name or description suggests deleting.** The absence of
    /// a delete capability is only real if nothing on the surface implies one.
    #[test]
    fn no_write_tool_offers_deletion() {
        for declared in write_tools() {
            assert!(
                !declared.name.contains("delete") && !declared.name.contains("remove"),
                "{}",
                declared.name
            );
        }
    }
}
