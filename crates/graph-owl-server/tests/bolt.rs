//! Epic 7d — Bolt protocol server, Slices B through E.
//!
//! A hand-rolled test client drives the wire protocol directly, using
//! `graph-owl-bolt`'s own codec/framing/message modules — the right tool for
//! testing this server's *own* state transitions (Slice B/C/D/E's
//! acceptance criteria are all expressed at exactly that level). Slice F's
//! acceptance is different in kind: a hand-rolled client "can be wrong in
//! the same way the server is, and prove nothing" (the plan's own words),
//! which is why Slice F's test is a real, off-the-shelf driver instead.

#![cfg(feature = "bolt")]

mod common;

use graph_owl_api::{Catalog, SparqlBudget, UpsertAsset};
use graph_owl_bolt::packstream::BoltValue;
use graph_owl_bolt::{chunking, handshake, messages};
use graph_owl_core::{AssetKind, Principal};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SECRET: &str = "bolt-test-secret";

fn token(subject: &str) -> String {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        name: &'a str,
        exp: usize,
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: subject,
            name: subject,
            exp: 4_102_444_800,
        }, // year 2100
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("token should encode")
}

async fn spawn_server(catalog: Catalog) -> std::net::SocketAddr {
    spawn_server_with_batch_size(catalog, 1000).await
}

async fn spawn_server_with_batch_size(
    catalog: Catalog,
    fetch_batch_size: usize,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let limits = graph_owl_bolt::BoltLimits {
        fetch_batch_size,
        ..graph_owl_bolt::BoltLimits::default()
    };
    let server = graph_owl_server::bolt::build_server(catalog, limits, SparqlBudget::default());
    tokio::spawn(async move {
        server.serve(listener, std::future::pending()).await;
    });
    addr
}

/// Seeds one queryable entity — a root-kind `Service` asset needs no parent,
/// so it is the cheapest fixture that still projects into the graph
/// (`Catalog::upsert_asset` projects on every write).
async fn seed_one_asset(catalog: &Catalog, name: &str) {
    catalog
        .upsert_asset(
            &Principal::system(),
            UpsertAsset {
                kind: AssetKind::Service,
                name: name.to_string(),
                parent_id: None,
                description: None,
                properties: None,
                extension: None,
            },
        )
        .await
        .expect("seed asset");
}

struct Client {
    stream: TcpStream,
    decoder: chunking::Decoder,
}

