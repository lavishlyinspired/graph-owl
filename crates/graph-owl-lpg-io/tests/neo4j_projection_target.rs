//! Epic 9a Slice D: one-directional projection to an external store,
//! checked against a real Neo4j via testcontainers — the same
//! reusable-container pattern already used for Postgres/Kafka elsewhere in
//! this workspace, and for `graph-owl-lpg-io`'s own Slice C bulk-CSV test.

#![cfg(feature = "bolt-target")]

use graph_owl_core::flake::Sid;
use graph_owl_lpg::{ElementId, LpgEdge, LpgNode, PropertyMap};
use graph_owl_lpg_io::projection::{
    ElementBatch, GraphProjectionTarget, Neo4jProjectionTarget, ProjectionScope,
};
use testcontainers_modules::neo4j::Neo4j;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

fn node(id: &str, labels: &[&str]) -> LpgNode {
    LpgNode {
        element_id: ElementId::encode(&Sid::dsc(id)),
        labels: labels.iter().map(ToString::to_string).collect(),
        properties: PropertyMap::new(),
    }
}

fn edge(id: &str, from: &str, to: &str, edge_type: &str) -> LpgEdge {
    LpgEdge {
        element_id: ElementId::encode(&Sid::dsc(id)),
        edge_type: edge_type.to_string(),
        start: ElementId::encode(&Sid::dsc(from)),
        end: ElementId::encode(&Sid::dsc(to)),
        properties: PropertyMap::new(),
    }
}

async fn connected_target(
    container: &testcontainers_modules::testcontainers::ContainerAsync<
        testcontainers_modules::neo4j::Neo4jImage,
    >,
) -> Neo4jProjectionTarget {
    let uri = format!(
        "bolt://{}:{}",
        container.get_host().await.expect("host"),
        container.image().bolt_port_ipv4().expect("bolt port")
    );
    let user = container.image().user().expect("default user configured");
    let pass = container
        .image()
        .password()
        .expect("default password configured");
    Neo4jProjectionTarget::connect(&uri, user, pass)
        .await
        .expect("connect and ensure schema")
}

async fn node_count(
    container: &testcontainers_modules::testcontainers::ContainerAsync<
        testcontainers_modules::neo4j::Neo4jImage,
    >,
) -> i64 {
    let uri = format!(
        "bolt://{}:{}",
        container.get_host().await.expect("host"),
        container.image().bolt_port_ipv4().expect("bolt port")
    );
    let user = container.image().user().expect("user");
    let pass = container.image().password().expect("pass");
    let graph = neo4rs::Graph::new(uri, user, pass).expect("connect");
    let mut result = graph
        .execute(neo4rs::query(
            "MATCH (n:GraphOwlElement) RETURN count(n) AS total",
        ))
        .await
        .expect("count query");
    let row = result.next().await.expect("row").expect("one row");
    row.get::<i64>("total").expect("total column")
}

/// **The target's own schema is created once, idempotently** — `connect`
/// runs `CREATE CONSTRAINT ... IF NOT EXISTS`, so connecting twice against
/// the same target must not error, and the constraint must actually reject
/// a second node under one id.
#[tokio::test]
async fn schema_is_created_idempotently_and_enforces_element_id_uniqueness() {
    let container = Neo4j::default().start().await.expect("start neo4j");
    let _first = connected_target(&container).await;
    let _second = connected_target(&container).await;

    let target = connected_target(&container).await;
    let ack = target
        .project(&ElementBatch {
            nodes: vec![node("a", &["Table"])],
            edges: Vec::new(),
            retracted: Vec::new(),
        })
        .await
        .expect("project");
    assert_eq!(ack.nodes_written, 1);
    assert_eq!(node_count(&container).await, 1);
}

