//! Ties the wire format to the state machine: the accept loop, the
//! handshake, and one connection's message loop.
//!
//! Generic over [`Authenticator`] and [`QueryEngine`] — this module knows
//! nothing about JWTs, Postgres, or `graph-owl-api`. The composition root
//! (`graph-owl-server`) supplies adapters over `Catalog` and constructs the
//! one [`BoltServer`] the process runs.

use std::sync::Arc;

use graph_owl_core::Principal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use crate::auth::Authenticator;
use crate::chunking;
use crate::handshake;
use crate::limits::BoltLimits;
use crate::messages::{self, ClientMessage, ServerMessage};
use crate::packstream;
use crate::query::{QueryEngine, QueryError, RecordReceiver};
use crate::state::{self, Phase, Session};

/// Protocol versions this server negotiates. See `crate::messages`' module
/// doc for why the supported set is exactly one version today.
const SUPPORTED_VERSIONS: &[(u8, u8)] = &[(5, 0)];

pub struct BoltServer {
    auth: Arc<dyn Authenticator>,
    query: Arc<dyn QueryEngine>,
    limits: BoltLimits,
}

impl BoltServer {
    #[must_use]
    pub fn new(
        auth: Arc<dyn Authenticator>,
        query: Arc<dyn QueryEngine>,
        limits: BoltLimits,
    ) -> Self {
        Self {
            auth,
            query,
            limits,
        }
    }

    /// Accept connections until `shutdown` resolves, one task per
    /// connection.
    ///
    /// `max_connections` is enforced by a semaphore acquired before the
    /// handshake and held for the connection's whole life — refused
    /// immediately past the limit, never queued, the same posture
    /// `graph-owl-server::admission` takes for HTTP.
    pub async fn serve(
        self: Arc<Self>,
        listener: TcpListener,
        shutdown: impl std::future::Future<Output = ()>,
    ) {
        let admission = Arc::new(Semaphore::new(self.limits.max_connections));
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                accepted = listener.accept() => {
                    let Ok((socket, _addr)) = accepted else { continue };
                    let Ok(permit) = Arc::clone(&admission).try_acquire_owned() else {
                        drop(socket);
                        continue;
                    };
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        let _permit = permit;
                        handle_connection(socket, server.as_ref()).await;
                    });
                }
            }
        }
    }
}

async fn perform_handshake(socket: &mut tokio::net::TcpStream) -> bool {
    let mut magic = [0u8; 4];
    if socket.read_exact(&mut magic).await.is_err() || magic != handshake::MAGIC {
        // Wrong preamble: closed without a reply, per spec — this is not
        // Bolt, so there is nothing to negotiate a version with.
        return false;
    }
    let mut offer_bytes = [0u8; 16];
    if socket.read_exact(&mut offer_bytes).await.is_err() {
        return false;
    }
    #[allow(clippy::unwrap_used)] // fixed 4-byte slices of a fixed 16-byte array
    let offers: [[u8; 4]; 4] = [
        offer_bytes[0..4].try_into().unwrap(),
        offer_bytes[4..8].try_into().unwrap(),
        offer_bytes[8..12].try_into().unwrap(),
        offer_bytes[12..16].try_into().unwrap(),
    ];

    match handshake::negotiate(offers, SUPPORTED_VERSIONS) {
        Some((major, minor)) => socket
            .write_all(&handshake::encode_version(major, minor))
            .await
            .is_ok(),
        None => {
            let _ = socket.write_all(&handshake::NO_VERSION).await;
            false
        }
    }
}

async fn send(socket: &mut tokio::net::TcpStream, message: &ServerMessage) -> std::io::Result<()> {
    let value = messages::encode_server_message(message);
    let bytes = packstream::encode(&value);
    socket.write_all(&chunking::encode(&bytes)).await
}