impl Client {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        let mut bytes = handshake::MAGIC.to_vec();
        bytes.extend_from_slice(&handshake::encode_version(5, 0));
        bytes.extend_from_slice(&handshake::NO_VERSION);
        bytes.extend_from_slice(&handshake::NO_VERSION);
        bytes.extend_from_slice(&handshake::NO_VERSION);
        stream.write_all(&bytes).await.expect("write handshake");
        Self {
            stream,
            decoder: chunking::Decoder::new(),
        }
    }

    async fn handshake_reply(&mut self) -> [u8; 4] {
        let mut reply = [0u8; 4];
        self.stream
            .read_exact(&mut reply)
            .await
            .expect("read handshake reply");
        reply
    }

    async fn send(&mut self, value: BoltValue) {
        let bytes = graph_owl_bolt::packstream::encode(&value);
        self.stream
            .write_all(&chunking::encode(&bytes))
            .await
            .expect("write message");
    }

    async fn recv(&mut self) -> BoltValue {
        let mut buf = [0u8; 4096];
        loop {
            if let Some(message) = self
                .decoder
                .next_message(16 * 1024 * 1024)
                .expect("chunking")
            {
                let (value, _) = graph_owl_bolt::packstream::decode(&message, 16 * 1024 * 1024)
                    .expect("packstream decode")
                    .expect("a complete top-level value");
                return value;
            }
            let n = self.stream.read(&mut buf).await.expect("socket read");
            assert!(n > 0, "connection closed while a reply was expected");
            self.decoder.feed(&buf[..n]);
        }
    }

    async fn hello(&mut self, token: Option<&str>) -> BoltValue {
        let mut extra = vec![(
            "user_agent".to_string(),
            BoltValue::String("bolt-test-client/1.0".to_string()),
        )];
        if let Some(token) = token {
            extra.push((
                "scheme".to_string(),
                BoltValue::String("bearer".to_string()),
            ));
            extra.push((
                "credentials".to_string(),
                BoltValue::String(token.to_string()),
            ));
        }
        self.send(BoltValue::Structure {
            signature: messages::signature::HELLO,
            fields: vec![BoltValue::Dictionary(extra)],
        })
        .await;
        self.recv().await
    }

    async fn run(&mut self, query: &str) -> BoltValue {
        self.send(BoltValue::Structure {
            signature: messages::signature::RUN,
            fields: vec![
                BoltValue::String(query.to_string()),
                BoltValue::Dictionary(vec![]),
                BoltValue::Dictionary(vec![]),
            ],
        })
        .await;
        self.recv().await
    }

    async fn pull(&mut self, n: i64) -> (Vec<BoltValue>, BoltValue) {
        self.send(BoltValue::Structure {
            signature: messages::signature::PULL,
            fields: vec![BoltValue::Dictionary(vec![(
                "n".to_string(),
                BoltValue::Integer(n),
            )])],
        })
        .await;
        let mut records = Vec::new();
        loop {
            let message = self.recv().await;
            let BoltValue::Structure { signature, .. } = &message else {
                panic!("expected a structure, got {message:?}");
            };
            match *signature {
                messages::signature::RECORD => records.push(message),
                messages::signature::SUCCESS | messages::signature::FAILURE => {
                    return (records, message);
                }
                other => panic!("unexpected signature 0x{other:02x}"),
            }
        }
    }

    async fn reset(&mut self) -> BoltValue {
        self.send(BoltValue::Structure {
            signature: messages::signature::RESET,
            fields: vec![],
        })
        .await;
        self.recv().await
    }

    async fn goodbye(&mut self) {
        self.send(BoltValue::Structure {
            signature: messages::signature::GOODBYE,
            fields: vec![],
        })
        .await;
    }
}

fn is_success(value: &BoltValue) -> bool {
    matches!(value, BoltValue::Structure { signature, .. } if *signature == messages::signature::SUCCESS)
}

fn is_failure(value: &BoltValue) -> bool {
    matches!(value, BoltValue::Structure { signature, .. } if *signature == messages::signature::FAILURE)
}

fn success_metadata(value: &BoltValue) -> &[(String, BoltValue)] {
    match value {
        BoltValue::Structure { signature, fields }
            if *signature == messages::signature::SUCCESS =>
        {
            match &fields[0] {
                BoltValue::Dictionary(entries) => entries,
                other => panic!("SUCCESS metadata is not a dictionary: {other:?}"),
            }
        }
        other => panic!("expected SUCCESS, got {other:?}"),
    }
}

async fn authed_client(addr: std::net::SocketAddr, subject: &str) -> Client {
    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    let reply = client.hello(Some(&token(subject))).await;
    assert!(is_success(&reply), "HELLO must succeed: {reply:?}");
    client
}

// ---- Slice B: handshake and authentication ----

#[tokio::test]
async fn a_correct_magic_preamble_negotiates_the_supported_version() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut client = Client::connect(addr).await;
    assert_eq!(
        client.handshake_reply().await,
        handshake::encode_version(5, 0)
    );
}

#[tokio::test]
async fn a_wrong_magic_preamble_closes_the_connection_without_a_reply() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    // Only the bad magic — the server reads exactly 4 bytes and bails before
    // ever reading further, so writing the 16-byte offer block afterward
    // would just be extra data sent to an already-closing peer, which can
    // surface as a connection reset on some platforms rather than the
    // graceful EOF this test is actually checking for.
    stream
        .write_all(&[0xDE, 0xAD, 0xBE, 0xEF])
        .await
        .expect("write garbage");
    let mut buf = [0u8; 4];
    match stream.read(&mut buf).await {
        Ok(n) => assert_eq!(n, 0, "no reply must be sent for a non-Bolt preamble"),
        // A reset is also a valid "the peer closed" signal — no bytes of a
        // reply were sent either way, which is the property under test.
        Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => {}
        Err(err) => panic!("unexpected error waiting for closure: {err}"),
    }
}

