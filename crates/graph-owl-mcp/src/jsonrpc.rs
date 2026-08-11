//! The MCP wire protocol: JSON-RPC 2.0 — Epic 14, the transport Slices A–G left out.
//!
//! Pure. Given a request document and the two ports, produces a response
//! document. No socket, no framework — which is what lets the protocol's sharp
//! edges be tested without standing up a server, and what keeps the HTTP layer
//! a shell with no decisions in it.
//!
//! **The distinction this module exists to preserve: a tool that refused is not
//! a protocol that failed.** JSON-RPC has an `error` member, and it is tempting
//! to put "asset not found" there. That would be wrong, and expensively so — a
//! JSON-RPC error means *the call could not be made*, so a client retries it,
//! reconnects, or reports the server as broken. A tool that ran and answered
//! "no such asset" has succeeded at the protocol level. MCP models this
//! correctly: the result carries `isError`, and the transport error member is
//! reserved for malformed requests and unknown methods.
//!
//! Getting it backwards means an agent treats a policy denial as an outage.

use serde_json::{Value, json};

/// The protocol version this server speaks.
///
/// **Taken from `rmcp` rather than written here**, and that is the whole
/// reason the dependency exists. This was a hand-pinned `"2024-11-05"` and
/// MCP had moved on three revisions without anything noticing — a string
/// constant cannot tell you it has gone stale, and a client negotiating
/// against a two-year-old version either refuses or silently degrades.
///
/// Still **pinned rather than echoed**: echoing whatever a client asked for
/// would claim conformance to a protocol this server has never seen.
/// `LATEST`, not `STANDARD_HEADERS` (`rmcp`'s newest revision, 2026-07-28,
/// gated on SEP-2243) — this server does not implement that revision's
/// stateless-HTTP/MRTR/subscriptions surface, and advertising it would be
/// the same false conformance claim the pin exists to prevent.
#[must_use]
pub fn protocol_version() -> String {
    rmcp::model::ProtocolVersion::LATEST.to_string()
}

/// JSON-RPC 2.0's reserved error codes.
///
/// Only the ones this server can actually produce. A code it never emits would
/// be a branch nothing tests.
mod code {
    /// The body was not JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// It was JSON, but not a JSON-RPC request.
    pub const INVALID_REQUEST: i64 = -32600;
    /// No such method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The method exists and its params do not fit.
    pub const INVALID_PARAMS: i64 = -32602;
}

/// What the caller is allowed to reach.
///
/// **The write half is `Option`**, and that is the deployment control: a server
/// constructed without a [`crate::write::WriteSink`] does not merely refuse
/// write tools, it does not declare them — so an agent never learns they might
/// exist. Epic 32's capability checks are the second line; this is the first.
pub struct Server<'a> {
    pub reads: &'a dyn crate::ContextSource,
    pub writes: Option<&'a dyn crate::write::WriteSink>,
    pub budget: crate::budget::TokenBudget,
}

/// Handle one request document.
///
/// Returns `None` for a **notification** — a request with no `id`. JSON-RPC
/// requires silence there, and answering one is a protocol violation that some
/// clients treat as a fatal desync rather than as noise.
pub async fn handle(
    server: &Server<'_>,
    principal: crate::Principal<'_>,
    request: &Value,
) -> Option<Value> {
    // A batch is an array. Supported because the spec requires it, and because
    // an agent issuing five tool calls in one round trip is exactly the traffic
    // shape a task-shaped surface produces.
    if let Value::Array(calls) = request {
        if calls.is_empty() {
            // An empty batch is an invalid request, not an empty batch response
            // — the spec is explicit, and a client that gets `[]` back cannot
            // tell it from "all your calls were notifications".
            return Some(error_response(
                &Value::Null,
                code::INVALID_REQUEST,
                "an empty batch is not a valid request",
            ));
        }
        let mut responses = Vec::new();
        for call in calls {
            if let Some(response) = Box::pin(handle_one(server, principal, call)).await {
                responses.push(response);
            }
        }
        // A batch consisting only of notifications gets no response at all.
        return (!responses.is_empty()).then_some(Value::Array(responses));
    }

    handle_one(server, principal, request).await
}

