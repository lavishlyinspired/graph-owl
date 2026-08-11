//! Epic 7a: the frontier walk.
//!
//! The load-bearing test here is the depth one. A relationship is reified as
//! `entity → relationship → entity`, so a chain of five logical edges is ten
//! stored hops — and reporting ten is the single most likely way to get this
//! wrong, because it is what a naive walk over the flakes produces.

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::{PredicateDef, PredicateRegistry, TripleStore};
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, TraversalEngine, ValidityWindow};

async fn store() -> (PostgresTripleStore, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, database)
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
                valid_at: None,
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
                valid_at: None,
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

/// A reified relationship with no `relType` flake at all still counts as an
/// edge, defaulting to `"related"` — the `COALESCE` in `push_logical_edges`'s
/// aggregate, unexercised by every other test here since they all name a
/// type.
#[tokio::test]
async fn a_relationship_with_no_rel_type_defaults_to_related() {
    let (store, _container) = store().await;
    let subject = node("r1");
    store
        .assert_flakes(&[
            Flake::assert(
                subject.clone(),
                Sid::new(namespace::RDF, "type"),
                FlakeValue::Ref(Sid::dsc("Relationship")),
                1,
            ),
            Flake::assert(
                subject.clone(),
                Sid::dsc("fromEntity"),
                FlakeValue::Ref(node("a")),
                1,
            ),
            Flake::assert(subject, Sid::dsc("toEntity"), FlakeValue::Ref(node("b")), 1),
        ])
        .await
        .expect("write");

    let graph = store
        .subgraph(
            &[node("a")],
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    let edge = graph
        .edges
        .iter()
        .find(|e| e.from.id == "a" && e.to.id == "b")
        .expect("edge present");
    assert_eq!(edge.relationship, "related");
}

/// `derived` reflects the reified relationship's `fromEntity` flake context —
/// Epic 6's reasoning-vs-asserted distinction. No other test here reads
/// `EdgeRef::derived`.
#[tokio::test]
async fn a_reified_relationship_asserted_in_the_reasoning_context_is_marked_derived() {
    let (store, _container) = store().await;
    let subject = node("r1");
    store
        .assert_flakes(&[
            Flake {
                cx: Some(Sid::dsc("graph:reasoning")),
                ..Flake::assert(
                    subject.clone(),
                    Sid::new(namespace::RDF, "type"),
                    FlakeValue::Ref(Sid::dsc("Relationship")),
                    1,
                )
            },
            Flake {
                cx: Some(Sid::dsc("graph:reasoning")),
                ..Flake::assert(
                    subject.clone(),
                    Sid::dsc("fromEntity"),
                    FlakeValue::Ref(node("a")),
                    1,
                )
            },
            Flake {
                cx: Some(Sid::dsc("graph:reasoning")),
                ..Flake::assert(
                    subject.clone(),
                    Sid::dsc("toEntity"),
                    FlakeValue::Ref(node("b")),
                    1,
                )
            },
            Flake {
                cx: Some(Sid::dsc("graph:reasoning")),
                ..Flake::assert(
                    subject,
                    Sid::dsc("relType"),
                    FlakeValue::String("feeds".to_string()),
                    1,
                )
            },
        ])
        .await
        .expect("write");
    store
        .assert_flakes(&edge("r2", "b", "c", "feeds", 1))
        .await
        .expect("write");

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

    let derived_edge = graph
        .edges
        .iter()
        .find(|e| e.from.id == "a" && e.to.id == "b")
        .expect("a->b edge present");
    assert!(
        derived_edge.derived,
        "a reasoning-context edge must be marked derived"
    );

    let asserted_edge = graph
        .edges
        .iter()
        .find(|e| e.from.id == "b" && e.to.id == "c")
        .expect("b->c edge present");
    assert!(
        !asserted_edge.derived,
        "an ordinary edge must not be marked derived"
    );
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

// ---- shortest_path ----

#[tokio::test]
async fn shortest_path_returns_the_route_not_just_its_length() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let path = store
        .shortest_path(
            &node("a"),
            &node("c"),
            Direction::Outgoing,
            Bounds {
                max_hops: 5,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk")
        .expect("c is reachable from a");

    assert_eq!(path.length, 2, "two logical edges");
    let ids: Vec<&str> = path.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"], "the route itself, in order");
}

/// Unreachable is an answer, not an error. It is the commonest true result of
/// asking, and making it exceptional would force every caller to treat the
/// normal case as a failure.
#[tokio::test]
async fn an_unreachable_target_is_none_rather_than_an_error() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&edge("r1", "a", "b", "feeds", 1))
        .await
        .expect("write");

    let path = store
        .shortest_path(
            &node("a"),
            &node("elsewhere"),
            Direction::Outgoing,
            Bounds {
                max_hops: 5,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("not reaching something is not a failure");
    assert!(path.is_none());
}

/// A node reaches itself in zero edges, by definition. Walking for it would
/// make the answer depend on whether the node happens to sit on a cycle.
#[tokio::test]
async fn a_node_reaches_itself_in_zero_edges() {
    let (store, _container) = store().await;
    let path = store
        .shortest_path(
            &node("alone"),
            &node("alone"),
            Direction::Outgoing,
            Bounds::default(),
            &EdgeFilter::default(),
        )
        .await
        .expect("walk")
        .expect("a node always reaches itself");
    assert_eq!(path.length, 0);
    assert_eq!(path.nodes.len(), 1);
}

/// It must be the *shortest*, not merely a route.
#[tokio::test]
async fn shortest_path_prefers_the_shorter_of_two_routes() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "z", "feeds", 1));
    flakes.extend(edge("r3", "a", "x", "feeds", 1));
    flakes.extend(edge("r4", "x", "y", "feeds", 1));
    flakes.extend(edge("r5", "y", "z", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let path = store
        .shortest_path(
            &node("a"),
            &node("z"),
            Direction::Outgoing,
            Bounds {
                max_hops: 6,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk")
        .expect("reachable");
    assert_eq!(path.length, 2, "the two-hop route, not the three-hop one");
}

/// Equal-length alternatives must resolve the same way every call. A tiebreak
/// left to the planner is a result that changes between runs for no reason the
/// caller can see.
#[tokio::test]
async fn equal_length_routes_resolve_deterministically() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "via_m", "feeds", 1));
    flakes.extend(edge("r2", "a", "via_k", "feeds", 1));
    flakes.extend(edge("r3", "via_m", "z", "feeds", 1));
    flakes.extend(edge("r4", "via_k", "z", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let mut seen = Vec::new();
    for _ in 0..4 {
        let path = store
            .shortest_path(
                &node("a"),
                &node("z"),
                Direction::Outgoing,
                Bounds {
                    max_hops: 4,
                    max_nodes: 200,
                },
                &EdgeFilter::default(),
            )
            .await
            .expect("walk")
            .expect("reachable");
        seen.push(path.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>());
    }
    assert!(
        seen.windows(2).all(|w| w[0] == w[1]),
        "the same question gave different answers: {seen:?}"
    );
}

/// **The filter test.** A short route through an excluded edge must not be
/// used; the longer permitted one is returned instead. Applying the filter
/// after selection would hand back a path the caller explicitly ruled out.
#[tokio::test]
async fn a_filtered_out_edge_is_not_used_even_when_it_would_be_shorter() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    // Direct, but of a type the caller will exclude.
    flakes.extend(edge("r1", "a", "z", "sameAs", 1));
    // Longer, but permitted.
    flakes.extend(edge("r2", "a", "b", "feeds", 1));
    flakes.extend(edge("r3", "b", "z", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let path = store
        .shortest_path(
            &node("a"),
            &node("z"),
            Direction::Outgoing,
            Bounds {
                max_hops: 5,
                max_nodes: 200,
            },
            &EdgeFilter {
                relationship_types: Some(vec!["feeds".to_string()]),
                as_of: None,
                valid_at: None,
            },
        )
        .await
        .expect("walk")
        .expect("reachable through the permitted edges");

    assert_eq!(path.length, 2, "the one-hop sameAs route was excluded");
}

// ---- all_paths ----

#[tokio::test]
async fn all_paths_enumerates_every_distinct_route() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "feeds", 1));
    flakes.extend(edge("r3", "b", "z", "feeds", 1));
    flakes.extend(edge("r4", "c", "z", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let set = store
        .all_paths(
            &node("a"),
            &node("z"),
            Direction::Outgoing,
            Bounds {
                max_hops: 5,
                max_nodes: 200,
            },
            100,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(set.paths.len(), 2, "{:?}", set.paths);
    assert!(!set.truncated);
}

/// The cap is a hard stop, not a hint. Path enumeration in a dense graph is
/// exponential, and the alternative to capping is a query that runs until
/// something else times out.
#[tokio::test]
async fn all_paths_caps_hard_and_reports_it() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    // Six parallel routes a -> via_i -> z.
    for i in 0..6 {
        flakes.extend(edge(&format!("r{i}a"), "a", &format!("via{i}"), "feeds", 1));
        flakes.extend(edge(&format!("r{i}b"), &format!("via{i}"), "z", "feeds", 1));
    }
    store.assert_flakes(&flakes).await.expect("write");

    let set = store
        .all_paths(
            &node("a"),
            &node("z"),
            Direction::Outgoing,
            Bounds {
                max_hops: 4,
                max_nodes: 200,
            },
            3,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(set.paths.len(), 3, "the cap is honoured exactly");
    assert!(set.truncated, "and reported");
}

// ---- detect_cycles ----

/// One loop is one cycle. Without normalisation a 3-cycle reports three times,
/// once per starting point, and a reader concludes the graph is three times as
/// tangled as it is.
#[tokio::test]
async fn a_single_loop_is_reported_once() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "c", "a", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let cycles = store
        .detect_cycles(
            &node("a"),
            Bounds {
                max_hops: 6,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    let ids: Vec<&str> = cycles[0].nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"], "normalised so the smallest leads");
}

#[tokio::test]
async fn a_dag_has_no_cycles() {
    let (store, _container) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "a", "c", "feeds", 1));
    store.assert_flakes(&flakes).await.expect("write");

    let cycles = store
        .detect_cycles(
            &node("a"),
            Bounds {
                max_hops: 6,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert!(cycles.is_empty(), "a diamond is not a cycle: {cycles:?}");
}

// ---- namespace-scoped nodes — Epic 105e's own bugfix ----
//
// Every test above uses `node()`, which is `Sid::dsc(id)` — every caller
// this engine ever had (`asset_subgraph`) only ever fed it DSC-scoped
// catalog assets. Epic 105e's `finding_evidence_graph` is the first caller
// to pass a domain pack's own node, and it found the walk silently empty:
// the direct-reference branch only ever recognised `namespace_p = DSC`, and
// every reconstructed `Sid` was hardcoded back to `namespace::DSC`
// regardless of what it actually was.

/// A node in a namespace that is neither DSC nor a shipped one — what a
/// domain pack's own instance data actually looks like.
fn pack_node(id: &str) -> Sid {
    Sid::new(2048, id)
}

/// A namespace outside DSC is only writable once its predicates are
/// declared (`reject_unregistered_predicates`) — unlike `dsc:fromEntity`
/// and friends, which `V3__predicate_registry.sql` seeds so every DSC-only
/// test above never needed this.
async fn register_pack_predicate(store: &PostgresTripleStore, namespace: u16, name: &str) {
    store
        .define(&PredicateDef {
            namespace,
            name: name.to_string(),
            value_type: 0,
            many: false,
            core: false,
        })
        .await
        .expect("declare predicate");
}

#[tokio::test]
async fn a_direct_reference_in_a_domain_pack_s_own_namespace_is_an_edge_too() {
    let (store, _container) = store().await;
    register_pack_predicate(&store, 2048, "issuedBy").await;
    store
        .assert_flakes(&[Flake::assert(
            pack_node("invoice-1"),
            Sid::new(2048, "issuedBy"),
            FlakeValue::Ref(pack_node("supplier-1")),
            1,
        )])
        .await
        .expect("write");

    let result = store
        .neighbours(
            &pack_node("invoice-1"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(
        distance_of(&result, "supplier-1"),
        Some(1),
        "a domain pack's own Ref-valued predicate must be walkable, not just \
         dsc:parentTable/dsc:owner and friends: {result:?}"
    );
}

/// The reconstructed node must carry the namespace it actually came from,
/// not a namespace hardcoded to whatever the *first* caller happened to use.
#[tokio::test]
async fn a_reconstructed_node_carries_its_own_namespace_not_a_hardcoded_one() {
    let (store, _container) = store().await;
    register_pack_predicate(&store, 2048, "issuedBy").await;
    store
        .assert_flakes(&[Flake::assert(
            pack_node("invoice-1"),
            Sid::new(2048, "issuedBy"),
            FlakeValue::Ref(pack_node("supplier-1")),
            1,
        )])
        .await
        .expect("write");

    let graph = store
        .subgraph(
            &[pack_node("invoice-1")],
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    let supplier = graph
        .nodes
        .iter()
        .find(|n| n.id == "supplier-1")
        .expect("supplier reachable");
    assert_eq!(
        supplier.namespace_code, 2048,
        "the node must keep the namespace it was written in: {graph:?}"
    );
    let seed = graph
        .nodes
        .iter()
        .find(|n| n.id == "invoice-1")
        .expect("seed present");
    assert_eq!(seed.namespace_code, 2048, "the seed itself too: {graph:?}");
}

/// The same local id in two different namespaces must stay two different
/// nodes. Before this fix the recursive CTE carried only the bare local
/// name as node identity, so this was a silent merge, not a walkable
/// distinction.
#[tokio::test]
async fn two_namespaces_sharing_a_local_id_stay_distinct_nodes() {
    let (store, _container) = store().await;
    register_pack_predicate(&store, 2048, "issuedBy").await;
    register_pack_predicate(&store, 3072, "issuedBy").await;
    store
        .assert_flakes(&[
            Flake::assert(
                pack_node("shared"),
                Sid::new(2048, "issuedBy"),
                FlakeValue::Ref(pack_node("only-in-2048")),
                1,
            ),
            Flake::assert(
                Sid::new(3072, "shared"),
                Sid::new(3072, "issuedBy"),
                FlakeValue::Ref(Sid::new(3072, "only-in-3072")),
                1,
            ),
        ])
        .await
        .expect("write");

    let result = store
        .neighbours(
            &pack_node("shared"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(
        distance_of(&result, "only-in-2048"),
        Some(1),
        "got {result:?}"
    );
    assert!(
        !result.reached.iter().any(|r| r.node.id == "only-in-3072"),
        "a different namespace's node with the same local id must not leak \
         into this walk: {result:?}"
    );
}

/// `rdf:type` and friends stay excluded from the walk even though they are
/// `value_type = 0` (`Ref`) — a data graph should not flood into class nodes
/// just because direct references are no longer restricted to DSC. The old
/// `namespace_p = DSC` filter kept this out by accident (`rdf:type`'s own
/// namespace is RDF, not DSC); the fix keeps it out on purpose.
#[tokio::test]
async fn rdf_type_is_still_not_walked_as_an_edge() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&[Flake::assert(
            pack_node("invoice-1"),
            Sid::new(namespace::RDF, "type"),
            FlakeValue::Ref(Sid::dsc("PurchaseInvoice")),
            1,
        )])
        .await
        .expect("write");

    let result = store
        .neighbours(
            &pack_node("invoice-1"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(
        result.reached.len(),
        1,
        "only the seed itself — rdf:type must not become a traversal edge: {result:?}"
    );
}

/// A table that feeds itself through a recursive view. Length 1, and real.
#[tokio::test]
async fn a_self_loop_is_a_cycle_of_length_one() {
    let (store, _container) = store().await;
    store
        .assert_flakes(&edge("r1", "a", "a", "feeds", 1))
        .await
        .expect("write");

    let cycles = store
        .detect_cycles(
            &node("a"),
            Bounds {
                max_hops: 4,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycles[0].nodes.len(), 1);
}

/// Date-window traversal — Epic 105 P8 (`plans/105u-date-window-traversal.md`).
/// Identical fixtures and assertions to
/// `graph-owl-traversal-memory/tests/traversal.rs`'s own date-window suite,
/// matching this file's own header comment: two implementations of one
/// trait, differentially tested against the same input.
fn dated(id: &str, from: &str, to: Option<&str>, t: i64) -> Vec<Flake> {
    let subject = node(id);
    let mut flakes = vec![Flake::assert(
        subject.clone(),
        Sid::dsc("effectiveFrom"),
        FlakeValue::String(from.to_string()),
        t,
    )];
    if let Some(to) = to {
        flakes.push(Flake::assert(
            subject,
            Sid::dsc("effectiveTo"),
            FlakeValue::String(to.to_string()),
            t,
        ));
    }
    flakes
}

/// `effectiveFrom`/`effectiveTo` are not among `V3__predicate_registry.sql`'s
/// shipped DSC predicates (unlike `fromEntity`/`toEntity`/`relType`, seeded
/// there), so — unlike every other DSC predicate this file uses — writing
/// them needs an explicit `define` first, matching `register_pack_predicate`'s
/// own precedent above for a namespace outside DSC's seeded set.
async fn register_dated_predicates(store: &PostgresTripleStore) {
    for name in ["effectiveFrom", "effectiveTo"] {
        store
            .define(&PredicateDef {
                namespace: namespace::DSC,
                name: name.to_string(),
                value_type: 1, // string
                many: false,
                core: false,
            })
            .await
            .expect("declare predicate");
    }
}

fn valid_at(date: &str) -> EdgeFilter {
    EdgeFilter {
        valid_at: Some(ValidityWindow {
            effective_from: Sid::dsc("effectiveFrom"),
            effective_to: Sid::dsc("effectiveTo"),
            at: date.parse().expect("test date is ISO-8601"),
        }),
        ..EdgeFilter::default()
    }
}

#[tokio::test]
async fn a_node_not_yet_valid_is_excluded_from_the_walk() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(dated("t1", "2030-01-01", None, 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), None, "{result:?}");
}

#[tokio::test]
async fn a_node_no_longer_valid_is_excluded_from_the_walk() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(dated("t1", "2020-01-01", Some("2021-01-01"), 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), None, "{result:?}");
}

#[tokio::test]
async fn a_node_inside_its_own_window_is_reached() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(dated("t1", "2020-01-01", Some("2030-01-01"), 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), Some(1), "{result:?}");
}

#[tokio::test]
async fn the_window_boundaries_are_from_inclusive_to_exclusive() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(edge("rel-1", "t0", "t2", "feeds", 1));
    flakes.extend(dated("t1", "2026-01-01", Some("2027-01-01"), 1));
    flakes.extend(dated("t2", "2020-01-01", Some("2026-01-01"), 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(
        distance_of(&result, "t1"),
        Some(1),
        "effective_from is inclusive: {result:?}"
    );
    assert_eq!(
        distance_of(&result, "t2"),
        None,
        "effective_to is exclusive: {result:?}"
    );
}

#[tokio::test]
async fn an_open_ended_window_is_valid_arbitrarily_far_in_the_future() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(dated("t1", "2020-01-01", None, 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2099-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), Some(1), "{result:?}");
}

#[tokio::test]
async fn a_node_with_no_validity_properties_is_reached_regardless_of_the_filter() {
    let (store, _db) = store().await;
    let flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), Some(1), "{result:?}");
}

#[tokio::test]
async fn an_excluded_node_cannot_be_a_pass_through_to_a_further_node() {
    let (store, _db) = store().await;
    register_dated_predicates(&store).await;
    let mut flakes = edge("rel-0", "t0", "t1", "feeds", 1);
    flakes.extend(edge("rel-1", "t1", "t2", "feeds", 1));
    flakes.extend(dated("t1", "2030-01-01", None, 1));
    store.assert_flakes(&flakes).await.expect("write");

    let result = store
        .neighbours(
            &node("t0"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &valid_at("2026-01-01"),
        )
        .await
        .expect("walk");

    assert_eq!(distance_of(&result, "t1"), None, "{result:?}");
    assert_eq!(
        distance_of(&result, "t2"),
        None,
        "t2 is only reachable through the excluded t1: {result:?}"
    );
}