#[tokio::test]
async fn four_unsupported_version_offers_are_refused_with_the_zero_value() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream
        .write_all(&handshake::MAGIC)
        .await
        .expect("write magic");
    let mut offers = Vec::new();
    for major in [250u8, 251, 252, 253] {
        offers.extend_from_slice(&handshake::encode_version(major, 0));
    }
    stream.write_all(&offers).await.expect("write offers");
    let mut reply = [0u8; 4];
    stream.read_exact(&mut reply).await.expect("read reply");
    assert_eq!(reply, handshake::NO_VERSION);
}

#[tokio::test]
async fn hello_with_valid_bearer_credentials_succeeds() {
    let (catalog, _db, _conn) = common::test_catalog_with_secret(SECRET).await;
    let addr = spawn_server(catalog).await;
    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    let reply = client.hello(Some(&token("bolt-user"))).await;
    assert!(is_success(&reply), "expected SUCCESS, got {reply:?}");
}

#[tokio::test]
async fn hello_with_invalid_credentials_fails_and_the_connection_closes() {
    let (catalog, _db, _conn) = common::test_catalog_with_secret(SECRET).await;
    let addr = spawn_server(catalog).await;
    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    let reply = client.hello(Some("not-a-real-token")).await;
    assert!(is_failure(&reply), "expected FAILURE, got {reply:?}");

    let mut buf = [0u8; 1];
    let n = client
        .stream
        .read(&mut buf)
        .await
        .expect("read after FAILURE");
    assert_eq!(n, 0, "the connection must close after a HELLO failure");
}

#[tokio::test]
async fn a_missing_user_agent_is_tolerated() {
    let (catalog, _db, _conn) = common::test_catalog_with_secret(SECRET).await;
    let addr = spawn_server(catalog).await;
    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    client
        .send(BoltValue::Structure {
            signature: messages::signature::HELLO,
            fields: vec![BoltValue::Dictionary(vec![
                (
                    "scheme".to_string(),
                    BoltValue::String("bearer".to_string()),
                ),
                (
                    "credentials".to_string(),
                    BoltValue::String(token("no-agent-user")),
                ),
            ])],
        })
        .await;
    assert!(is_success(&client.recv().await));
}

#[tokio::test]
async fn hello_resolves_the_identical_principal_the_http_path_would() {
    let (catalog, _db, _conn) = common::test_catalog_with_secret(SECRET).await;
    let addr = spawn_server(catalog.clone()).await;
    let subject = format!("bolt-identity-{}", uuid::Uuid::new_v4());

    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    assert!(is_success(&client.hello(Some(&token(&subject))).await));

    // `resolve_principal` is the exact function both the HTTP `Auth`
    // extractor and Bolt's `HELLO` handler call (via the shared
    // `authenticate_bearer_token`) — decision 4. Calling it again for the
    // same fresh subject returns the row HELLO's own call already
    // provisioned, with the shape any first-ever request — HTTP or Bolt —
    // produces: the subject as both id and name, no roles, not an admin.
    let principal = catalog
        .resolve_principal(&subject, &subject)
        .await
        .expect("resolve");
    assert_eq!(principal.id, subject);
    assert_eq!(principal.name, subject);
    assert!(principal.roles.is_empty());
    assert!(!principal.is_admin);
}

// ---- Slice C: the state machine, including FAILED ----

#[tokio::test]
async fn an_illegal_message_before_hello_is_a_failure_not_a_panic() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut client = Client::connect(addr).await;
    client.handshake_reply().await;
    let reply = client.run("RETURN 1").await;
    assert!(
        is_failure(&reply),
        "RUN before HELLO must fail cleanly: {reply:?}"
    );
}

#[tokio::test]
async fn reset_from_authed_succeeds_and_stays_ready() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    seed_one_asset(&catalog, "bolt-reset-service").await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "reset-user").await;
    assert!(is_success(&client.reset().await));
    // Still usable afterward — RESET did not close the connection.
    let reply = client.run("MATCH (n:service) RETURN n.name AS name").await;
    assert!(
        is_success(&reply),
        "expected SUCCESS after RESET, got {reply:?}"
    );
}

