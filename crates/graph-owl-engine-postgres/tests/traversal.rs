//! Epic 7a: the frontier walk.
//!
//! The load-bearing test here is the depth one. A relationship is reified as
//! `entity → relationship → entity`, so a chain of five logical edges is ten
//! stored hops — and reporting ten is the single most likely way to get this
//! wrong, because it is what a naive walk over the flakes produces.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::TripleStore;
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, TraversalEngine};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

async fn store() -> (PostgresTripleStore, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .start()
        .await
        .expect("postgres should start");
    let connection_string = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        container.get_host().await.expect("host"),
        container.get_host_port_ipv4(5432).await.expect("port")
    );
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, container)
}

fn node(id: &str) -> Sid {
    Sid::dsc(id)
}

/// One reified relationship: its own node, both endpoints as references.
fn edge(id: &str, from: &str, to: &str, kind: &str, t: i64) -> Vec<Flake> {
    let subject = node(id);
    vec![
        Flake::assert(
            subject.clone(),
            Sid::new(namespace::RDF, "type"),
            FlakeValue::Ref(Sid::dsc("Relationship")),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("fromEntity"),
            FlakeValue::Ref(node(from)),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("toEntity"),
            FlakeValue::Ref(node(to)),
            t,
        ),
        Flake::assert(
            subject,
            Sid::dsc("relType"),
            FlakeValue::String(kind.to_string()),
            t,
        ),
    ]
}

fn distance_of(result: &graph_owl_traversal::TraversalResult, id: &str) -> Option<usize> {
    result
        .reached
        .iter()
        .find(|r| r.node.id == id)
        .map(|r| r.distance)
}

/// **Decision 2.** Five logical edges is distance five. A walk that counted
/// stored hops would report ten, and every distance the product shows would be
/// double.
#[tokio::test]
async fn a_chain_of_five_logical_edges_reports_distance_five() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    for i in 0..5 {
        flakes.extend(edge(
            &format!("rel-{i}"),
            &format!("t{i}"),
            &format!("t{}", i + 1),
            "feeds",
            1,
        ));
    }
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 10,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t5"), Some(5), "got {result:?}");
    assert_eq!(distance_of(&result, "t1"), Some(1));
    assert_eq!(
        distance_of(&result, "t0"),
        Some(0),
        "the seed is distance 0"
    );
    assert!(
        !result.reached.iter().any(|r| r.node.id.starts_with("rel-")),
        "the relationship nodes are plumbing and must not appear as neighbours"
    );
}

/// `max_hops` bounds exactly. Off by one in either direction is a different
/// answer, not a rounding difference.
#[tokio::test]
async fn max_hops_bounds_exactly() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    for i in 0..5 {
        flakes.extend(edge(
            &format!("rel-{i}"),
            &format!("t{i}"),
            &format!("t{}", i + 1),
            "feeds",
            1,
        ));
    }
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t2"), Some(2), "two hops is included");
    assert_eq!(distance_of(&result, "t3"), None, "three hops is not");
}