/// Read one complete, chunk-reassembled message, or `None` once the socket
/// cannot yield one — EOF, a read error, or a chunked message over budget.
async fn read_message(
    socket: &mut tokio::net::TcpStream,
    decoder: &mut chunking::Decoder,
    buf: &mut [u8],
    max_message_bytes: usize,
) -> Option<Vec<u8>> {
    loop {
        match decoder.next_message(max_message_bytes) {
            Ok(Some(message)) => return Some(message),
            Ok(None) => {}
            Err(_oversized) => return None,
        }
        match socket.read(buf).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => decoder.feed(&buf[..n]),
        }
    }
}

fn failure_for(err: &QueryError) -> ServerMessage {
    let (code, message) = match err {
        QueryError::Refused(message) => {
            ("ClientError.Request.Invalid".to_string(), message.clone())
        }
        QueryError::Storage(message) => (
            "ClientError.Statement.ExecutionError".to_string(),
            message.clone(),
        ),
        QueryError::Timeout => (
            "ClientError.Statement.TimedOut".to_string(),
            "query exceeded its time budget".to_string(),
        ),
    };
    ServerMessage::Failure { code, message }
}

enum DrainOutcome {
    /// The receiver is exhausted — nothing left, `has_more: false`. Carries
    /// whatever lossy mappings the drained rows reported (Epic 7c decision
    /// 2) — possibly empty, which the caller renders as no `notifications`
    /// field at all rather than an empty list, matching a driver's
    /// expectation that the key is absent when there is nothing to say.
    Exhausted(Vec<graph_owl_lpg::LossyMapping>),
    /// The requested count was pulled but the receiver may hold more. Same
    /// accumulation as `Exhausted`, scoped to just this call's own rows.
    HasMore(Vec<graph_owl_lpg::LossyMapping>),
    Failed(QueryError),
}

/// Pull up to `limit` rows (`None` = drain to exhaustion) from `receiver`,
/// sending a `RECORD` per row when `emit_records` is set — `false` for
/// `DISCARD`, which consumes identically but transmits nothing.
async fn drain(
    socket: &mut tokio::net::TcpStream,
    receiver: &mut RecordReceiver,
    limit: Option<usize>,
    emit_records: bool,
) -> DrainOutcome {
    let mut count = 0usize;
    let mut lossy = Vec::new();
    loop {
        if limit.is_some_and(|limit| count >= limit) {
            return DrainOutcome::HasMore(lossy);
        }
        match receiver.recv().await {
            Some(Ok(row)) => {
                lossy.extend(row.lossy);
                if emit_records {
                    let values = row
                        .values
                        .into_iter()
                        .map(|(_, value)| value.into_bolt_value())
                        .collect();
                    if send(socket, &ServerMessage::Record(values)).await.is_err() {
                        return DrainOutcome::Exhausted(lossy);
                    }
                }
                count += 1;
            }
            Some(Err(err)) => return DrainOutcome::Failed(err),
            None => return DrainOutcome::Exhausted(lossy),
        }
    }
}

/// `has_more` plus, when there is anything to report, a `notifications`
/// list — one entry per lossy mapping accumulated over the rows this
/// `PULL`/`DISCARD` actually drained.
fn pull_summary_metadata(
    has_more: bool,
    lossy: Vec<graph_owl_lpg::LossyMapping>,
) -> Vec<(String, packstream::BoltValue)> {
    let mut metadata = vec![(
        "has_more".to_string(),
        packstream::BoltValue::Boolean(has_more),
    )];
    if !lossy.is_empty() {
        metadata.push((
            "notifications".to_string(),
            packstream::BoltValue::List(lossy.iter().map(messages::bolt_notification).collect()),
        ));
    }
    metadata
}

#[allow(clippy::cast_sign_loss)] // guarded: only called with n >= 0
fn pull_limit(n: i64) -> Option<usize> {
    if n < 0 { None } else { Some(n as usize) }
}

