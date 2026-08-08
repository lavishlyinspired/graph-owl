//! Epic 102 decision 5: "a merged read is measured before it is adopted...
//! if it costs more on reads than compaction saves on writes, the split is
//! a loss and must not ship out of sunk-cost."
//!
//! Deliberately a **new** file rather than an edit to
//! `scale_partition_trigger.rs` — that file answers Epic 37a Slice C's own
//! question ("when does the write-amplification trigger fire", answered:
//! not yet, flat 53,641-57,919 flakes/s across 1M-10M) and its recorded
//! numbers are the "before" baseline this file compares against. This file
//! answers a different question: having built the mechanism, is adopting
//! it — right now, at this scale, on this measurement — a net win.
//!
//! Same direct-insertion methodology as Slice C's own harness (`assert_flakes`,
//! not the HTTP/asset surface — see that file's own comment on why), same
//! predicate shape, same pattern-query shape, so every number here is
//! comparable to Slice C's recorded ones without inventing a new baseline.
//!
//! Not run by the normal gate. Run by hand:
//! `cargo test -p graph-owl-engine-postgres --test scale_partition_adopted -- --ignored --nocapture`

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace, value_type};
use graph_owl_engine::{PredicateDef, PredicateRegistry, TripleStore};
use graph_owl_engine_postgres::PostgresTripleStore;
use std::time::{Duration, Instant};

const PREDICATES: &[(&str, i16)] = &[
    ("name", value_type::STRING),
    ("fqn", value_type::STRING),
    ("description", value_type::STRING),
    ("kind", value_type::STRING),
    ("ordinal", value_type::INT),
];

async fn define_predicates(store: &PostgresTripleStore) {
    for (name, vtype) in PREDICATES {
        store
            .define(&PredicateDef {
                namespace: namespace::RUNTIME_START,
                name: (*name).to_string(),
                value_type: *vtype,
                many: false,
                core: false,
            })
            .await
            .expect("predicate definition should succeed");
    }
}

fn entity_batch(first_entity: u64, count: u64, t: i64) -> Vec<Flake> {
    let mut flakes = Vec::with_capacity(count as usize * PREDICATES.len());
    for entity in first_entity..first_entity + count {
        let subject = Sid::new(
            namespace::RUNTIME_START,
            format!("scale-entity-{entity:010}"),
        );
        for (name, vtype) in PREDICATES {
            let value = if *vtype == value_type::INT {
                FlakeValue::Int(i64::try_from(entity).unwrap_or(i64::MAX))
            } else {
                FlakeValue::String(format!("{name}-{entity}"))
            };
            flakes.push(Flake::assert(
                subject.clone(),
                Sid::new(namespace::RUNTIME_START, (*name).to_string()),
                value,
                t,
            ));
        }
    }
    flakes
}

/// Identical query shape to Slice C's own `pattern_query_p99` — comparable
/// numbers depend on it.
async fn pattern_query_p99(store: &PostgresTripleStore, reps: usize) -> Duration {
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let start = Instant::now();
        sqlx::query(
            "SELECT sid_s FROM flakes \
             WHERE namespace_p = $1 AND sid_p = 'name' AND op = true \
             ORDER BY t DESC LIMIT 100",
        )
        .bind(i32::from(namespace::RUNTIME_START))
        .fetch_all(store.pool())
        .await
        .expect("pattern query should run");
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[samples.len() - 1]
}

/// **Both tables, not `'flakes'::regclass`** — `flakes` is a view now
/// (`V9__flakes_delta_partition.sql`), and a view has no entry in
/// `pg_index`; querying it directly would silently report zero regardless
/// of real index size. Found checking, not assumed from Slice C's original
/// version of this helper, which targeted the table `flakes` used to be.
async fn total_index_bytes(store: &PostgresTripleStore) -> i64 {
    let (bytes,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(pg_relation_size(indexrelid)), 0)::BIGINT \
         FROM pg_index \
         WHERE indrelid = 'flakes_main'::regclass OR indrelid = 'flakes_delta'::regclass",
    )
    .fetch_one(store.pool())
    .await
    .expect("index size query should run");
    bytes
}