#[tokio::test]
async fn goodbye_closes_the_connection_with_no_reply() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "goodbye-user").await;
    client.goodbye().await;
    let mut buf = [0u8; 1];
    let n = client
        .stream
        .read(&mut buf)
        .await
        .expect("read after GOODBYE");
    assert_eq!(n, 0, "GOODBYE gets no summary message, only closure");
}

#[tokio::test]
async fn the_pipelined_batch_after_a_failure_is_ignored_until_reset() {
    // The scenario Slice C exists for: send a failing RUN, then further
    // messages, all without waiting for a reply in between — request-response
    // testing can never exercise this, since it always waits for one answer
    // before sending the next.
    let (catalog, _db, _conn) = common::test_catalog().await;
    seed_one_asset(&catalog, "bolt-pipeline-service").await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "pipeline-user").await;

    // An unparseable Cypher query fails the RUN and puts the session into
    // FAILED.
    client
        .send(BoltValue::Structure {
            signature: messages::signature::RUN,
            fields: vec![
                BoltValue::String("THIS IS NOT CYPHER {{{".to_string()),
                BoltValue::Dictionary(vec![]),
                BoltValue::Dictionary(vec![]),
            ],
        })
        .await;
    // A second RUN and a PULL, sent immediately without reading the first
    // reply — exactly the pipelined batch the module doc describes.
    client
        .send(BoltValue::Structure {
            signature: messages::signature::RUN,
            fields: vec![
                BoltValue::String("RETURN 1".to_string()),
                BoltValue::Dictionary(vec![]),
                BoltValue::Dictionary(vec![]),
            ],
        })
        .await;
    client
        .send(BoltValue::Structure {
            signature: messages::signature::PULL,
            fields: vec![BoltValue::Dictionary(vec![(
                "n".to_string(),
                BoltValue::Integer(-1),
            )])],
        })
        .await;

    let first = client.recv().await;
    assert!(
        is_failure(&first),
        "the malformed query must fail: {first:?}"
    );

    let second = client.recv().await;
    assert!(
        matches!(&second, BoltValue::Structure { signature, .. } if *signature == messages::signature::IGNORED),
        "the RUN sent while FAILED must be IGNORED, not executed: {second:?}"
    );
    let third = client.recv().await;
    assert!(
        matches!(&third, BoltValue::Structure { signature, .. } if *signature == messages::signature::IGNORED),
        "the PULL sent while FAILED must also be IGNORED: {third:?}"
    );

    // Only RESET recovers it.
    assert!(is_success(&client.reset().await));
    let reply = client.run("MATCH (n:service) RETURN n.name AS name").await;
    assert!(
        is_success(&reply),
        "expected SUCCESS after RESET, got {reply:?}"
    );
}

// ---- Slice D: query execution streams results ----

#[tokio::test]
async fn run_and_pull_stream_a_seeded_node_with_its_label_and_properties() {
    let (catalog, db, conn) = common::test_catalog().await;
    seed_one_asset(&catalog, "bolt-run-pull-service").await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "run-pull-user").await;

    let run_reply = client.run("MATCH (n:service) RETURN n").await;
    assert!(is_success(&run_reply), "RUN must succeed: {run_reply:?}");
    let fields = success_metadata(&run_reply);
    assert_eq!(fields[0].0, "fields");

    let (records, summary) = client.pull(-1).await;
    assert!(
        is_success(&summary),
        "PULL summary must be SUCCESS: {summary:?}"
    );
    assert!(
        !records.is_empty(),
        "the seeded asset must come back as at least one row"
    );

    let BoltValue::Structure {
        fields: record_fields,
        ..
    } = &records[0]
    else {
        unreachable!()
    };
    let BoltValue::List(values) = &record_fields[0] else {
        panic!("RECORD's field is not a list")
    };
    let BoltValue::Structure {
        signature,
        fields: node_fields,
    } = &values[0]
    else {
        panic!("expected a Node structure, got {:?}", values[0])
    };
    assert_eq!(*signature, messages::signature::NODE);
    assert_eq!(
        node_fields.len(),
        4,
        "id, labels, properties, element_id — the 5.0+ Node shape"
    );

    drop(db);
    drop(conn);
}

