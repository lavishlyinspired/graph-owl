//! Slice B: all four index orderings serve their query shape.
//!
//! These are the only tests in the epic that assert on a *plan* rather than a
//! result. That is the point: a dropped or unusable index changes nothing
//! about correctness, so every functional test still passes while every query
//! degrades to a sequential scan. Only the plan shows it.
//!
//! The table is loaded to 100k flakes first. Below a few thousand rows
//! Postgres correctly prefers a sequential scan no matter what indexes exist,
//! so a plan assertion on a small table would assert the planner's arithmetic
//! rather than this schema's design.

mod common;

use graph_owl_core::flake::TriplePattern;
use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_engine::TripleStore;
use graph_owl_engine_postgres::PostgresTripleStore;

const SUBJECTS: i64 = 10_000;
/// 10 predicates x 10k subjects = 100k flakes, the size the plan's acceptance
/// criteria name.
const PREDICATES: [&str; 10] = [
    "name",
    "description",
    "fqn",
    "ordinalPosition",
    "dataType",
    "nullable",
    "updatedAt",
    "owner",
    "parentTable",
    "confidence",
];

async fn loaded_store() -> (PostgresTripleStore, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");

    for chunk_start in (0..SUBJECTS).step_by(1_000) {
        let mut batch = Vec::with_capacity(10_000);
        for subject in chunk_start..(chunk_start + 1_000).min(SUBJECTS) {
            for (index, predicate) in PREDICATES.iter().enumerate() {
                // `owner` and `parentTable` are references so OPST has
                // something to scan; the rest are literals so POST does.
                let value = match *predicate {
                    "owner" => FlakeValue::Ref(Sid::dsc(format!("team-{}", subject % 50))),
                    "parentTable" => FlakeValue::Ref(Sid::dsc(format!("table-{}", subject % 500))),
                    "ordinalPosition" => {
                        FlakeValue::Int(i64::try_from(index).expect("predicate index is small"))
                    }
                    "nullable" => FlakeValue::Boolean(subject % 2 == 0),
                    "confidence" => FlakeValue::Float(0.5),
                    _ => FlakeValue::String(format!("{predicate}-value-{subject}")),
                };
                batch.push(Flake::assert(
                    Sid::dsc(format!("column-{subject}")),
                    Sid::dsc(*predicate),
                    value,
                    1,
                ));
            }
        }
        store.assert_flakes(&batch).await.expect("bulk load");
    }

    // Epic 102: `assert_flakes` writes only to `flakes_delta` (minimal
    // index — see `V9__flakes_delta_partition.sql`), and this test is
    // specifically about `flakes_main`'s four *read-optimised* orderings,
    // not the write path. Moved by hand rather than through a real
    // compaction pass, which this epic has not built yet — the same
    // stand-in `tests/partition_split.rs` uses, and for the same reason:
    // this test's premise ("100k flakes, check the plan uses the right
    // index") is a property of `flakes_main`'s schema, independent of how
    // a row arrives there.
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

    // Without fresh statistics the planner is working from defaults and its
    // choice says nothing about the data. `flakes` is a view now (the same
    // migration) and `ANALYZE` cannot target one — it silently skips with a
    // warning rather than failing, found only by checking, not assuming.
    sqlx::query("ANALYZE flakes_main")
        .execute(store.pool())
        .await
        .expect("analyze");

    (store, database)
}

fn assert_uses_index(plan: &str, expected: &str, shape: &str) {
    assert!(
        plan.contains(expected),
        "{shape} should scan {expected}.\nPlan was:\n{plan}"
    );
}

/// **`flakes_main` specifically, not the whole plan text — found running
/// this suite after Epic 102's split, not designed around in advance.**
/// `flakes_delta` intentionally carries only one index (the SPOT-style
/// identity constraint; `V9__flakes_delta_partition.sql`), so a shape that
/// index cannot serve — e.g. `(?, p, ?)`, which does not bind subject —
/// legitimately falls back to `Seq Scan on flakes_delta` in the plan. That
/// is by design, not a regression: it is the entire tradeoff the split
/// makes, and it stays cheap only because delta is meant to stay small
/// between compaction passes. What this test must still catch is a scan of
/// `flakes_main` — the table all four orderings exist to keep indexed —
/// which is the only sequential scan this schema was ever supposed to
/// avoid.
fn assert_no_sequential_scan(plan: &str, shape: &str) {
    assert!(
        !plan.contains("Seq Scan on flakes_main"),
        "{shape} fell back to a sequential scan over flakes_main.\nPlan was:\n{plan}"
    );
}