async fn handle_one(
    server: &Server<'_>,
    principal: crate::Principal<'_>,
    request: &Value,
) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let is_notification = request.get("id").is_none();

    // `jsonrpc: "2.0"` is required. Checked rather than assumed, because a
    // client sending 1.0 shapes will otherwise get answers it cannot parse and
    // will report *this* server as broken.
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return (!is_notification).then(|| {
            error_response(
                &id,
                code::INVALID_REQUEST,
                "`jsonrpc` must be exactly \"2.0\"",
            )
        });
    }

    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return (!is_notification).then(|| {
            error_response(
                &id,
                code::INVALID_REQUEST,
                "`method` is required, as a string",
            )
        });
    };

    let params = request.get("params").cloned().unwrap_or(json!({}));
    let outcome = dispatch(server, principal, method, &params).await;

    // **A notification gets no reply, even when it failed.** Suppressed here,
    // after dispatch, so the work still happens — a client that fires
    // `notifications/initialized` expects the server to have noticed.
    if is_notification {
        return None;
    }

    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(&id, code, &message),
    })
}

/// Route one method.
async fn dispatch(
    server: &Server<'_>,
    principal: crate::Principal<'_>,
    method: &str,
    params: &Value,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": protocol_version(),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "graph-owl", "version": env!("CARGO_PKG_VERSION") },
        })),

        // `notifications/initialized` is the client telling us it is ready —
        // nothing to do, and answering it would be the protocol violation
        // described above. `ping` is a liveness check with the same empty
        // result. Grouped because the *response* is identical; they are listed
        // together rather than merged silently so a reader sees both are served.
        "notifications/initialized" | "initialized" | "ping" => Ok(json!({})),

        "tools/list" => Ok(json!({ "tools": declared(server) })),

        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Err((
                    code::INVALID_PARAMS,
                    "`name` is required, as a string".to_string(),
                ));
            };
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(call_tool(server, principal, name, &arguments).await)
        }

        other => Err((code::METHOD_NOT_FOUND, format!("no method named `{other}`"))),
    }
}

/// Every tool this server will actually serve.
///
/// **The write tools appear only when a sink is wired.** A manifest advertising
/// a tool that cannot run teaches an agent to distrust the manifest and probe
/// instead, which is the behaviour a governance surface least wants to encourage.
fn declared(server: &Server<'_>) -> Vec<Value> {
    let mut tools: Vec<Value> = crate::tools()
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        })
        .collect();
    if server.writes.is_some() {
        tools.extend(crate::write::write_tools().into_iter().map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        }));
    }
    tools
}

/// Run one tool and render it as an MCP result.
///
/// Always `Ok`: every failure an agent should see is a tool result with
/// `isError`, never a JSON-RPC error. See the module docs for why that
/// distinction is load-bearing.
async fn call_tool(
    server: &Server<'_>,
    principal: crate::Principal<'_>,
    name: &str,
    arguments: &Value,
) -> Value {
    let outcome = if crate::write::is_write_tool(name) {
        let Some(writes) = server.writes else {
            // Reached only if a client calls a tool it was never offered.
            return tool_error("this server is read-only; no write tools are available");
        };
        let Some(agent_id) = principal else {
            return tool_error("this session is not authenticated");
        };
        crate::write::call_write(writes, agent_id, name, arguments).await
    } else {
        crate::call_within(server.reads, principal, name, arguments, server.budget).await
    };

    render(outcome)
}