/// A diamond must report its far node once, at the shorter distance. Without
/// `DISTINCT ON` it appears once per route.
#[tokio::test]
async fn a_diamond_reports_its_far_node_once_at_the_shorter_distance() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "feeds", 1));
    flakes.extend(edge("r3", "b", "d", "feeds", 1));
    flakes.extend(edge("r4", "c", "d", "feeds", 1));
    // And a long way round, so "shortest" is a real choice.
    flakes.extend(edge("r5", "a", "x", "feeds", 1));
    flakes.extend(edge("r6", "x", "y", "feeds", 1));
    flakes.extend(edge("r7", "y", "d", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("a"),
            Direction::Outgoing,
            Bounds {
                max_hops: 5,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    let appearances = result.reached.iter().filter(|r| r.node.id == "d").count();
    assert_eq!(appearances, 1, "d appeared {appearances} times");
    assert_eq!(distance_of(&result, "d"), Some(2), "the shorter route wins");
}

/// A cycle must terminate. On a metadata graph this is not hypothetical, and
/// the failure mode without a guard is a hang rather than a wrong answer.
#[tokio::test]
async fn a_cycle_terminates() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "c", "a", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        store.neighbours(
            &node("a"),
            Direction::Outgoing,
            Bounds {
                max_hops: 20,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        ),
    )
    .await
    .expect("the walk must terminate, not hang")
    .expect("walk");

    assert_eq!(result.reached.len(), 3, "a, b and c, each once");
}

/// Direction is the difference between "what does this feed" and "what feeds
/// this", and the two are served by different indexes.
#[tokio::test]
async fn direction_selects_which_way_the_walk_goes() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&edge("r1", "upstream", "middle", "feeds", 1))
        .await
        .expect("write");
    store
        .assert_flakes(&edge("r2", "middle", "downstream", "feeds", 1))
        .await
        .expect("write");

    let bounds = Bounds {
        max_hops: 3,
        max_nodes: 200,
    };

    let out = store
        .neighbours(
            &node("middle"),
            Direction::Outgoing,
            bounds,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(distance_of(&out, "downstream"), Some(1));
    assert_eq!(distance_of(&out, "upstream"), None);

    let incoming = store
        .neighbours(
            &node("middle"),
            Direction::Incoming,
            bounds,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(distance_of(&incoming, "upstream"), Some(1));
    assert_eq!(distance_of(&incoming, "downstream"), None);

    let both = store
        .neighbours(
            &node("middle"),
            Direction::Both,
            bounds,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(distance_of(&both, "upstream"), Some(1));
    assert_eq!(distance_of(&both, "downstream"), Some(1));
    // Each neighbour once. A `Both` that unioned without dedup would double
    // every node reachable by two routes.
    assert_eq!(both.reached.len(), 3, "got {both:?}");
}

/// The hierarchy is graph structure. A catalogue with no lineage declared yet
/// would otherwise traverse to nothing at all — which is the state every new
/// deployment starts in.
#[tokio::test]
async fn direct_references_such_as_the_hierarchy_are_edges_too() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&[
            Flake::assert(
                node("column-1"),
                Sid::dsc("parentTable"),
                FlakeValue::Ref(node("table-1")),
                1,
            ),
            Flake::assert(
                node("table-1"),
                Sid::dsc("parentSchema"),
                FlakeValue::Ref(node("schema-1")),
                1,
            ),
        ])
        .await
        .expect("write");

    let result = store
        .neighbours(
            &node("column-1"),
            Direction::Outgoing,
            Bounds {
                max_hops: 3,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "table-1"), Some(1));
    assert_eq!(distance_of(&result, "schema-1"), Some(2));
}

/// A filtered-out edge must not be walked. Filtering *after* the walk would
/// return nodes reached through edges the caller explicitly excluded.
#[tokio::test]
async fn an_excluded_relationship_type_is_not_traversed() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "sameAs", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("a"),
            Direction::Outgoing,
            Bounds {
                max_hops: 3,
                max_nodes: 200,
            },
            &EdgeFilter {
                relationship_types: Some(vec!["feeds".to_string()]),
                as_of: None,
            },
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "b"), Some(1));
    assert_eq!(distance_of(&result, "c"), None, "sameAs was excluded");
}

/// Time-travelling traversal falls out of Epic 4 rather than needing its own
/// mechanism: an edge asserted later must be invisible to a walk asked about
/// an earlier transaction.
#[tokio::test]
async fn as_of_walks_the_graph_as_it_was() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&edge("r1", "a", "b", "feeds", 1))
        .await
        .expect("write");
    store
        .assert_flakes(&edge("r2", "b", "c", "feeds", 5))
        .await
        .expect("write");

    let bounds = Bounds {
        max_hops: 3,
        max_nodes: 200,
    };

    let now = store
        .neighbours(
            &node("a"),
            Direction::Outgoing,
            bounds,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(distance_of(&now, "c"), Some(2), "both edges exist now");

    let before = store
        .neighbours(
            &node("a"),
            Direction::Outgoing,
            bounds,
            &EdgeFilter {
                relationship_types: None,
                as_of: Some(3),
            },
        )
        .await
        .expect("walk");
    assert_eq!(distance_of(&before, "b"), Some(1));
    assert_eq!(
        distance_of(&before, "c"),
        None,
        "the second edge had not been asserted at t=3"
    );
}

