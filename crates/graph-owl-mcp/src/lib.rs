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
}

/// What a tool call produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Found(Box<AssetContext>),
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

/// Everything this server offers.
///
/// Slice A declares exactly one. A surface advertising tools it cannot serve
/// teaches an agent to distrust the manifest, and an agent that distrusts the
/// manifest probes instead — the behaviour a read-only surface least wants to
/// encourage.
#[must_use]
pub fn tools() -> Vec<ToolDeclaration> {
    vec![ToolDeclaration {
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
    }]
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

    if tool != GET_ASSET_CONTEXT {
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
                }));
            }
            Ok(None)
        }
    }

    fn args(fqn: &str) -> serde_json::Value {
        serde_json::json!({ "fullyQualifiedName": fqn })
    }

    mod what_the_manifest_declares {
        use super::*;

        #[test]
        fn the_declared_tool_is_the_one_that_can_be_called() {
            let declared = tools();

            assert_eq!(declared.len(), 1);
            assert_eq!(declared[0].name, GET_ASSET_CONTEXT);
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
            })
            .expect("serialises");

            assert!(json["fullyQualifiedName"].is_string(), "{json}");
            assert!(json.get("fully_qualified_name").is_none(), "{json}");
        }
    }
}