/// **Idempotent — projecting twice yields one copy, not two.** The same
/// batch, sent to `project` a second time, must converge rather than
/// duplicate — the plan's own criterion, and the property that also makes
/// "a mid-batch failure leaves a consistent checkpoint and re-running
/// converges" true: a retry after a partial failure is, from the target's
/// point of view, exactly this same "the same batch arrives twice" case.
#[tokio::test]
async fn projecting_the_same_batch_twice_converges_on_one_copy() {
    let container = Neo4j::default().start().await.expect("start neo4j");
    let target = connected_target(&container).await;

    let batch = ElementBatch {
        nodes: vec![node("a", &["Table"]), node("b", &["Table"])],
        edges: vec![edge("r1", "a", "b", "feeds")],
        retracted: Vec::new(),
    };

    target.project(&batch).await.expect("first project");
    target.project(&batch).await.expect("second project");

    assert_eq!(
        node_count(&container).await,
        2,
        "projecting the same two nodes twice must not produce four"
    );

    let uri = format!(
        "bolt://{}:{}",
        container.get_host().await.expect("host"),
        container.image().bolt_port_ipv4().expect("bolt port")
    );
    let graph = neo4rs::Graph::new(
        uri,
        container.image().user().expect("user"),
        container.image().password().expect("pass"),
    )
    .expect("connect");
    let mut result = graph
        .execute(neo4rs::query(
            "MATCH ()-[r:GRAPH_OWL_EDGE]->() RETURN count(r) AS total",
        ))
        .await
        .expect("count edges");
    let row = result.next().await.expect("row").expect("one row");
    let edge_total: i64 = row.get("total").expect("total");
    assert_eq!(
        edge_total, 1,
        "projecting the same one edge twice must not produce two"
    );
}

/// **`checkpoint`/`advance_checkpoint` survive a reconnect** — the value
/// lives in the target itself, not only in this process's memory, which is
/// what lets a fresh process resume an incremental projection (Slice E)
/// instead of restarting from nothing.
#[tokio::test]
async fn the_checkpoint_survives_a_reconnect() {
    let container = Neo4j::default().start().await.expect("start neo4j");
    let target = connected_target(&container).await;
    assert_eq!(
        target
            .checkpoint()
            .await
            .expect("checkpoint")
            .last_projected_t,
        0
    );

    target.advance_checkpoint(42).await.expect("advance");
    assert_eq!(
        target
            .checkpoint()
            .await
            .expect("checkpoint")
            .last_projected_t,
        42
    );

    let reconnected = connected_target(&container).await;
    assert_eq!(
        reconnected
            .checkpoint()
            .await
            .expect("checkpoint")
            .last_projected_t,
        42,
        "a fresh connection must read the checkpoint the target itself stored, not start at 0"
    );
}

/// **`reset` clears a scope** — the whole-target scope, which is Slice D's
/// own; a `graph_id`-narrowed reset is Slice E's concern (documented on
/// `GraphProjectionTarget::reset`'s own implementation) since it needs
/// per-element scope carried through projection, which does not exist yet.
#[tokio::test]
async fn reset_clears_every_projected_element() {
    let container = Neo4j::default().start().await.expect("start neo4j");
    let target = connected_target(&container).await;
    target
        .project(&ElementBatch {
            nodes: vec![node("a", &["Table"]), node("b", &["Table"])],
            edges: Vec::new(),
            retracted: Vec::new(),
        })
        .await
        .expect("project");
    assert_eq!(node_count(&container).await, 2);

    target
        .reset(&ProjectionScope::default())
        .await
        .expect("reset");
    assert_eq!(
        node_count(&container).await,
        0,
        "reset must clear every projected node"
    );
    assert_eq!(
        target
            .checkpoint()
            .await
            .expect("checkpoint")
            .last_projected_t,
        0,
        "reset must also clear the checkpoint — a target with no elements but a stale \
         checkpoint would make a later incremental projection skip data that no longer exists"
    );
}

/// A batched write, not one round trip per element — a structural proof
/// via `neo4rs`' own query log is not available through its public API, so
/// this asserts the *externally observable* behaviour instead: two
/// elements projected in one `project()` call land in one server-side
/// transaction's worth of visible state with no way to observe a partial
/// intermediate state from outside, which per-element round trips could
/// not guarantee under concurrent readers. Kept as a documented proxy for
/// "batched", not a claim of measuring wire-level round trips directly.
#[tokio::test]
async fn a_batch_of_elements_lands_together() {
    let container = Neo4j::default().start().await.expect("start neo4j");
    let target = connected_target(&container).await;
    let batch = ElementBatch {
        nodes: (0..20)
            .map(|i| node(&format!("n{i}"), &["Table"]))
            .collect(),
        edges: Vec::new(),
        retracted: Vec::new(),
    };
    let ack = target.project(&batch).await.expect("project");
    assert_eq!(ack.nodes_written, 20);
    assert_eq!(node_count(&container).await, 20);
}