/// A retracted edge must stop being walkable — otherwise deleting a
/// relationship leaves it in every traversal.
#[tokio::test]
async fn a_retracted_edge_is_not_traversed() {
    let (store, _container) = store().await;
    let flakes = edge("r1", "a", "b", "feeds", 1);
    store.assert_flakes(&flakes).await.expect("write");
    store
        .retract_flakes(&edge("r1", "a", "b", "feeds", 2))
        .await
        .expect("retract");

    let result = store
        .neighbours(
            &node("a"),
            Direction::Outgoing,
            Bounds {
                max_hops: 3,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "b"), None, "got {result:?}");
}

#[tokio::test]
async fn an_isolated_node_returns_itself_and_no_error() {
    let (store, _container) = store().await;
    let result = store
        .neighbours(
            &node("alone"),
            Direction::Both,
            Bounds::default(),
            &EdgeFilter::default(),
        )
        .await
        .expect("an isolated node is not an error");

    assert_eq!(result.reached.len(), 1);
    assert_eq!(result.reached[0].distance, 0);
    assert!(!result.truncated);
}

/// The node budget must report itself. A silently truncated graph is a picture
/// that claims to be the whole story.
#[tokio::test]
async fn exceeding_the_node_budget_reports_truncation() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    for i in 0..20 {
        flakes.extend(edge(
            &format!("r{i}"),
            "hub",
            &format!("leaf{i}"),
            "feeds",
            1,
        ));
    }
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("hub"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 5,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(result.reached.len(), 5, "the budget is honoured exactly");
    assert!(result.truncated);
    assert_eq!(
        result.truncation_reason,
        Some(graph_owl_traversal::TruncationReason::NodeBudget)
    );
}

// ---- subgraph ----

#[tokio::test]
async fn a_subgraph_returns_nodes_and_the_edges_between_them() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let graph = store
        .subgraph(
            &[node("a")],
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    assert_eq!(graph.nodes.len(), 3, "a, b, c: {:?}", graph.nodes);
    assert_eq!(graph.edges.len(), 2, "{:?}", graph.edges);
    assert!(graph.edges.iter().all(|e| e.relationship == "feeds"));
}

/// Every edge's endpoints must be in the node set. A dangling edge is drawn
/// into empty space by a renderer, which is worse than a smaller picture.
#[tokio::test]
async fn a_truncated_subgraph_has_no_dangling_edges() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    for i in 0..30 {
        flakes.extend(edge(
            &format!("r{i}"),
            "hub",
            &format!("leaf{i}"),
            "feeds",
            1,
        ));
    }
    store.assert_flakes(&flakes).await.expect("write");

    let graph = store
        .subgraph(
            &[node("hub")],
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 6,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    assert!(graph.truncated);
    for edge in &graph.edges {
        assert!(
            graph.nodes.contains(&edge.from) && graph.nodes.contains(&edge.to),
            "{edge:?} dangles outside the node set"
        );
    }
}

/// Overlapping seeds produce one merged picture. The same node twice is a
/// rendering bug, not extra information.
#[tokio::test]
async fn overlapping_seeds_merge_into_one_subgraph() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "shared", "feeds", 1));
    flakes.extend(edge("r2", "b", "shared", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let graph = store
        .subgraph(
            &[node("a"), node("b")],
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    assert_eq!(
        graph.nodes.iter().filter(|n| n.id == "shared").count(),
        1,
        "the shared node must appear once: {:?}",
        graph.nodes
    );
}

#[tokio::test]
async fn an_empty_seed_set_returns_an_empty_subgraph() {
    let (store, _container) = store().await;
    let graph = store
        .subgraph(
            &[],
            Direction::Both,
            Bounds::default(),
            &EdgeFilter::default(),
        )
        .await
        .expect("empty seeds are not an error");

    assert!(graph.nodes.is_empty() && graph.edges.is_empty() && !graph.truncated);
}
