//! MCP over stdio — Epic 14 Slice H.
//!
//! **A second, independent transport onto the same dispatch, not a rewrite
//! of `POST /mcp`.** Every protocol decision already lives in
//! `graph_owl_mcp::jsonrpc`, tested without a socket; this module owns only
//! what is genuinely different about speaking JSON-RPC over a byte stream
//! instead of HTTP request/response pairs — newline framing, and how a
//! session's identity is established once at the start rather than read off
//! a header on every call.
//!
//! **Why a session has one identity, not one per message.** HTTP carries a
//! credential on every request because there is no negotiation phase during
//! which a client legitimately has none (`mcp_endpoint`'s own doc comment).
//! stdio is the opposite: a client spawns one process per session, and
//! there is no per-message header to carry a credential on even if the
//! protocol wanted one. So the operator names who this *process* acts as,
//! once, at start.
//!
//! **"Defaults closed" is not new logic — it is reusing the gate that
//! already exists.** `jsonrpc::handle` already refuses every tool call when
//! its `principal` argument is `None`, proven by `jsonrpc.rs`'s own
//! `an_unauthenticated_call_is_refused_without_naming_tools`. An
//! unconfigured stdio session simply never has anything to pass but `None`
//! — there is no second authorization path to build or to get wrong.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Run one stdio session to completion.
///
/// Reads newline-delimited JSON-RPC requests from `input`, dispatches each
/// through `server` as `who`, and writes each response as its own
/// newline-terminated line to `output`, flushed immediately — a client
/// reading a pipe has no other signal that a reply is complete. Returns once
/// `input` reaches EOF, which is how a client ends a session: it closes its
/// end of the pipe rather than sending a farewell message.
///
/// A blank line between messages is not an error — some clients pad frames
/// — and is silently skipped. A line that is not valid JSON-RPC still gets
/// an answer: [`graph_owl_mcp::jsonrpc::parse`]'s own `-32700` response,
/// which this function writes exactly like any other.
///
/// # Errors
///
/// An `io::Error` from the underlying reader or writer — a broken pipe, most
/// realistically, since a client killing the process mid-response is not
/// this transport's decision to recover from.
pub async fn run_session<R, W>(
    server: &graph_owl_mcp::jsonrpc::Server<'_>,
    who: Option<&str>,
    mut input: R,
    mut output: W,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        // `read_line` appends, hence the `clear()` above — and its count
        // includes the newline itself, so `0` (not an empty string) is the
        // real EOF signal; a genuinely empty message is still `"\n"`, one
        // byte read.
        let bytes_read = input.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match graph_owl_mcp::jsonrpc::parse(trimmed.as_bytes()) {
            Ok(request) => graph_owl_mcp::jsonrpc::handle(server, who, &request).await,
            Err(parse_error) => Some(parse_error),
        };

        // A notification produces no response at all (`handle` already
        // returns `None` for one) — silence on this transport means exactly
        // what it means on HTTP's `204`: nothing was owed.
        if let Some(response) = response {
            let text = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
            output.write_all(text.as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use graph_owl_mcp::{
        AssetContext, ContextSource, Direction, EvidenceContext, FactExplanation, MemoryContext,
        SearchResults, SourceError, TraversalContext,
    };
    use serde_json::{Value, json};
    use std::io::Cursor;

    /// A minimal `ContextSource` — this module's own tests are about
    /// framing, not protocol behaviour (`jsonrpc.rs` already proves that
    /// exhaustively), so the fixture only needs to distinguish "the
    /// authenticated caller asked for the one asset that exists" from
    /// everything else.
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
                description: None,
                related: vec![],
                policy_filtered: false,
                trust: graph_owl_mcp::trust::summarise(
                    &graph_owl_mcp::trust::Observed::default(),
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
        ) -> Result<Option<graph_owl_mcp::lineage::LineageWalk>, SourceError> {
            Ok(None)
        }
        async fn impact(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<graph_owl_mcp::lineage::ImpactReport>, SourceError> {
            Ok(None)
        }
        async fn governance(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<graph_owl_mcp::lineage::GovernanceContext>, SourceError> {
            Ok(None)
        }
        async fn query_graph(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Result<graph_owl_mcp::QueryAnswer, graph_owl_mcp::QueryFault>, SourceError>
        {
            Ok(Ok(graph_owl_mcp::QueryAnswer::default()))
        }
        async fn run_pack_query(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &std::collections::BTreeMap<String, String>,
        ) -> Result<
            Result<Option<graph_owl_mcp::QueryAnswer>, graph_owl_mcp::QueryFault>,
            SourceError,
        > {
            Ok(Ok(None))
        }
        async fn traverse(
            &self,
            _: &str,
            _: &str,
            _: Direction,
            _: u32,
        ) -> Result<Option<TraversalContext>, SourceError> {
            Ok(None)
        }
        async fn find_evidence(
            &self,
            _: &str,
            _: uuid::Uuid,
            _: u32,
        ) -> Result<Option<EvidenceContext>, SourceError> {
            Ok(None)
        }
        async fn explain(
            &self,
            _: &str,
            _: &graph_owl_core::flake::Sid,
            _: &graph_owl_core::flake::Sid,
            _: &graph_owl_core::flake::Sid,
        ) -> Result<Option<FactExplanation>, SourceError> {
            Ok(None)
        }
        async fn reconcile(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<graph_owl_api::ReconcileOutcome>, SourceError> {
            Ok(None)
        }
        async fn analytics(
            &self,
            _: &str,
            _: &str,
            _: Direction,
            _: u32,
        ) -> Result<Option<graph_owl_mcp::AnalyticsContext>, SourceError> {
            Ok(None)
        }
        async fn run_rule(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Option<graph_owl_api::ReconcileOutcome>, SourceError> {
            Ok(None)
        }
        async fn resolve_entity(
            &self,
            _: &str,
            _: &str,
            _: usize,
        ) -> Result<graph_owl_mcp::ResolvedEntityContext, SourceError> {
            Ok(graph_owl_mcp::ResolvedEntityContext::default())
        }
        async fn calculate_risk(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Vec<graph_owl_api::Obligation>, SourceError> {
            Ok(Vec::new())
        }
    }

    fn read_only() -> graph_owl_mcp::jsonrpc::Server<'static> {
        graph_owl_mcp::jsonrpc::Server {
            reads: &Fixture,
            writes: None,
            budget: graph_owl_mcp::budget::TokenBudget::default(),
        }
    }

    fn request(id: i64, method: &str, params: &Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    /// Splits stdout back into the JSON documents it wrote — one per line,
    /// which is the contract [`run_session`] promises its own caller.
    fn responses(written: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(written)
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON: {line}")))
            .collect()
    }

    #[tokio::test]
    async fn an_empty_session_produces_no_output() {
        let server = read_only();
        let mut out = Vec::new();

        run_session(&server, Some("alice"), Cursor::new(b"" as &[u8]), &mut out)
            .await
            .expect("EOF ends the session cleanly");

        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn a_single_line_gets_a_single_newline_terminated_response() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!("{}\n", request(1, "ping", &json!({})));

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        assert!(
            out.ends_with(b"\n"),
            "a reader waiting on read_line must see a terminator: {out:?}"
        );
        let parsed = responses(&out);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["id"], 1);
    }

    /// **The negative that matters for framing specifically**: three
    /// requests in one input must come back as three separate lines, in
    /// order — not concatenated, not merged, not reordered.
    #[tokio::test]
    async fn several_lines_each_get_their_own_response_in_order() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!(
            "{}\n{}\n{}\n",
            request(1, "ping", &json!({})),
            request(2, "ping", &json!({})),
            request(3, "ping", &json!({})),
        );

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0]["id"], 1);
        assert_eq!(parsed[1]["id"], 2);
        assert_eq!(parsed[2]["id"], 3);
    }

    /// A blank line between messages — padding some clients emit — is
    /// silently skipped, not a parse error, and does not stall the session.
    #[tokio::test]
    async fn a_blank_line_is_skipped_not_answered() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!("\n{}\n\n", request(1, "ping", &json!({})));

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed.len(), 1, "{parsed:?}");
        assert_eq!(parsed[0]["id"], 1);
    }

    /// A line that is not JSON at all still gets an answer — the `-32700`
    /// [`graph_owl_mcp::jsonrpc::parse`] already produces, written exactly
    /// like any other response rather than dropped or panicking the session.
    #[tokio::test]
    async fn a_malformed_line_gets_a_parse_error_response() {
        let server = read_only();
        let mut out = Vec::new();
        let input = "not json at all\n";

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.as_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["error"]["code"], -32700, "{parsed:?}");
    }

    /// One malformed line must not end the session — the next, valid line
    /// still gets answered.
    #[tokio::test]
    async fn a_malformed_line_does_not_stop_the_session() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!("not json\n{}\n", request(2, "ping", &json!({})));

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["error"]["code"], -32700);
        assert_eq!(parsed[1]["id"], 2);
    }

    /// **The property this whole module exists to preserve**: an
    /// unconfigured session (`who: None`) refuses a tool call rather than
    /// running open — reusing `jsonrpc::handle`'s own already-tested gate,
    /// proven here only at the level this module adds anything: that
    /// `run_session` actually threads `who` through rather than
    /// substituting some other value.
    #[tokio::test]
    async fn an_unconfigured_session_refuses_a_tool_call() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!(
            "{}\n",
            request(
                1,
                "tools/call",
                &json!({
                    "name": graph_owl_mcp::GET_ASSET_CONTEXT,
                    "arguments": { "fullyQualifiedName": "warehouse.orders" }
                }),
            )
        );

        run_session(&server, None, Cursor::new(input.into_bytes()), &mut out)
            .await
            .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed[0]["result"]["isError"], true, "{parsed:?}");
        let text = parsed[0]["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("authenticated"), "{text}");
    }

    /// And the positive that makes the negative above mean something: a
    /// configured session's identity really does reach the call and
    /// succeed.
    #[tokio::test]
    async fn a_configured_session_can_complete_a_real_call() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!(
            "{}\n",
            request(
                1,
                "tools/call",
                &json!({
                    "name": graph_owl_mcp::GET_ASSET_CONTEXT,
                    "arguments": { "fullyQualifiedName": "warehouse.orders" }
                }),
            )
        );

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(parsed[0]["result"]["isError"], false, "{parsed:?}");
    }

    /// A notification (no `id`) produces no line at all — `handle` already
    /// returns `None` for one, and this transport's silence means the same
    /// thing HTTP's `204` does.
    #[tokio::test]
    async fn a_notification_produces_no_output_line() {
        let server = read_only();
        let mut out = Vec::new();
        let input = format!(
            "{}\n{}\n",
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            request(2, "ping", &json!({})),
        );

        run_session(
            &server,
            Some("alice"),
            Cursor::new(input.into_bytes()),
            &mut out,
        )
        .await
        .expect("session completes");

        let parsed = responses(&out);
        assert_eq!(
            parsed.len(),
            1,
            "the notification wrote nothing, only the ping did: {parsed:?}"
        );
        assert_eq!(parsed[0]["id"], 2);
    }
}
