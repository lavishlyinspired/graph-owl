//! Epic 37a Slice C: the flake-table partitioning trigger, measured rather
//! than assumed — `04-engine-triples.md` says "start unpartitioned,
//! partition by `namespace_s` at ~10M"; this is what turns that number from
//! a guess into a measurement, and is licensed to move it.
//!
//! **Direct flake insertion, not entity restore.** The AC is about the
//! storage layer's index and write behaviour, not the asset API — going
//! through `POST /ingest` or `/admin/restore` for 10M flakes would spend
//! the measurement's wall time on HTTP/JSON overhead rather than on the
//! thing being measured. `TripleStore::assert_flakes` (`graph-owl-engine`)
//! is the real, only write path into the `flakes` table; this drives it
//! directly, the way `graph-owl-connectors` and `Catalog::project_*`
//! already do internally.
//!
//! Not run by the normal gate — this is the single most expensive
//! measurement in the plan. Run by hand:
//! `cargo test -p graph-owl-engine-postgres --test scale_partition_trigger -- --ignored --nocapture`

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace, value_type};
use graph_owl_engine::{PredicateDef, PredicateRegistry, TripleStore};
use graph_owl_engine_postgres::PostgresTripleStore;
use std::time::{Duration, Instant};

/// Five properties per synthetic entity, mirroring a real asset's
/// projection shape (name, fqn, description, kind, a numeric field) rather
/// than one degenerate predicate repeated — the four indexes lead with
/// `(namespace_s, sid_s)` or `(namespace_p, sid_p)`, and a single predicate
/// would make the predicate-first indexes trivially small regardless of
/// row count, understating their real cost.
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

/// `count` synthetic entities, each carrying every predicate in
/// [`PREDICATES`] — `count * PREDICATES.len()` flakes, starting at
/// `first_entity`, so successive calls extend the keyspace rather than
/// re-writing it (idempotent re-assertion at the same `t` would just be a
/// no-op and understate real insert cost).
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

struct Checkpoint {
    flakes: u64,
    write_p99: Duration,
    write_throughput_per_sec: f64,
    pattern_query_p99: Duration,
    index_only_scan: bool,
    index_bytes_total: i64,
}

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

/// Whether the pattern query gets an Index Only Scan. **Measured, and the
/// honest answer at every checkpoint was no** — not from a stale
/// visibility map (`VACUUM` at the call site did not change it either),
/// but architecturally: this query filters on `op` (current vs.
/// retracted), and `op` is not a column of `idx_flakes_post`
/// `(namespace_p, sid_p, value_type, value_key, namespace_s, sid_s, t
/// DESC)`. Postgres chooses a Bitmap Heap Scan instead — the index
/// narrows candidates, but every candidate still needs a heap fetch to
/// check `op`. A schema change (an `op`-partial index, or `op` appended
/// to this ordering) would be required to change this, which is out of
/// scope for a benchmarking slice; recorded as a finding, not chased.
async fn index_only_scan_ratio(store: &PostgresTripleStore) -> bool {
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
    let plan: String = rows
        .into_iter()
        .map(|(l,)| l)
        .collect::<Vec<_>>()
        .join("\n");
    println!("---- pattern query plan ----\n{plan}\n----");
    plan.contains("Index Only Scan")
}

