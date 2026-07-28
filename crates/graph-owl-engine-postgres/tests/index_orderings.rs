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

use graph_owl_core::flake::TriplePattern;
use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_engine::TripleStore;
use graph_owl_engine_postgres::PostgresTripleStore;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

/// Pinned, not defaulted: `Postgres::default()` is `postgres:11-alpine`, which
/// predates generated columns and every planner behaviour this project's design
/// notes assume.
///
/// The **major** is pinned and the minor floats, so a security release arrives
/// without a manual bump while a major upgrade stays a deliberate decision.
/// See `plans/00g-operations.md`, "Supported PostgreSQL versions".
const POSTGRES_IMAGE_TAG: &str = "16-alpine";

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

async fn loaded_store() -> (PostgresTripleStore, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_tag(POSTGRES_IMAGE_TAG)
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

    // Without fresh statistics the planner is working from defaults and its
    // choice says nothing about the data.
    sqlx::query("ANALYZE flakes")
        .execute(store.pool())
        .await
        .expect("analyze");

    (store, container)
}

fn assert_uses_index(plan: &str, expected: &str, shape: &str) {
    assert!(
        plan.contains(expected),
        "{shape} should scan {expected}.\nPlan was:\n{plan}"
    );
}

fn assert_no_sequential_scan(plan: &str, shape: &str) {
    assert!(
        !plan.contains("Seq Scan on flakes"),
        "{shape} fell back to a sequential scan over 100k flakes.\nPlan was:\n{plan}"
    );
}

/// One container, one load, every shape — building 100k flakes per test would
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