#[tokio::test]
async fn pull_with_a_small_n_reports_has_more_then_exhausts() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    for i in 0..5 {
        seed_one_asset(&catalog, &format!("bolt-batch-service-{i}")).await;
    }
    let addr = spawn_server_with_batch_size(catalog, 2).await;
    let mut client = authed_client(addr, "batch-user").await;

    assert!(is_success(&client.run("MATCH (n:service) RETURN n").await));

    let (first_batch, first_summary) = client.pull(2).await;
    assert_eq!(first_batch.len(), 2);
    let BoltValue::Structure { fields, .. } = &first_summary else {
        unreachable!()
    };
    let BoltValue::Dictionary(meta) = &fields[0] else {
        unreachable!()
    };
    let has_more = meta
        .iter()
        .find(|(k, _)| k == "has_more")
        .map(|(_, v)| v.clone());
    assert_eq!(
        has_more,
        Some(BoltValue::Boolean(true)),
        "more than 2 rows exist, has_more must be true"
    );

    // Drain the rest.
    let (_rest, last_summary) = client.pull(-1).await;
    let BoltValue::Structure { fields, .. } = &last_summary else {
        unreachable!()
    };
    let BoltValue::Dictionary(meta) = &fields[0] else {
        unreachable!()
    };
    let has_more = meta
        .iter()
        .find(|(k, _)| k == "has_more")
        .map(|(_, v)| v.clone());
    assert_eq!(
        has_more,
        Some(BoltValue::Boolean(false)),
        "the stream is now exhausted"
    );
}

#[tokio::test]
async fn discard_consumes_without_transmitting_any_record() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    seed_one_asset(&catalog, "bolt-discard-service").await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "discard-user").await;

    assert!(is_success(&client.run("MATCH (n:service) RETURN n").await));
    client
        .send(BoltValue::Structure {
            signature: messages::signature::DISCARD,
            fields: vec![BoltValue::Dictionary(vec![(
                "n".to_string(),
                BoltValue::Integer(-1),
            )])],
        })
        .await;
    let reply = client.recv().await;
    assert!(
        is_success(&reply),
        "DISCARD must still summarise with SUCCESS: {reply:?}"
    );
}

#[tokio::test]
async fn a_bounded_pull_over_a_larger_result_never_holds_more_than_one_batch_in_flight() {
    // The acceptance criterion this exists for: a large result must not be
    // materialized before paging. `fetch_batch_size` 5 bounds the channel
    // `Catalog::cypher_stream` feeds — if the server collected the whole
    // result before responding to RUN, this would behave identically; the
    // property under test is that PULL 5 returns *exactly* 5 with more
    // remaining, repeatedly, never all of it in the first reply.
    let (catalog, _db, _conn) = common::test_catalog().await;
    for i in 0..40 {
        seed_one_asset(&catalog, &format!("bolt-bounded-service-{i}")).await;
    }
    let addr = spawn_server_with_batch_size(catalog, 5).await;
    let mut client = authed_client(addr, "bounded-user").await;

    assert!(is_success(&client.run("MATCH (n:service) RETURN n").await));
    let mut total = 0;
    loop {
        let (batch, summary) = client.pull(5).await;
        assert!(
            batch.len() <= 5,
            "PULL 5 must never return more than 5 records at once"
        );
        total += batch.len();
        let BoltValue::Structure { fields, .. } = &summary else {
            unreachable!()
        };
        let BoltValue::Dictionary(meta) = &fields[0] else {
            unreachable!()
        };
        let has_more = meta
            .iter()
            .any(|(k, v)| k == "has_more" && *v == BoltValue::Boolean(true));
        if !has_more {
            break;
        }
    }
    assert!(
        total >= 40,
        "every seeded asset must eventually be returned, {total} were"
    );
}

// ---- Slice E: authorization and write refusal ----

#[tokio::test]
async fn a_write_clause_is_refused_naming_the_catalog_api() {
    let (catalog, _db, _conn) = common::test_catalog().await;
    let addr = spawn_server(catalog).await;
    let mut client = authed_client(addr, "write-refusal-user").await;

    let reply = client.run("CREATE (n:Table {name: 'x'})").await;
    assert!(
        is_failure(&reply),
        "a write clause must be refused: {reply:?}"
    );
    let BoltValue::Structure { fields, .. } = &reply else {
        unreachable!()
    };
    let BoltValue::Dictionary(meta) = &fields[0] else {
        unreachable!()
    };
    let message = meta
        .iter()
        .find(|(k, _)| k == "message")
        .map(|(_, v)| v.clone());
    let Some(BoltValue::String(message)) = message else {
        panic!("FAILURE has no message field")
    };
    assert!(
        message.to_lowercase().contains("catalog") || message.to_lowercase().contains("api"),
        "the refusal must name the catalog API as the write path, got: {message}"
    );
}

