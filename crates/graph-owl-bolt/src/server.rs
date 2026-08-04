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
    /// The receiver is exhausted — nothing left, `has_more: false`.
    Exhausted,
    /// The requested count was pulled but the receiver may hold more.
    HasMore,
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
    loop {
        if limit.is_some_and(|limit| count >= limit) {
            return DrainOutcome::HasMore;
        }
        match receiver.recv().await {
            Some(Ok(row)) => {
                if emit_records {
                    let values = row
                        .0
                        .into_iter()
                        .map(|(_, value)| value.into_bolt_value())
                        .collect();
                    if send(socket, &ServerMessage::Record(values)).await.is_err() {
                        return DrainOutcome::Exhausted;
                    }
                }
                count += 1;
            }
            Some(Err(err)) => return DrainOutcome::Failed(err),
            None => return DrainOutcome::Exhausted,
        }
    }
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
                    DrainOutcome::HasMore => {
                        stream = Some(receiver);
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(vec![(
                                "has_more".to_string(),
                                packstream::BoltValue::Boolean(true),
                            )]),
                        )
                        .await;
                    }
                    DrainOutcome::Exhausted => {
                        session.phase = Phase::Authed;
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(vec![(
                                "has_more".to_string(),
                                packstream::BoltValue::Boolean(false),
                            )]),
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
                    DrainOutcome::HasMore => {
                        stream = Some(receiver);
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(vec![(
                                "has_more".to_string(),
                                packstream::BoltValue::Boolean(true),
                            )]),
                        )
                        .await;
                    }
                    DrainOutcome::Exhausted => {
                        session.phase = Phase::Authed;
                        let _ = send(
                            &mut socket,
                            &ServerMessage::Success(vec![(
                                "has_more".to_string(),
                                packstream::BoltValue::Boolean(false),
                            )]),
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