/// One database, one load, every shape — building 100k flakes per test would
/// dominate the suite for no extra coverage.
#[tokio::test]
async fn every_pattern_shape_is_served_by_its_index() {
    let (store, _container) = loaded_store().await;

    let subject = Sid::dsc("column-42");
    let predicate = Sid::dsc("name");

    // (s, ?, ?) -> SPOT
    let plan = store
        .explain(&TriplePattern {
            s: Some(subject.clone()),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");
    assert_uses_index(&plan, "idx_flakes_spot", "(s, ?, ?)");
    assert_no_sequential_scan(&plan, "(s, ?, ?)");

    // (s, p, ?) -> SPOT or PSOT. Both bind subject and predicate completely,
    // so both are correct; the planner picks whichever is cheaper and PSOT is
    // the narrower row. Naming one specifically would assert the planner's
    // cost arithmetic rather than anything about this schema. What must not
    // happen is a scan, and that is what is asserted.
    let plan = store
        .explain(&TriplePattern {
            s: Some(subject.clone()),
            p: Some(predicate.clone()),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");
    assert!(
        plan.contains("idx_flakes_spot") || plan.contains("idx_flakes_psot"),
        "(s, p, ?) should scan an index binding both terms.\nPlan was:\n{plan}"
    );
    assert_no_sequential_scan(&plan, "(s, p, ?)");

    // (?, p, ?) -> PSOT or POST. Both lead with the predicate, so either is a
    // correct index choice; forcing PSOT would assert the planner's cost
    // arithmetic rather than this schema's design.
    let plan = store
        .explain(&TriplePattern {
            p: Some(predicate.clone()),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");
    assert!(
        plan.contains("idx_flakes_psot") || plan.contains("idx_flakes_post"),
        "(?, p, ?) should scan a predicate-leading index.\nPlan was:\n{plan}"
    );
    assert_no_sequential_scan(&plan, "(?, p, ?)");

    // (?, p, o) with a literal object -> POST
    let plan = store
        .explain(&TriplePattern {
            p: Some(predicate),
            o: Some(FlakeValue::String("name-value-42".into())),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");
    assert_uses_index(&plan, "idx_flakes_post", "(?, p, o) literal");
    assert_no_sequential_scan(&plan, "(?, p, o) literal");

    // (?, ?, o) where o is a reference -> OPST, the partial index
    let plan = store
        .explain(&TriplePattern {
            o: Some(FlakeValue::Ref(Sid::dsc("team-7"))),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");
    assert_uses_index(&plan, "idx_flakes_opst", "(?, ?, o) reference");
    assert_no_sequential_scan(&plan, "(?, ?, o) reference");
}

/// OPST exists so reverse traversal is an index seek rather than a scan of
/// every reference in the graph. If a literal object could reach it, the
/// partial predicate would be wrong and the index four times its size.
#[tokio::test]
async fn a_literal_object_lookup_does_not_use_the_reference_only_index() {
    let (store, _container) = loaded_store().await;

    let plan = store
        .explain(&TriplePattern {
            o: Some(FlakeValue::String("name-value-42".into())),
            ..TriplePattern::default()
        })
        .await
        .expect("explain");

    assert!(
        !plan.contains("idx_flakes_opst"),
        "OPST is declared WHERE value_type = 0 and cannot serve a string \
         object.\nPlan was:\n{plan}"
    );
}

/// The plan assertions above are only meaningful if the same patterns also
/// return the right rows — an index scan over a wrong predicate is still fast.
#[tokio::test]
async fn the_indexed_shapes_return_the_rows_they_claim_to() {
    let (store, _container) = loaded_store().await;

    let by_subject = store
        .query_pattern(&TriplePattern {
            s: Some(Sid::dsc("column-42")),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    assert_eq!(
        by_subject.len(),
        PREDICATES.len(),
        "one flake per predicate"
    );

    let by_reference = store
        .query_pattern(&TriplePattern {
            o: Some(FlakeValue::Ref(Sid::dsc("team-7"))),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    // subject % 50 == 7 over 10k subjects
    assert_eq!(
        by_reference.len(),
        200,
        "reverse traversal must find every subject pointing at team-7"
    );
    assert!(
        by_reference.iter().all(|f| f.p.id == "owner"),
        "only the owner predicate points at a team"
    );

    let by_predicate_and_object = store
        .query_pattern(&TriplePattern {
            p: Some(Sid::dsc("name")),
            o: Some(FlakeValue::String("name-value-42".into())),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    assert_eq!(by_predicate_and_object.len(), 1);
    assert_eq!(by_predicate_and_object[0].s.id, "column-42");
}
