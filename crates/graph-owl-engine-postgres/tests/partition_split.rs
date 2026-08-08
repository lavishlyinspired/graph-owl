//! Epic 102 Slice A: the flake table splits into a read-optimised
//! `flakes_main` and a write-optimised `flakes_delta`, unified by a `flakes`
//! view every existing reader keeps querying unchanged.
//!
//! Built on an explicit override, not because Epic 37a's own write-throughput
//! trigger fired — see `plans/102-read-write-partitions.md`'s status line.

mod common;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern};
use graph_owl_engine::TripleStore;
use graph_owl_engine_postgres::PostgresTripleStore;

async fn store() -> (PostgresTripleStore, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, database)
}

fn subject() -> Sid {
    Sid::dsc("table-upi-transactions")
}

fn named(t: i64) -> Flake {
    Flake::assert(
        subject(),
        Sid::dsc("name"),
        FlakeValue::String("upi_transactions".into()),
        t,
    )
}

async fn count(store: &PostgresTripleStore, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(store.pool())
        .await
        .unwrap_or_else(|e| panic!("counting {table} should succeed: {e}"))
}

#[tokio::test]
async fn a_fresh_assertion_lands_in_delta_not_main() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");

    assert_eq!(
        count(&store, "flakes_delta").await,
        1,
        "every write goes to delta, the only place `write()` inserts"
    );
    assert_eq!(
        count(&store, "flakes_main").await,
        0,
        "main is never written directly — only compaction moves rows into it"
    );
}

#[tokio::test]
async fn a_fresh_retraction_also_lands_in_delta() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store.retract_flakes(&[named(2)]).await.expect("retract");

    assert_eq!(
        count(&store, "flakes_delta").await,
        2,
        "both the assertion and its retraction are writes, so both go to delta"
    );
    assert_eq!(count(&store, "flakes_main").await, 0);
}

/// The property the whole split exists to preserve: every existing reader —
/// `query_pattern`, and by extension `count`/`explain`/the traversal engine
/// — queries `flakes`, and `flakes` is now a view unioning both partitions.
/// A write landing only in delta must still be visible through it, with no
/// change to the query builders that resolve current state.
#[tokio::test]
async fn the_flakes_view_resolves_current_state_across_both_partitions() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");

    let current = store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        })
        .await
        .expect("query");

    assert_eq!(
        current.len(),
        1,
        "a fact written only to delta must still be current: {current:?}"
    );
}

/// The negative half of the same property: a fact in `flakes_main` (as
/// every previously-migrated row is, since the rename moved existing data
/// there unchanged) must still resolve correctly once a *newer* write for
/// the same fact identity lands in `flakes_delta` — proving the view's
/// `DISTINCT ON ... ORDER BY t DESC` sees across the partition boundary
/// rather than picking whichever partition happens to be scanned first.
#[tokio::test]
async fn a_delta_write_supersedes_an_older_fact_synthetically_placed_in_main() {
    let (store, _container) = store().await;

    // Seed t=1 into delta via the real write path, then move it into main
    // by hand — this slice does not build compaction yet, so this is the
    // only way to get a row into main pre-compaction, standing in for what
    // a real compaction pass will later do transactionally.
    store.assert_flakes(&[named(1)]).await.expect("seed t=1");
    // Named columns, not `SELECT *` — the two tables' physical column
    // orders differ (see the migration's own comment on this), so a
    // positional copy silently pairs the wrong columns.
    sqlx::query(
        "INSERT INTO flakes_main
             (id, namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
              value_ref_ns, value_ref_id, value_str, value_bool, value_int,
              value_float, value_inst, value_json, value_bytes, value_uuid,
              value_lang, value_dir, cx_namespace, cx_id, t, op)
         SELECT id, namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
                value_ref_ns, value_ref_id, value_str, value_bool, value_int,
                value_float, value_inst, value_json, value_bytes, value_uuid,
                value_lang, value_dir, cx_namespace, cx_id, t, op
         FROM flakes_delta",
    )
    .execute(store.pool())
    .await
    .expect("move to main");
    sqlx::query("DELETE FROM flakes_delta")
        .execute(store.pool())
        .await
        .expect("clear delta");
    assert_eq!(
        count(&store, "flakes_main").await,
        1,
        "t=1 now lives only in main"
    );
    assert_eq!(count(&store, "flakes_delta").await, 0);

    // Now retract at t=2 — this write goes to delta, and must supersede
    // the t=1 assertion sitting in main.
    store.retract_flakes(&[named(2)]).await.expect("retract");

    let current = store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        })
        .await
        .expect("query");

    assert!(
        current.is_empty(),
        "a delta retraction must supersede an older assertion sitting in main: {current:?}"
    );
}

// --- Epic 102 Slice B: compaction ---
//
// `compact(batch_size)` moves up to `batch_size` rows from `flakes_delta`
// into `flakes_main`, oldest first, and reports how many it moved. It is
// deliberately built as one Postgres statement per batch (a `DELETE ...
// RETURNING` feeding an `INSERT ... SELECT` in the same CTE) rather than an
// explicit multi-statement transaction: a single statement is atomic by
// construction, with no separate `BEGIN`/`COMMIT` to get wrong, and calling
// it repeatedly with a small `batch_size` is what makes compaction
// interruptible (`102-read-write-partitions.md` decision 3) — each call
// either fully happens or fully does not, so a crash between calls leaves
// whatever rows already moved moved, and whatever had not yet still in
// delta, with the `flakes` view giving the same answer regardless of which
// side any given row is currently on.

fn other_subject() -> Sid {
    Sid::dsc("table-kyc-documents")
}

fn other_named(t: i64) -> Flake {
    Flake::assert(
        other_subject(),
        Sid::dsc("name"),
        FlakeValue::String("kyc_documents".into()),
        t,
    )
}