async fn total_index_bytes(store: &PostgresTripleStore) -> i64 {
    let (bytes,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(pg_relation_size(indexrelid)), 0)::BIGINT \
         FROM pg_index WHERE indrelid = 'flakes'::regclass",
    )
    .fetch_one(store.pool())
    .await
    .expect("index size query should run");
    bytes
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "the single most expensive measurement in the plan — minutes to tens of minutes"]
async fn partitioning_trigger_measured_at_1m_5m_10m_flakes() {
    let (_db, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("connect");
    define_predicates(&store).await;

    let (shared_buffers,): (String,) = sqlx::query_as("SHOW shared_buffers")
        .fetch_one(store.pool())
        .await
        .expect("SHOW shared_buffers should run");
    println!("shared_buffers = {shared_buffers}");

    const BATCH_ENTITIES: u64 = 10_000; // 50,000 flakes/batch (5 predicates each)
    // Overridable for a fast local sanity pass before committing to the
    // real 1M/5M/10M run, which takes minutes — mirrors
    // `GRAPH_OWL_SCALE_TABLES` in the read/traversal-budget harnesses.
    let checkpoints_at: Vec<u64> = std::env::var("GRAPH_OWL_SCALE_FLAKE_CHECKPOINTS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|n| n.trim().parse().expect("comma-separated integers"))
                .collect()
        })
        .unwrap_or_else(|| vec![1_000_000, 5_000_000, 10_000_000]);
    let mut results = Vec::new();
    let mut next_entity = 0u64;
    let mut t = 1i64;

    for &checkpoint_flakes in &checkpoints_at {
        let mut batch_p99_samples = Vec::new();
        let mut inserted_since_checkpoint = 0u64;
        let already_have = next_entity * PREDICATES.len() as u64;
        let need = checkpoint_flakes.saturating_sub(already_have);
        let batches_needed = need.div_ceil(BATCH_ENTITIES * PREDICATES.len() as u64);

        let phase_start = Instant::now();
        for _ in 0..batches_needed {
            let batch = entity_batch(next_entity, BATCH_ENTITIES, t);
            let batch_flakes = batch.len() as u64;
            let start = Instant::now();
            store
                .assert_flakes(&batch)
                .await
                .expect("bulk insert should succeed");
            batch_p99_samples.push(start.elapsed());
            next_entity += BATCH_ENTITIES;
            t += 1;
            inserted_since_checkpoint += batch_flakes;
        }
        let phase_elapsed = phase_start.elapsed();

        batch_p99_samples.sort();
        let write_p99 = batch_p99_samples.last().copied().unwrap_or(Duration::ZERO);
        let write_throughput_per_sec = if phase_elapsed.as_secs_f64() > 0.0 {
            inserted_since_checkpoint as f64 / phase_elapsed.as_secs_f64()
        } else {
            0.0
        };

        // Run regardless of whether it changes the plan below — freshly
        // bulk-inserted rows have no visibility-map bit set, which forces a
        // heap fetch on any scan until a vacuum runs, and measuring across
        // that transient bulk-load state (rather than the steady state an
        // operator's own autovacuum produces) would be measuring the wrong
        // thing. It turned out not to be the deciding factor for the
        // specific pattern query below — see the comment at its call site.
        sqlx::query("VACUUM flakes")
            .execute(store.pool())
            .await
            .expect("VACUUM should run");

        let pattern_p99 = pattern_query_p99(&store, 20).await;
        let index_only = index_only_scan_ratio(&store).await;
        let index_bytes = total_index_bytes(&store).await;

        let checkpoint = Checkpoint {
            flakes: checkpoint_flakes,
            write_p99,
            write_throughput_per_sec,
            pattern_query_p99: pattern_p99,
            index_only_scan: index_only,
            index_bytes_total: index_bytes,
        };
        println!(
            "checkpoint {}: write_p99_per_batch={:?} throughput={:.0} flakes/s \
             pattern_query_p99={:?} index_only_scan={} index_bytes={} ({:.1} MB)",
            checkpoint.flakes,
            checkpoint.write_p99,
            checkpoint.write_throughput_per_sec,
            checkpoint.pattern_query_p99,
            checkpoint.index_only_scan,
            checkpoint.index_bytes_total,
            checkpoint.index_bytes_total as f64 / (1024.0 * 1024.0),
        );
        results.push(checkpoint);
    }

    // BRIN evaluation against the final, 10M-flake state — `t` is a
    // trailing column on every existing index already, so this measures
    // whether a dedicated BRIN buys anything *additional* for a
    // time-range query, not whether `t` can be looked up at all.
    sqlx::query("CREATE INDEX flakes_brin_t_scale_check ON flakes USING BRIN (t)")
        .execute(store.pool())
        .await
        .expect("BRIN index creation should succeed");
    let (brin_bytes,): (i64,) =
        sqlx::query_as("SELECT pg_relation_size('flakes_brin_t_scale_check')")
            .fetch_one(store.pool())
            .await
            .expect("BRIN size query should run");
    let btree_bytes_for_t_alone = results.last().map_or(0, |c| c.index_bytes_total);
    println!(
        "BRIN(t) size = {brin_bytes} bytes ({:.2} MB) vs total B-tree index bytes = {} \
         ({:.1} MB) — BRIN is a trailing column on all four B-trees already, \
         so this is the marginal cost/benefit of a dedicated index, not the \
         only way to reach `t`.",
        brin_bytes as f64 / (1024.0 * 1024.0),
        btree_bytes_for_t_alone,
        btree_bytes_for_t_alone as f64 / (1024.0 * 1024.0),
    );

    let range_start = t / 4;
    let range_end = t / 2;
    let rows: Vec<(String,)> = sqlx::query_as(&format!(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) \
         SELECT COUNT(*) FROM flakes WHERE t BETWEEN {range_start} AND {range_end}"
    ))
    .fetch_all(store.pool())
    .await
    .expect("EXPLAIN ANALYZE should run");
    let range_plan: String = rows
        .into_iter()
        .map(|(l,)| l)
        .collect::<Vec<_>>()
        .join("\n");
    println!("---- time-range query plan (BRIN available) ----\n{range_plan}\n----");
    println!(
        "BRIN chosen by the planner: {}",
        range_plan.contains("flakes_brin_t_scale_check")
    );

    // No hard assertions on the BRIN result — decision 5 in the plan is
    // explicit that a negative result is as useful to record as a positive
    // one. The partitioning-trigger numbers above are what the plan's own
    // acceptance criteria need; whether they cross Epic 4's ~10M guess is
    // a judgement made from the printed report, not a boolean this test
    // can assert without inventing a threshold nobody has derived yet.
    for checkpoint in &results {
        println!(
            "SUMMARY {} flakes: write_throughput={:.0}/s pattern_p99={:?} index_bytes={}",
            checkpoint.flakes,
            checkpoint.write_throughput_per_sec,
            checkpoint.pattern_query_p99,
            checkpoint.index_bytes_total
        );
    }
}