/// An [`crate::Outcome`] as an MCP tool result.
///
/// The payload goes in as **text containing JSON**, which is MCP's content
/// model: a client renders it, and a language model reads it. A structured
/// content block would be a different protocol.
fn render(outcome: crate::Outcome) -> Value {
    use crate::Outcome;

    let (payload, is_error) = match outcome {
        Outcome::Found(context) => (serde_json::to_value(*context).unwrap_or(Value::Null), false),
        Outcome::Recalled(memories) => (
            json!({ "memories": memories, "count": memories.len() }),
            false,
        ),
        Outcome::Searched(results) => {
            (serde_json::to_value(*results).unwrap_or(Value::Null), false)
        }
        Outcome::Lineage(walk) => (serde_json::to_value(*walk).unwrap_or(Value::Null), false),
        Outcome::Impact(report) => (serde_json::to_value(*report).unwrap_or(Value::Null), false),
        Outcome::Governance(context) => {
            (serde_json::to_value(*context).unwrap_or(Value::Null), false)
        }
        Outcome::Bindings(answer) => (serde_json::to_value(*answer).unwrap_or(Value::Null), false),
        Outcome::Traversed(context) => {
            (serde_json::to_value(*context).unwrap_or(Value::Null), false)
        }
        Outcome::Wrote(receipt) => (serde_json::to_value(*receipt).unwrap_or(Value::Null), false),

        // **Absent and denied are one answer**, and the text says so without
        // implying which — the property Slice A's whole design rests on, carried
        // through to the wire unchanged.
        Outcome::NotFound => (
            json!({ "error": "no such entity, or it is not visible to you" }),
            true,
        ),
        Outcome::Unauthenticated => (
            json!({ "error": "this session is not authenticated" }),
            true,
        ),
        Outcome::BadRequest(why) => (json!({ "error": why }), true),
        Outcome::Refused(why) => (json!({ "error": why, "kind": "refused" }), true),
        Outcome::Unsupported(why) => (json!({ "error": why, "kind": "unsupported" }), true),
        Outcome::Unavailable(why) => (json!({ "error": why, "kind": "unavailable" }), true),
    };

    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()),
        }],
        "isError": is_error,
    })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": json!({ "error": message }).to_string() }],
        "isError": true,
    })
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// Parse a request body, or the JSON-RPC parse error for it.
///
/// Separate from [`handle`] so the HTTP layer has nothing to decide: it hands
/// over bytes and forwards whatever comes back.
///
/// # Errors
///
/// The `-32700` response document, ready to send.
pub fn parse(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice(body)
        .map_err(|error| error_response(&Value::Null, code::PARSE_ERROR, &error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssetContext, ContextSource, Direction, MemoryContext, SearchResults, SourceError,
    };
    use async_trait::async_trait;

    struct Fixture;

    #[async_trait]
    impl ContextSource for Fixture {
        async fn asset_context(
            &self,
            principal: &str,
            fqn: &str,
        ) -> Result<Option<AssetContext>, SourceError> {
            if principal != "alice" || fqn != "warehouse.orders" {
                return Ok(None);
            }
            Ok(Some(AssetContext {
                fully_qualified_name: fqn.to_string(),
                kind: "table".to_string(),
                description: Some("customer orders".to_string()),
                related: vec![],
                policy_filtered: false,
                trust: crate::trust::summarise(
                    &crate::trust::Observed::default(),
                    chrono::Utc::now(),
                ),
                truncated: false,
                truncation_reason: None,
            }))
        }
        async fn recall(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Option<Vec<MemoryContext>>, SourceError> {
            Ok(Some(Vec::new()))
        }
        async fn search(
            &self,
            _: &str,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<SearchResults, SourceError> {
            Ok(SearchResults::default())
        }
        async fn lineage(
            &self,
            _: &str,
            _: &str,
            _: Direction,
        ) -> Result<Option<crate::lineage::LineageWalk>, SourceError> {
            Ok(None)
        }
        async fn impact(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<crate::lineage::ImpactReport>, SourceError> {
            Ok(None)
        }
        async fn governance(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<crate::lineage::GovernanceContext>, SourceError> {
            Ok(None)
        }
        async fn query_graph(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Result<crate::QueryAnswer, crate::QueryFault>, SourceError> {
            Ok(Ok(crate::QueryAnswer::default()))
        }
        async fn traverse(
            &self,
            _: &str,
            _: &str,
            _: Direction,
            _: u32,
        ) -> Result<Option<crate::TraversalContext>, SourceError> {
            Ok(None)
        }
    }

    struct Sink;

    #[async_trait]
    impl crate::write::WriteSink for Sink {
        async fn write(
            &self,
            _: &str,
            _: graph_owl_authz::agent::AgentCapability,
            target_fqn: &str,
            _: serde_json::Value,
            _: &str,
            _: f64,
        ) -> Result<Result<crate::write::WriteReceipt, String>, SourceError> {
            Ok(Ok(crate::write::WriteReceipt {
                outcome: "proposed",
                proposal_id: Some("p-1".to_string()),
                target_fqn: target_fqn.to_string(),
                reason: "test".to_string(),
            }))
        }
    }

    fn read_only() -> Server<'static> {
        Server {
            reads: &Fixture,
            writes: None,
            budget: crate::budget::TokenBudget::default(),
        }
    }

    fn read_write() -> Server<'static> {
        Server {
            reads: &Fixture,
            writes: Some(&Sink),
            budget: crate::budget::TokenBudget::default(),
        }
    }

    async fn ask(server: &Server<'_>, request: Value) -> Value {
        handle(server, Some("alice"), &request)
            .await
            .expect("a response")
    }

    // ---- framing ----

    #[tokio::test]
    async fn initialize_reports_the_protocol_version_and_the_server() {
        let response = ask(
            &read_only(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        )
        .await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], protocol_version());
        assert_eq!(response["result"]["serverInfo"]["name"], "graph-owl");
    }

    /// **The version is a real MCP version, and current.**
    ///
    /// It was a hand-written `"2024-11-05"` that had gone three revisions stale
    /// without anything noticing. Asserting against `ProtocolVersion::latest()`
    /// means the constant cannot rot again — and asserting it is *not* the old
    /// one is what makes this test say something rather than tautologise.
    #[test]
    fn the_advertised_protocol_version_is_current() {
        let advertised = protocol_version();

        assert_eq!(advertised, rmcp::model::ProtocolVersion::LATEST.to_string());
        assert_ne!(
            advertised, "2024-11-05",
            "the version this server pinned by hand before the schema crate \
             was adopted"
        );
    }

    /// **The version is pinned, not echoed.** Echoing whatever a client asked
    /// for would claim conformance to a protocol this server has never seen.
    #[tokio::test]
    async fn the_protocol_version_is_not_echoed_back_from_the_request() {
        let response = ask(
            &read_only(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "1999-01-01" }
            }),
        )
        .await;

        assert_eq!(response["result"]["protocolVersion"], protocol_version());
    }

    #[tokio::test]
    async fn an_unknown_method_is_a_protocol_error() {
        let response = ask(
            &read_only(),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "drop/everything" }),
        )
        .await;

        assert_eq!(response["error"]["code"], -32601);
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("drop/everything")),
            "{response}"
        );
        assert!(response.get("result").is_none(), "{response}");
    }

    #[tokio::test]
    async fn a_request_without_the_version_marker_is_invalid() {
        let response = ask(&read_only(), json!({ "id": 1, "method": "ping" })).await;

        assert_eq!(response["error"]["code"], -32600);
    }

    /// **A notification gets no reply at all** — JSON-RPC requires silence, and
    /// some clients treat an unexpected response as a fatal desync.
    #[tokio::test]
    async fn a_notification_gets_no_response() {
        let silence = handle(
            &read_only(),
            Some("alice"),
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await;

        assert!(silence.is_none());
    }

    /// And a notification that *fails* is still silent — the failure has
    /// nowhere to go, and inventing an id to report it against is worse.
    #[tokio::test]
    async fn even_a_failing_notification_is_silent() {
        let silence = handle(
            &read_only(),
            Some("alice"),
            &json!({ "jsonrpc": "2.0", "method": "no/such/method" }),
        )
        .await;

        assert!(silence.is_none());
    }

    #[tokio::test]
    async fn a_batch_is_answered_as_a_batch() {
        let response = handle(
            &read_only(),
            Some("alice"),
            &json!([
                { "jsonrpc": "2.0", "id": 1, "method": "ping" },
                { "jsonrpc": "2.0", "id": 2, "method": "tools/list" },
            ]),
        )
        .await
        .expect("a batch response");

        let entries = response.as_array().expect("an array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], 1);
        assert_eq!(entries[1]["id"], 2);
    }

    /// A batch of only notifications gets nothing back, for the same reason a
    /// single notification does.
    #[tokio::test]
    async fn a_batch_of_only_notifications_is_silent() {
        let silence = handle(
            &read_only(),
            Some("alice"),
            &json!([{ "jsonrpc": "2.0", "method": "ping" }]),
        )
        .await;

        assert!(silence.is_none());
    }

    #[tokio::test]
    async fn an_empty_batch_is_an_invalid_request() {
        let response = handle(&read_only(), Some("alice"), &json!([]))
            .await
            .expect("a response");

        assert_eq!(response["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn a_body_that_is_not_json_is_a_parse_error() {
        let problem = parse(b"{not json").expect_err("a parse error");

        assert_eq!(problem["error"]["code"], -32700);
        assert_eq!(problem["id"], Value::Null);
    }

    // ---- the distinction that matters ----

    /// **A tool that refused is not a protocol that failed.**
    ///
    /// `NotFound` comes back as a successful JSON-RPC response whose *result*
    /// says `isError`. Put in the JSON-RPC `error` member instead, a client
    /// would retry, reconnect, or report the server as broken — an agent would
    /// read a policy denial as an outage.
    #[tokio::test]
    async fn a_tool_failure_is_a_result_not_a_transport_error() {
        let response = ask(
            &read_only(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": crate::GET_ASSET_CONTEXT,
                    "arguments": { "fullyQualifiedName": "finance.salaries" }
                }
            }),
        )
        .await;

        assert!(
            response.get("error").is_none(),
            "a denial is not a transport failure: {response}"
        );
        assert_eq!(response["result"]["isError"], true);
    }

    /// Whereas a malformed *call* — no tool name — is a protocol error, because
    /// no tool ran.
    #[tokio::test]
    async fn a_call_with_no_tool_name_is_a_protocol_error() {
        let response = ask(
            &read_only(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {} }),
        )
        .await;

        assert_eq!(response["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn a_successful_tool_call_carries_its_payload_as_text() {
        let response = ask(
            &read_only(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": crate::GET_ASSET_CONTEXT,
                    "arguments": { "fullyQualifiedName": "warehouse.orders" }
                }
            }),
        )
        .await;

        assert_eq!(response["result"]["isError"], false);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text content");
        let payload: Value = serde_json::from_str(text).expect("the text is JSON");
        assert_eq!(payload["fullyQualifiedName"], "warehouse.orders");
    }

    // ---- what the manifest offers ----

    /// **A read-only deployment does not declare write tools**, so an agent
    /// never learns they might exist. Epic 32's capability checks are the second
    /// line of defence; this is the first.
    #[tokio::test]
    async fn a_read_only_server_declares_no_write_tools() {
        let response = ask(
            &read_only(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .await;

        let names: Vec<&str> = response["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();

        assert_eq!(names.len(), 8, "the eight read tools: {names:?}");
        assert!(!names.iter().any(|name| crate::write::is_write_tool(name)));
    }

    #[tokio::test]
    async fn a_writable_server_declares_both_halves() {
        let response = ask(
            &read_write(),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .await;

        let names: Vec<&str> = response["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();

        assert_eq!(names.len(), 14, "eight read plus six write: {names:?}");
        assert!(names.contains(&crate::write::RECORD_MEMORY));
    }

    /// And a write tool called against a read-only server is refused as a tool
    /// error — reachable only if a client calls something it was never offered.
    #[tokio::test]
    async fn a_write_tool_on_a_read_only_server_is_refused() {
        let response = ask(
            &read_only(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": crate::write::RECORD_MEMORY,
                    "arguments": {
                        "fullyQualifiedName": "warehouse.orders",
                        "content": "x", "rationale": "y", "confidence": 0.9
                    }
                }
            }),
        )
        .await;

        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("read-only"), "{text}");
    }

    #[tokio::test]
    async fn a_write_tool_on_a_writable_server_runs() {
        let response = ask(
            &read_write(),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": crate::write::RECORD_MEMORY,
                    "arguments": {
                        "fullyQualifiedName": "warehouse.orders",
                        "content": "the loader drops refunds",
                        "rationale": "the row counts differ by exactly the refund count",
                        "confidence": 0.9
                    }
                }
            }),
        )
        .await;

        assert_eq!(response["result"]["isError"], false, "{response}");
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("proposed"), "{text}");
    }

    /// **Every declared tool is callable**, or the manifest is a lie — the same
    /// property the in-process surface asserts, now checked through the wire.
    #[tokio::test]
    async fn every_declared_tool_can_be_called_over_the_wire() {
        let server = read_write();
        let listed = ask(
            &server,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .await;

        for tool in listed["result"]["tools"].as_array().expect("tools") {
            let name = tool["name"].as_str().expect("a name");
            let response = ask(
                &server,
                json!({
                    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": { "name": name, "arguments": {} }
                }),
            )
            .await;
            // It may well refuse for want of arguments — what it must not do is
            // report that no such tool exists.
            assert!(
                response.get("error").is_none(),
                "{name} is advertised and not routable: {response}"
            );
        }
    }

    // ---- authentication ----

    /// **An unauthenticated session learns nothing, including which tools
    /// exist.** The in-process surface checks this before the tool name; the
    /// wire must not undo it.
    #[tokio::test]
    async fn an_unauthenticated_call_is_refused_without_naming_tools() {
        let response = handle(
            &read_only(),
            None,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": crate::GET_ASSET_CONTEXT, "arguments": {} }
            }),
        )
        .await
        .expect("a response");

        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("authenticated"), "{text}");
    }

    /// An unauthenticated *write* is refused before it reaches the sink.
    #[tokio::test]
    async fn an_unauthenticated_write_never_reaches_the_sink() {
        let response = handle(
            &read_write(),
            None,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {
                    "name": crate::write::RECORD_MEMORY,
                    "arguments": {
                        "fullyQualifiedName": "warehouse.orders",
                        "content": "x", "rationale": "y", "confidence": 0.9
                    }
                }
            }),
        )
        .await
        .expect("a response");

        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("authenticated"), "{text}");
    }

    /// `initialize` and `tools/list` are reachable unauthenticated, deliberately:
    /// a client has to negotiate before it can present a credential, and the
    /// manifest describes capabilities rather than data.
    #[tokio::test]
    async fn negotiation_does_not_require_a_credential() {
        for method in ["initialize", "ping", "tools/list"] {
            let response = handle(
                &read_only(),
                None,
                &json!({ "jsonrpc": "2.0", "id": 1, "method": method }),
            )
            .await
            .expect("a response");

            assert!(response.get("result").is_some(), "{method}: {response}");
        }
    }
}