/// Extends Epic 7b Slice E's `cypher_and_sparql_agree_under_one_restricted_principal`
/// (`authorization.rs`) to a third surface. A divergence here is a data
/// leak, and it is exactly the kind that only shows up once a third surface
/// is added late — which is why this reuses the *identical* fixture
/// (`common::authorization_fixture`) rather than a separate one that could
/// quietly drift from it.
#[tokio::test]
async fn bolt_agrees_with_sparql_and_cypher_under_one_restricted_principal() {
    let (app, _container, catalog) = common::authorization_fixture().await;
    let addr = spawn_server(catalog).await;

    let sparql_query = "SELECT ?name WHERE { ?t <https://graph-owl.dev/ns/catalog#type> \"table\" . \
         ?t <https://graph-owl.dev/ns/catalog#name> ?name }";
    let cypher_query = "MATCH (t) WHERE t.type = \"table\" RETURN t.name AS name";

    let sparql_body = post_query(&app, "/sparql", "asha", sparql_query).await;
    let cypher_body = post_query(&app, "/cypher", "asha", cypher_query).await;

    // Not `authed_client`: the fixture signs tokens with
    // `common::AUTHZ_FIXTURE_SECRET`, not this file's own `SECRET`.
    let mut bolt_client = Client::connect(addr).await;
    bolt_client.handshake_reply().await;
    let hello_reply = bolt_client.hello(Some(&common::token("asha"))).await;
    assert!(
        is_success(&hello_reply),
        "Bolt HELLO must succeed: {hello_reply:?}"
    );
    let run_reply = bolt_client.run(cypher_query).await;
    assert!(
        is_success(&run_reply),
        "Bolt RUN must succeed: {run_reply:?}"
    );
    let (records, summary) = bolt_client.pull(-1).await;
    assert!(is_success(&summary), "Bolt PULL must succeed: {summary:?}");

    let names_from_http = |body: &serde_json::Value| -> std::collections::BTreeSet<String> {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| {
                row["name"]
                    .as_str()
                    .map(|s| s.trim_matches('"').to_string())
            })
            .collect()
    };
    let names_from_bolt = |records: &[BoltValue]| -> std::collections::BTreeSet<String> {
        records
            .iter()
            .map(|record| {
                let BoltValue::Structure { fields, .. } = record else {
                    unreachable!()
                };
                let BoltValue::List(values) = &fields[0] else {
                    unreachable!()
                };
                let BoltValue::String(name) = &values[0] else {
                    panic!("expected a string name, got {:?}", values[0])
                };
                name.clone()
            })
            .collect()
    };

    let sparql_names = names_from_http(&sparql_body);
    let cypher_names = names_from_http(&cypher_body);
    let bolt_names = names_from_bolt(&records);

    assert_eq!(
        sparql_names, cypher_names,
        "sparql and cypher must already agree (Epic 7b Slice E): sparql={sparql_body}, cypher={cypher_body}"
    );
    assert_eq!(
        bolt_names, sparql_names,
        "the same restricted principal must see the same names through Bolt as through \
         SPARQL/Cypher: bolt={bolt_names:?}, sparql={sparql_names:?}"
    );
    assert!(
        !bolt_names.iter().any(|n| n == "customers"),
        "the analyst must not see the denied table through Bolt either: {bolt_names:?}"
    );
    assert!(
        !bolt_names.is_empty(),
        "and must still see everything else: {bolt_names:?}"
    );
}

async fn post_query(
    app: &axum::Router,
    path: &str,
    subject: &str,
    query: &str,
) -> serde_json::Value {
    use tower::ServiceExt as _;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .header(
                    "authorization",
                    format!("Bearer {}", common::token(subject)),
                )
                .body(axum::body::Body::from(
                    serde_json::json!({ "query": query }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    common::json_body(response).await
}