async fn explain_pattern_query(store: &PostgresTripleStore) -> String {
    let rows: Vec<(String,)> = sqlx::query_as(&format!(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) \
         SELECT sid_s FROM flakes \
         WHERE namespace_p = {} AND sid_p = 'name' AND op = true \
         ORDER BY t DESC LIMIT 100",
        namespace::RUNTIME_START
    ))
    .fetch_all(store.pool())
    .await
    .expect("EXPLAIN ANALYZE should run");
    rows.into_iter()
        .map(|(l,)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

fn target_flakes() -> u64 {
    std::env::var("GRAPH_OWL_SCALE_FLAKES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "the decision-5 measurement — minutes; run explicitly"]
async fn adopting_the_split_is_measured_not_assumed() {
    let (_db, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("connect");
    define_predicates(&store).await;

    let target = target_flakes();
    const BATCH_ENTITIES: u64 = 10_000;
    let batches_needed = target.div_ceil(BATCH_ENTITIES * PREDICATES.len() as u64);

    println!("writing {target} flakes to flakes_delta (the only place a write can land)...");
    let mut batch_samples =
        Vec::with_capacity(usize::try_from(batches_needed).unwrap_or(usize::MAX));
    let mut next_entity = 0u64;
    let write_start = Instant::now();
    for t in 1i64..=i64::try_from(batches_needed).unwrap_or(i64::MAX) {
        let batch = entity_batch(next_entity, BATCH_ENTITIES, t);
        let start = Instant::now();
        store
            .assert_flakes(&batch)
            .await
            .expect("bulk insert should succeed");
        batch_samples.push(start.elapsed());
        next_entity += BATCH_ENTITIES;
    }
    let write_elapsed = write_start.elapsed();
    let written: u64 = next_entity * PREDICATES.len() as u64;
    let write_throughput = written as f64 / write_elapsed.as_secs_f64();
    batch_samples.sort();
    let write_p99 = batch_samples.last().copied().unwrap_or(Duration::ZERO);

    println!(
        "write: {written} flakes in {write_elapsed:?} — {write_throughput:.0} flakes/s \
         (batch p99 {write_p99:?}). Slice C's recorded baseline (4-index single table): \
         53,641-57,919 flakes/s."
    );

    sqlx::query("VACUUM flakes_delta")
        .execute(store.pool())
        .await
        .expect("VACUUM should run");

    let pre_compaction_p99 = pattern_query_p99(&store, 20).await;
    let pre_compaction_plan = explain_pattern_query(&store).await;
    println!(
        "pattern query BEFORE compaction (all {written} flakes in delta, 1 index): \
         p99={pre_compaction_p99:?}\n---- plan ----\n{pre_compaction_plan}\n----"
    );

    println!("compacting everything into flakes_main...");
    let compact_start = Instant::now();
    let mut total_moved = 0u64;
    loop {
        let moved = store.compact(50_000).await.expect("compact should succeed");
        total_moved += moved;
        if moved == 0 {
            break;
        }
    }
    let compact_elapsed = compact_start.elapsed();
    println!("compacted {total_moved} flakes into flakes_main in {compact_elapsed:?}");
    assert_eq!(
        total_moved, written,
        "every written flake must have been compacted — a mismatch here means \
         compaction lost or duplicated rows, the correctness property this \
         measurement must not silently paper over"
    );

    sqlx::query("VACUUM flakes_main")
        .execute(store.pool())
        .await
        .expect("VACUUM should run");
    sqlx::query("ANALYZE flakes_main")
        .execute(store.pool())
        .await
        .expect("ANALYZE should run");

    let post_compaction_p99 = pattern_query_p99(&store, 20).await;
    let post_compaction_plan = explain_pattern_query(&store).await;
    println!(
        "pattern query AFTER compaction (all {written} flakes in main, 4 indexes): \
         p99={post_compaction_p99:?}\n---- plan ----\n{post_compaction_plan}\n----"
    );

    let index_bytes = total_index_bytes(&store).await;
    println!(
        "total index bytes across both partitions: {index_bytes} ({:.1} MB)",
        index_bytes as f64 / (1024.0 * 1024.0)
    );

    // Decision 5's own wording: "if it costs more on reads than compaction
    // saves on writes, the split is a loss and must not ship out of
    // sunk-cost." No hard assertion, matching Slice C's own precedent for
    // this class of open, judgement-requiring measurement — the honest
    // verdict is printed for the plan's account to quote directly, not
    // computed here as a boolean nobody derived a threshold for.
    println!(
        "\n==== SUMMARY ====\n\
         write throughput (delta, 1 index): {write_throughput:.0} flakes/s \
         vs Slice C baseline (4-index single table): 53,641-57,919 flakes/s\n\
         read latency before compaction (delta only): {pre_compaction_p99:?}\n\
         read latency after compaction (main only): {post_compaction_p99:?}\n\
         read latency vs Slice C baseline at the same {target} scale: \
         see that file's own recorded pattern_query_p99 for this checkpoint"
    );
}