async fn current_for(store: &PostgresTripleStore, subject: Sid) -> Vec<Flake> {
    store
        .query_pattern(&TriplePattern {
            s: Some(subject),
            ..TriplePattern::default()
        })
        .await
        .expect("query")
}

#[tokio::test]
async fn compact_moves_rows_from_delta_to_main_oldest_first() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store
        .assert_flakes(&[other_named(2)])
        .await
        .expect("assert");
    assert_eq!(count(&store, "flakes_delta").await, 2);

    let moved = store.compact(1).await.expect("compact one batch of 1");

    assert_eq!(moved, 1, "batch_size=1 must move exactly one row");
    assert_eq!(
        count(&store, "flakes_main").await,
        1,
        "the moved row must now live in main"
    );
    assert_eq!(
        count(&store, "flakes_delta").await,
        1,
        "the other row stays"
    );

    // The row moved must be the older one (t=1), not an arbitrary pick.
    let moved_subject: String = sqlx::query_scalar("SELECT sid_s FROM flakes_main")
        .fetch_one(store.pool())
        .await
        .expect("read the moved row");
    assert_eq!(
        moved_subject,
        subject().id,
        "compaction is FIFO — the oldest (lowest id) row moves first"
    );
}

#[tokio::test]
async fn compact_with_nothing_in_delta_moves_zero_and_does_not_error() {
    let (store, _container) = store().await;

    let moved = store.compact(100).await.expect("compact an empty delta");

    assert_eq!(moved, 0, "an empty delta has nothing to move");
}

#[tokio::test]
async fn a_query_returns_identical_results_before_and_after_compaction() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store
        .assert_flakes(&[other_named(2)])
        .await
        .expect("assert");
    store.retract_flakes(&[named(3)]).await.expect("retract");

    let before = current_for(&store, subject()).await;
    let before_other = current_for(&store, other_subject()).await;

    store.compact(100).await.expect("compact everything");
    assert_eq!(
        count(&store, "flakes_delta").await,
        0,
        "compaction cleared delta"
    );

    let after = current_for(&store, subject()).await;
    let after_other = current_for(&store, other_subject()).await;

    assert_eq!(
        before, after,
        "compaction must not change what a query returns"
    );
    assert_eq!(before_other, after_other);
}

/// The single most dangerous bug this design can have (plan decision 2): a
/// retraction written **after** compaction has already moved its assertion
/// into main must still supersede it. Unlike the earlier hand-rolled test
/// above, this exercises the real `compact()` path end to end.
#[tokio::test]
async fn a_retraction_after_compaction_supersedes_the_compacted_assertion() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store
        .compact(100)
        .await
        .expect("compact the assertion into main");
    assert_eq!(current_for(&store, subject()).await.len(), 1);

    store.retract_flakes(&[named(2)]).await.expect("retract");

    assert!(
        current_for(&store, subject()).await.is_empty(),
        "a retraction in delta must supersede its assertion already compacted into main"
    );
}

#[tokio::test]
async fn as_of_returns_identical_results_across_a_compaction_boundary() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert t=1");
    store
        .retract_flakes(&[named(2)])
        .await
        .expect("retract t=2");
    store
        .assert_flakes(&[named(3)])
        .await
        .expect("re-assert t=3");

    async fn as_of(store: &PostgresTripleStore, at: i64) -> Vec<Flake> {
        store
            .query_pattern(&TriplePattern {
                s: Some(subject()),
                as_of: Some(at),
                ..TriplePattern::default()
            })
            .await
            .expect("as-of query")
    }

    let before_1 = as_of(&store, 1).await;
    let before_2 = as_of(&store, 2).await;

    // Compact only the first two writes (t=1, t=2) — leaving t=3 in delta —
    // so the boundary genuinely falls inside the history being queried.
    store.compact(2).await.expect("compact t=1 and t=2");
    assert_eq!(count(&store, "flakes_main").await, 2);
    assert_eq!(count(&store, "flakes_delta").await, 1);

    let after_1 = as_of(&store, 1).await;
    let after_2 = as_of(&store, 2).await;

    assert_eq!(
        before_1, after_1,
        "as-of t=1 must be unaffected by compaction"
    );
    assert_eq!(
        before_2, after_2,
        "as-of t=2 must be unaffected by compaction"
    );
    assert_eq!(before_1.len(), 1, "t=1: the assertion is visible");
    assert!(before_2.is_empty(), "t=2: the retraction has taken effect");
}

/// **Interruptible, and consistent at any point of interruption** (plan
/// decision 3 / AC). Simulated by calling `compact` repeatedly with a small
/// batch size and checking the view resolves correctly *between* calls, not
/// only once the whole thing finishes — a real interruption is indistinguishable
/// from "compaction stopped calling `compact` again", so this is a faithful test.
#[tokio::test]
async fn compaction_in_small_batches_leaves_a_consistent_state_at_every_point() {
    let (store, _container) = store().await;

    for t in 1..=5 {
        store.assert_flakes(&[named(t)]).await.expect("assert");
    }
    let expected = current_for(&store, subject()).await;
    assert_eq!(
        expected.len(),
        1,
        "five assertions of the same fact converge to one row"
    );

    for _ in 0..5 {
        let moved = store.compact(1).await.expect("compact one row");
        assert!(moved <= 1);
        // However far compaction has gotten, split across two partitions,
        // the view must resolve to exactly the same answer every time.
        assert_eq!(
            current_for(&store, subject()).await,
            expected,
            "current state must be correct after every partial compaction step, \
             not only once compaction finishes"
        );
    }

    assert_eq!(count(&store, "flakes_delta").await, 0, "fully compacted");
}
