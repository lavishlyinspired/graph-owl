//! Epic 103's own acceptance criterion: **measured**, not assumed, against
//! the Postgres CTE adapter on the workload that motivated the epic — the
//! same synthetic tree shape `plans/103-in-process-traversal.md`'s own
//! entry-condition measurement used (branching factor 3, so row count grows
//! like `3^depth`), now comparing the two `TraversalEngine` implementations
//! directly rather than "with guard" vs "without guard" on one of them.
//!
//! Not run by the normal gate — timing comparisons are not a correctness
//! property and would just add container-bound wall time to every `cargo
//! test`. Run by hand: `cargo test -p graph-owl-engine-postgres --test
//! traversal_vs_memory -- --ignored --nocapture`. The numbers this produced
//! are transcribed into the plan file; this is what reproduces them.

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::TripleStore;
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, TraversalEngine};
use graph_owl_traversal_memory::InMemoryTraversalEngine;
use std::sync::Arc;
use std::time::Instant;

fn node(id: usize) -> Sid {
    Sid::dsc(format!("n{id}"))
}

fn edge(id: usize, from: usize, to: usize, t: i64) -> Vec<Flake> {
    let subject = Sid::dsc(format!("rel-{id}"));
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
            FlakeValue::String("feeds".to_string()),
            t,
        ),
    ]
}

/// A complete ternary tree, breadth-first: node 0 is the root, each node `i`
/// has children `3i+1`, `3i+2`, `3i+3`. Matches the entry-condition
/// measurement's own fixture shape — branching factor 3, so the walk
/// considers roughly `3^depth` rows at `depth`.
fn ternary_tree_flakes(depth: usize) -> Vec<Flake> {
    let mut flakes = Vec::new();
    let mut frontier = vec![0usize];
    let mut next_id = 1usize;
    let mut edge_id = 0usize;
    for _ in 0..depth {
        let mut next_frontier = Vec::new();
        for &parent in &frontier {
            for _ in 0..3 {
                let child = next_id;
                next_id += 1;
                flakes.extend(edge(edge_id, parent, child, 1));
                edge_id += 1;
                next_frontier.push(child);
            }
        }
        frontier = next_frontier;
    }
    flakes
}

async fn timed_neighbours(
    engine: &dyn TraversalEngine,
    max_hops: usize,
) -> (std::time::Duration, usize) {
    let start = Instant::now();
    let result = engine
        .neighbours(
            &node(0),
            Direction::Outgoing,
            Bounds {
                max_hops,
                max_nodes: usize::MAX,
            },
            &EdgeFilter::default(),
        )
        .await
        .expect("walk");
    (start.elapsed(), result.reached.len())
}

/// Seeds one tree of `depth`, walks it from the root at increasing bounds
/// through both adapters, and prints a table — the same shape as the
/// entry-condition measurement's own table, so the two are directly
/// comparable. `max_hops` values are capped at `depth` since nothing beyond
/// the tree's own depth changes either adapter's answer.
async fn compare_at_depth(depth: usize) {
    let (_database, connection_string) = common::fresh_database().await;
    let postgres = Arc::new(
        PostgresTripleStore::connect(&connection_string)
            .await
            .expect("engine should connect and migrate"),
    );

    let flakes = ternary_tree_flakes(depth);
    eprintln!(
        "seeding a depth-{depth} ternary tree: {} logical edges, {} flakes",
        flakes.len() / 4,
        flakes.len()
    );
    let seed_start = Instant::now();
    postgres.assert_flakes(&flakes).await.expect("seed");
    eprintln!("seeded in {:?}", seed_start.elapsed());

    // `InMemoryTraversalEngine` only needs the `TripleStore` half; the
    // concrete `Arc<PostgresTripleStore>` stays alive above for its own
    // `TraversalEngine` impl, so both adapters read the identical data
    // through the identical connection pool.
    let memory = InMemoryTraversalEngine::new(postgres.clone() as Arc<dyn TripleStore>);

    eprintln!(
        "\n{:>6} | {:>14} | {:>14} | {:>8}",
        "depth", "postgres CTE", "in-memory", "ratio"
    );
    eprintln!("{:->6}-+-{:->14}-+-{:->14}-+-{:->8}", "", "", "", "");

    let steps: Vec<usize> = (1..=depth)
        .step_by((depth / 4).max(1))
        .chain([depth])
        .collect();
    for max_hops in steps {
        let (pg_time, pg_count) = timed_neighbours(postgres.as_ref(), max_hops).await;
        let (mem_time, mem_count) = timed_neighbours(&memory, max_hops).await;

        assert_eq!(
            pg_count, mem_count,
            "the two adapters must agree on reachable-node count at depth {max_hops}"
        );

        let ratio = pg_time.as_secs_f64() / mem_time.as_secs_f64().max(1e-9);
        eprintln!("{max_hops:>6} | {pg_time:>14?} | {mem_time:>14?} | {ratio:>7.2}x");
    }
}

/// The scale the entry-condition measurement itself used (branching factor
/// 3, a tree in the tens of thousands of edges).
#[tokio::test]
#[ignore = "timing measurement, not a correctness gate — run manually with --ignored --nocapture"]
async fn in_memory_adapter_vs_postgres_cte_on_a_branching_tree() {
    compare_at_depth(8).await;
}

/// An order of magnitude larger — looking for the crossover the routing
/// threshold needs, not assuming one exists at the smaller scale.
#[tokio::test]
#[ignore = "timing measurement, not a correctness gate — run manually with --ignored --nocapture"]
async fn in_memory_adapter_vs_postgres_cte_on_a_larger_branching_tree() {
    compare_at_depth(10).await;
}
