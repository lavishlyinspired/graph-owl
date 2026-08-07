//! Epic 103: the same 26 tests `graph-owl-engine-postgres::tests::traversal`
//! runs against the CTE adapter, run here against [`InMemoryTraversalEngine`]
//! instead. Only `store()` differs — an in-memory `TripleStore` instead of a
//! Postgres container — every fixture and assertion is unchanged, because two
//! implementations of one trait that are not differentially tested are two
//! behaviours, not one.

mod common;

use common::InMemoryTripleStore;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, TraversalEngine};
use graph_owl_traversal_memory::InMemoryTraversalEngine;
use std::sync::Arc;

// `async` for parity with the Postgres adapter's own `store()`, which
// genuinely awaits a connection — every call site in this file is written
// against that shape so the two differential suites stay line-for-line
// comparable.
#[allow(clippy::unused_async)]
async fn store() -> (InMemoryTraversalEngine, std::sync::Arc<InMemoryTripleStore>) {
    let triples = Arc::new(InMemoryTripleStore::new());
    (InMemoryTraversalEngine::new(triples.clone()), triples)
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
    let (store, triples) = store().await;
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
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
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
    triples.seed(&flakes).await;

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
/// dedup it appears once per route.
#[tokio::test]
async fn a_diamond_reports_its_far_node_once_at_the_shorter_distance() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "feeds", 1));
    flakes.extend(edge("r3", "b", "d", "feeds", 1));
    flakes.extend(edge("r4", "c", "d", "feeds", 1));
    // And a long way round, so "shortest" is a real choice.
    flakes.extend(edge("r5", "a", "x", "feeds", 1));
    flakes.extend(edge("r6", "x", "y", "feeds", 1));
    flakes.extend(edge("r7", "y", "d", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "c", "a", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    triples
        .seed(&edge("r1", "upstream", "middle", "feeds", 1))
        .await;
    triples
        .seed(&edge("r2", "middle", "downstream", "feeds", 1))
        .await;

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
    let (store, triples) = store().await;
    triples
        .seed(&[
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
        .await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "sameAs", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    triples.seed(&edge("r1", "a", "b", "feeds", 1)).await;
    triples.seed(&edge("r2", "b", "c", "feeds", 5)).await;

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
    let (store, triples) = store().await;
    let flakes = edge("r1", "a", "b", "feeds", 1);
    triples.seed(&flakes).await;
    triples.retract(&edge("r1", "a", "b", "feeds", 2)).await;

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
    let (store, _triples) = store().await;
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
    let (store, triples) = store().await;
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
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
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
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "shared", "feeds", 1));
    flakes.extend(edge("r2", "b", "shared", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, _triples) = store().await;
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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    triples.seed(&edge("r1", "a", "b", "feeds", 1)).await;

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
    let (store, _triples) = store().await;
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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "z", "feeds", 1));
    flakes.extend(edge("r3", "a", "x", "feeds", 1));
    flakes.extend(edge("r4", "x", "y", "feeds", 1));
    flakes.extend(edge("r5", "y", "z", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "via_m", "feeds", 1));
    flakes.extend(edge("r2", "a", "via_k", "feeds", 1));
    flakes.extend(edge("r3", "via_m", "z", "feeds", 1));
    flakes.extend(edge("r4", "via_k", "z", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    // Direct, but of a type the caller will exclude.
    flakes.extend(edge("r1", "a", "z", "sameAs", 1));
    // Longer, but permitted.
    flakes.extend(edge("r2", "a", "b", "feeds", 1));
    flakes.extend(edge("r3", "b", "z", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "feeds", 1));
    flakes.extend(edge("r3", "b", "z", "feeds", 1));
    flakes.extend(edge("r4", "c", "z", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    // Six parallel routes a -> via_i -> z.
    for i in 0..6 {
        flakes.extend(edge(&format!("r{i}a"), "a", &format!("via{i}"), "feeds", 1));
        flakes.extend(edge(&format!("r{i}b"), &format!("via{i}"), "z", "feeds", 1));
    }
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "c", "a", "feeds", 1));
    triples.seed(&flakes).await;

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
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "a", "c", "feeds", 1));
    triples.seed(&flakes).await;

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

/// A table that feeds itself through a recursive view. Length 1, and real.
#[tokio::test]
async fn a_self_loop_is_a_cycle_of_length_one() {
    let (store, triples) = store().await;
    triples.seed(&edge("r1", "a", "a", "feeds", 1)).await;

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

// ---- additional coverage, beyond the 26 ported from the Postgres suite ----
//
// The tests above prove the two adapters agree; these close gaps mutation
// testing found that the ported suite happens not to exercise — mostly exact
// boundary values (`>` vs `>=`/`==` is invisible unless something lands
// exactly on the line) and the `derived` flag, which no ported test reads.

/// A reified relationship's `fromEntity` flake carries the context that
/// decides `derived` (Epic 6's reasoning-vs-asserted distinction). No ported
/// test reads `EdgeRef::derived` at all, so a `==`/`!=` swap here survived
/// mutation testing silently.
#[tokio::test]
async fn a_reified_relationship_asserted_in_the_reasoning_context_is_marked_derived() {
    let (store, triples) = store().await;
    let subject = node("r1");
    triples
        .seed(&[
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
        .await;
    triples.seed(&edge("r2", "b", "c", "feeds", 1)).await;

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

/// The same `derived` flag, for the direct-reference edge source (the
/// hierarchy edges, not a reified relationship) — a separate code path with
/// its own `==`/`!=` check.
#[tokio::test]
async fn a_direct_reference_asserted_in_the_reasoning_context_is_marked_derived() {
    let (store, triples) = store().await;
    triples
        .seed(&[Flake {
            cx: Some(Sid::dsc("graph:reasoning")),
            ..Flake::assert(
                node("column-1"),
                Sid::dsc("parentTable"),
                FlakeValue::Ref(node("table-1")),
                1,
            )
        }])
        .await;

    let graph = store
        .subgraph(
            &[node("column-1")],
            Direction::Outgoing,
            Bounds {
                max_hops: 1,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    let derived_edge = graph
        .edges
        .iter()
        .find(|e| e.from.id == "column-1" && e.to.id == "table-1")
        .expect("edge present");
    assert!(derived_edge.derived);
}

/// `>` vs `>=` at the node budget is invisible unless something lands
/// exactly on it — one node short of the budget always exercises the
/// `false` branch either way, and one node over always exercises `true`.
#[tokio::test]
async fn landing_exactly_on_the_node_budget_is_not_truncated() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    for i in 0..4 {
        flakes.extend(edge(
            &format!("r{i}"),
            "hub",
            &format!("leaf{i}"),
            "feeds",
            1,
        ));
    }
    triples.seed(&flakes).await;

    // hub + 4 leaves = 5 nodes, exactly the budget.
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

    assert_eq!(result.reached.len(), 5);
    assert!(
        !result.truncated,
        "landing exactly on the budget is not truncation"
    );
}

/// The same boundary, for `subgraph`'s own, separately-computed budget check.
#[tokio::test]
async fn a_subgraph_landing_exactly_on_the_node_budget_is_not_truncated() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    for i in 0..4 {
        flakes.extend(edge(
            &format!("r{i}"),
            "hub",
            &format!("leaf{i}"),
            "feeds",
            1,
        ));
    }
    triples.seed(&flakes).await;

    let graph = store
        .subgraph(
            &[node("hub")],
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 5,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    assert_eq!(graph.nodes.len(), 5);
    assert!(!graph.truncated);
}

/// `subgraph` has no ported test proving `max_hops` actually excludes
/// anything — only `neighbours` does. A node one hop beyond the bound must
/// not appear.
#[tokio::test]
async fn a_subgraph_bounds_max_hops_exactly() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "hub", "n1", "feeds", 1));
    flakes.extend(edge("r2", "n1", "n2", "feeds", 1));
    flakes.extend(edge("r3", "n2", "n3", "feeds", 1));
    triples.seed(&flakes).await;

    let graph = store
        .subgraph(
            &[node("hub")],
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("subgraph");

    assert!(
        graph.nodes.iter().any(|n| n.id == "n2"),
        "two hops is included: {:?}",
        graph.nodes
    );
    assert!(
        !graph.nodes.iter().any(|n| n.id == "n3"),
        "three hops is not: {:?}",
        graph.nodes
    );
}

/// The same budget-boundary class, for `all_paths`'s own cap check.
#[tokio::test]
async fn all_paths_landing_exactly_on_the_cap_is_not_truncated() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    for i in 0..3 {
        flakes.extend(edge(&format!("r{i}a"), "a", &format!("via{i}"), "feeds", 1));
        flakes.extend(edge(&format!("r{i}b"), &format!("via{i}"), "z", "feeds", 1));
    }
    triples.seed(&flakes).await;

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

    assert_eq!(set.paths.len(), 3, "exactly three routes, exactly the cap");
    assert!(
        !set.truncated,
        "landing exactly on the cap is not truncation"
    );
}

/// A node reaches itself in zero hops for `all_paths` too, matching the
/// Postgres frontier's own depth-0 base-case row (`shortest_path` already
/// has this test; `all_paths` did not).
#[tokio::test]
async fn all_paths_from_a_node_to_itself_is_the_trivial_zero_length_path() {
    let (store, _triples) = store().await;

    let set = store
        .all_paths(
            &node("alone"),
            &node("alone"),
            Direction::Outgoing,
            Bounds::default(),
            10,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");

    assert_eq!(set.paths.len(), 1, "{:?}", set.paths);
    assert_eq!(set.paths[0].length, 0);
    assert_eq!(set.paths[0].nodes, vec![node("alone")]);
    assert!(!set.truncated);
}

/// The ported `all_paths_enumerates_every_distinct_route` only checks the
/// *count* of routes found, which happens to stay right even under a
/// `current == target` → `!=` mutation (every intermediate node gets
/// mis-recorded as a one-hop "route" instead, and a diamond has exactly two
/// of those too). Checking the actual node sequence and length closes that.
#[tokio::test]
async fn all_paths_reports_the_route_nodes_not_just_a_count() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "a", "c", "feeds", 1));
    flakes.extend(edge("r3", "b", "z", "feeds", 1));
    flakes.extend(edge("r4", "c", "z", "feeds", 1));
    triples.seed(&flakes).await;

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

    let routes: Vec<Vec<&str>> = set
        .paths
        .iter()
        .map(|p| p.nodes.iter().map(|n| n.id.as_str()).collect())
        .collect();
    assert!(routes.contains(&vec!["a", "b", "z"]), "{routes:?}");
    assert!(routes.contains(&vec!["a", "c", "z"]), "{routes:?}");
    assert!(set.paths.iter().all(|p| p.length == 2), "{:?}", set.paths);
}

/// `all_paths` has no ported `max_hops`-boundary test (unlike `neighbours`'
/// `max_hops_bounds_exactly`); a three-hop chain must be found exactly at a
/// bound of three and not at all at a bound of two.
#[tokio::test]
async fn all_paths_bounds_max_hops_exactly() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    for i in 0..3 {
        flakes.extend(edge(
            &format!("r{i}"),
            &format!("t{i}"),
            &format!("t{}", i + 1),
            "feeds",
            1,
        ));
    }
    triples.seed(&flakes).await;

    let within = store
        .all_paths(
            &node("t0"),
            &node("t3"),
            Direction::Outgoing,
            Bounds {
                max_hops: 3,
                max_nodes: 200,
            },
            10,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(
        within.paths.len(),
        1,
        "three hops is within bounds: {:?}",
        within.paths
    );

    let beyond = store
        .all_paths(
            &node("t0"),
            &node("t3"),
            Direction::Outgoing,
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            10,
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert!(
        beyond.paths.is_empty(),
        "three hops exceeds a bound of two: {:?}",
        beyond.paths
    );
}

/// The same boundary class for `detect_cycles`: the closing edge itself
/// needs one more hop than the cycle's other edges, so a bound one short
/// must find nothing while the exact bound finds the cycle.
#[tokio::test]
async fn detect_cycles_bounds_max_hops_exactly() {
    let (store, triples) = store().await;
    let mut flakes = Vec::new();
    flakes.extend(edge("r1", "a", "b", "feeds", 1));
    flakes.extend(edge("r2", "b", "c", "feeds", 1));
    flakes.extend(edge("r3", "c", "a", "feeds", 1));
    triples.seed(&flakes).await;

    let within = store
        .detect_cycles(
            &node("a"),
            Bounds {
                max_hops: 3,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert_eq!(within.len(), 1, "{within:?}");

    let beyond = store
        .detect_cycles(
            &node("a"),
            Bounds {
                max_hops: 2,
                max_nodes: 200,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    assert!(
        beyond.is_empty(),
        "the closing edge needs a third hop: {beyond:?}"
    );
}
