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

pub mod catalog;
pub mod trust;

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
}

/// What a tool call produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Found(Box<AssetContext>),
    /// What is known about an asset, best first. **Empty is a real answer** —
    /// "nothing has been written down" is information, and it is not
    /// [`Outcome::NotFound`].
    Recalled(Vec<MemoryContext>),
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

/// Everything this server offers.
///
/// A surface advertising tools it cannot serve teaches an agent to distrust the
/// manifest, and an agent that distrusts the manifest probes instead — the
/// behaviour a read-only surface least wants to encourage. So a tool appears here
/// only once [`call`] can serve it.
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
    // Authentication first, **before the tool name is checked**. Replying "no
    // such tool" to an unauthenticated caller tells them which tools exist.
    let Some(principal) = principal else {
        return Outcome::Unauthenticated;
    };

    if tool != GET_ASSET_CONTEXT && tool != RECALL_MEMORY {
        return Outcome::BadRequest(format!("no tool named `{tool}`"));
    }

    let Some(fqn) = arguments.get("fullyQualifiedName").and_then(|v| v.as_str()) else {
        return Outcome::BadRequest("`fullyQualifiedName` is required, as a string".to_string());
    };
    // An empty name is a mistake, not a lookup. Passing it through returns
    // `NotFound` and teaches the agent the asset does not exist, when what
    // happened is that it never asked about one.
    if fqn.is_empty() {
        return Outcome::BadRequest("`fullyQualifiedName` must not be empty".to_string());
    }

    if tool == RECALL_MEMORY {
        // `query` is optional — "everything you know about this table" is a real
        // question — but a `query` of the wrong *type* is a mistake worth naming
        // rather than silently reading as absent, or an agent sending
        // `{"query": ["a","b"]}` gets unranked results and no idea why.
        let query = match arguments.get("query") {
            None | Some(serde_json::Value::Null) => "",
            Some(serde_json::Value::String(text)) => text.as_str(),
            Some(_) => {
                return Outcome::BadRequest("`query`, when given, must be a string".to_string());
            }
        };
        return match source.recall(principal, fqn, query).await {
            // **Empty is `Recalled`, not `NotFound`.** "Nothing has been written
            // down about this table" and "there is no such table" are opposite
            // statements, and an agent that conflates them fills the silence.
            Ok(Some(memories)) => Outcome::Recalled(memories),
            Ok(None) => Outcome::NotFound,
            Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
        };
    }

    match source.asset_context(principal, fqn).await {
        Ok(Some(context)) => Outcome::Found(Box::new(context)),
        Ok(None) => Outcome::NotFound,
        Err(SourceError::Unavailable(detail)) => Outcome::Unavailable(detail),
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

            assert_eq!(names, vec![RECALL_MEMORY, GET_ASSET_CONTEXT]);
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
            })
            .expect("serialises");

            assert!(json["fullyQualifiedName"].is_string(), "{json}");
            assert!(json.get("fully_qualified_name").is_none(), "{json}");
        }
    }
}