async fn handle_connection(mut socket: tokio::net::TcpStream, server: &BoltServer) {
    if !perform_handshake(&mut socket).await {
        return;
    }

    let mut session = Session::new();
    let mut decoder = chunking::Decoder::new();
    let mut read_buf = vec![0u8; 4096];
    let mut principal: Option<Principal> = None;
    let mut stream: Option<RecordReceiver> = None;

    loop {
        let Some(raw) = read_message(
            &mut socket,
            &mut decoder,
            &mut read_buf,
            server.limits.max_message_bytes,
        )
        .await
        else {
            return;
        };

        let value = match packstream::decode(&raw, server.limits.max_message_bytes) {
            Ok(Some((value, consumed))) if consumed == raw.len() => value,
            _ => {
                let _ = send(
                    &mut socket,
                    &ServerMessage::Failure {
                        code: "Protocol.Error".to_string(),
                        message: "malformed message".to_string(),
                    },
                )
                .await;
                return;
            }
        };

        let client_message = match messages::decode_client_message(value) {
            Ok(message) => message,
            Err(err) => {
                let _ = send(
                    &mut socket,
                    &ServerMessage::Failure {
                        code: "Protocol.Error".to_string(),
                        message: err.to_string(),
                    },
                )
                .await;
                return;
            }
        };

        match state::admit(session, client_message.kind()) {
            state::Outcome::Ignore => {
                let _ = send(&mut socket, &ServerMessage::Ignored).await;
                continue;
            }
            state::Outcome::Violation => {
                let _ = send(
                    &mut socket,
                    &ServerMessage::Failure {
                        code: "Request.Invalid".to_string(),
                        message: format!(
                            "{:?} is not legal in the current state",
                            client_message.kind()
                        ),
                    },
                )
                .await;
                return;
            }
            state::Outcome::Proceed => {}
        }

        match client_message {
            ClientMessage::Hello { credentials, .. } => {
                match server.auth.authenticate(&credentials).await {
                    Ok(resolved) => {
                        principal = Some(resolved);
                        session.phase = Phase::Authed;
                        let _ = send(&mut socket, &ServerMessage::Success(vec![])).await;
                    }
                    Err(err) => {
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Failure {
                                code: "Unauthorized".to_string(),
                                message: err.0,
                            },
                        )
                        .await;
                        return;
                    }
                }
            }
            ClientMessage::Run { query } => {
                let Some(principal) = principal.as_ref() else {
                    unreachable!("Authed phase guarantees a prior successful HELLO");
                };
                match tokio::time::timeout(
                    server.limits.query_timeout,
                    server.query.run(principal, &query),
                )
                .await
                {
                    Ok(Ok((outcome, receiver))) => {
                        session.phase = Phase::Streaming;
                        stream = Some(receiver);
                        let fields = packstream::BoltValue::List(
                            outcome
                                .fields
                                .into_iter()
                                .map(packstream::BoltValue::String)
                                .collect(),
                        );
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(vec![("fields".to_string(), fields)]),
                        )
                        .await;
                    }
                    Ok(Err(err)) => {
                        session.phase = Phase::Failed;
                        let _ = send(&mut socket, &failure_for(&err)).await;
                    }
                    Err(_elapsed) => {
                        session.phase = Phase::Failed;
                        let _ = send(&mut socket, &failure_for(&QueryError::Timeout)).await;
                    }
                }
            }
            ClientMessage::Pull { n } => {
                let Some(mut receiver) = stream.take() else {
                    unreachable!("Streaming phase guarantees an active receiver");
                };
                match drain(&mut socket, &mut receiver, pull_limit(n), true).await {
                    DrainOutcome::HasMore(lossy) => {
                        stream = Some(receiver);
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(pull_summary_metadata(true, lossy)),
                        )
                        .await;
                    }
                    DrainOutcome::Exhausted(lossy) => {
                        session.phase = Phase::Authed;
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(pull_summary_metadata(false, lossy)),
                        )
                        .await;
                    }
                    DrainOutcome::Failed(err) => {
                        session.phase = Phase::Failed;
                        let _ = send(&mut socket, &failure_for(&err)).await;
                    }
                }
            }
            ClientMessage::Discard { n } => {
                let Some(mut receiver) = stream.take() else {
                    unreachable!("Streaming phase guarantees an active receiver");
                };
                match drain(&mut socket, &mut receiver, pull_limit(n), false).await {
                    DrainOutcome::HasMore(lossy) => {
                        stream = Some(receiver);
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(pull_summary_metadata(true, lossy)),
                        )
                        .await;
                    }
                    DrainOutcome::Exhausted(lossy) => {
                        session.phase = Phase::Authed;
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(pull_summary_metadata(false, lossy)),
                        )
                        .await;
                    }
                    DrainOutcome::Failed(err) => {
                        session.phase = Phase::Failed;
                        let _ = send(&mut socket, &failure_for(&err)).await;
                    }
                }
            }
            ClientMessage::Begin => {
                session.in_transaction = true;
                let _ = send(&mut socket, &ServerMessage::Success(vec![])).await;
            }
            ClientMessage::Commit => {
                session.in_transaction = false;
                let _ = send(&mut socket, &ServerMessage::Success(vec![])).await;
            }
            ClientMessage::Rollback => {
                session.in_transaction = false;
                let _ = send(&mut socket, &ServerMessage::Success(vec![])).await;
            }
            ClientMessage::Reset => {
                stream = None;
                session = Session {
                    phase: if principal.is_some() {
                        Phase::Authed
                    } else {
                        Phase::Negotiation
                    },
                    in_transaction: false,
                };
                let _ = send(&mut socket, &ServerMessage::Success(vec![])).await;
            }
            ClientMessage::Goodbye => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_lpg::LossyMapping;

    fn has_key(metadata: &[(String, packstream::BoltValue)], key: &str) -> bool {
        metadata.iter().any(|(k, _)| k == key)
    }

    /// Epic 7c decision 2's "reported, never dropped silently" only matters
    /// when there is something to report — `pull_summary_metadata` is the
    /// one place that decides whether `notifications` appears at all, and
    /// the real Bolt wire tests (`graph-owl-server`'s `tests/bolt.rs`) only
    /// exercise it through a real socket, which `cargo mutants` cannot run
    /// against this crate alone. Unit-testing the decision directly here
    /// closes that gap rather than leaving it to an integration-only proof.
    #[test]
    fn an_empty_report_omits_the_notifications_key_entirely() {
        let metadata = pull_summary_metadata(false, Vec::new());
        assert!(
            !has_key(&metadata, "notifications"),
            "an empty list must be no key at all, not an empty list: {metadata:?}"
        );
    }

    #[test]
    fn a_non_empty_report_adds_a_notifications_list() {
        let lossy = vec![LossyMapping::TypeNarrowed {
            subject: "table-1".to_string(),
            predicate: "properties".to_string(),
            from: "json",
        }];
        let metadata = pull_summary_metadata(false, lossy);
        let notifications = metadata
            .iter()
            .find(|(k, _)| k == "notifications")
            .map(|(_, v)| v.clone());
        let Some(packstream::BoltValue::List(entries)) = notifications else {
            panic!("expected a notifications list: {metadata:?}");
        };
        assert_eq!(entries.len(), 1, "{entries:?}");
    }

    /// `has_more` is threaded through unconditionally, independent of
    /// whether anything was lossy — the two fields are orthogonal, and a
    /// mutant that only sets `has_more` when `lossy` is non-empty (or vice
    /// versa) must fail this.
    #[test]
    fn has_more_is_reported_regardless_of_whether_anything_was_lossy() {
        let with_loss = pull_summary_metadata(
            true,
            vec![LossyMapping::RefInProperty {
                subject: "s".to_string(),
                predicate: "p".to_string(),
            }],
        );
        let without_loss = pull_summary_metadata(true, Vec::new());
        assert_eq!(
            with_loss
                .iter()
                .find(|(k, _)| k == "has_more")
                .map(|(_, v)| v.clone()),
            Some(packstream::BoltValue::Boolean(true))
        );
        assert_eq!(
            without_loss
                .iter()
                .find(|(k, _)| k == "has_more")
                .map(|(_, v)| v.clone()),
            Some(packstream::BoltValue::Boolean(true))
        );
    }
}
