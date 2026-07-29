//! Slice C: retraction hides a fact without deleting it.
//!
//! This is the property that makes time-travel native rather than a feature.
//! Every test here checks *two* things — what the current state says, and what
//! is still in the table — because a retraction that also deleted the
//! assertion would pass every current-state assertion on its own.

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

async fn current(store: &PostgresTripleStore) -> Vec<Flake> {
    store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        })
        .await
        .expect("query")
}

async fn rows_in_table(store: &PostgresTripleStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count")
}

#[tokio::test]
async fn a_retracted_fact_disappears_from_current_state() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    assert_eq!(current(&store).await.len(), 1, "asserted, so visible");

    store.retract_flakes(&[named(2)]).await.expect("retract");

    assert!(
        current(&store).await.is_empty(),
        "the fact was retracted and must not be current"
    );
}

/// The whole point. If the assertion row were deleted, the state at `t = 1`
/// would be unrecoverable and history would be a lie.
#[tokio::test]
async fn the_assertion_row_survives_its_own_retraction() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store.retract_flakes(&[named(2)]).await.expect("retract");

    assert_eq!(
        rows_in_table(&store).await,
        2,
        "a retraction is a new row, never a delete"
    );

    let asserting: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes WHERE op")
        .fetch_one(store.pool())
        .await
        .expect("count");
    assert_eq!(asserting, 1, "the original assertion must still be there");
}

/// **The bug this slice exists to prevent.**
///
/// Filtering `op = true` inside the inner `WHERE` rather than the outer query
/// excludes the retraction row — so `DISTINCT ON` picks the *superseded*
/// assertion at `t = 1` and the fact resurrects. That mistake passes the
/// simple assert-then-retract test above and fails only here.
#[tokio::test]
async fn assert_retract_assert_leaves_the_fact_visible() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store.retract_flakes(&[named(2)]).await.expect("retract");
    store.assert_flakes(&[named(3)]).await.expect("re-assert");

    let visible = current(&store).await;
    assert_eq!(
        visible.len(),
        1,
        "the newest operation is an assertion, so the fact is current again"
    );
    assert_eq!(visible[0].t, 3, "and it is the newest one that is visible");
    assert!(visible[0].op);

    assert_eq!(
        rows_in_table(&store).await,
        3,
        "three operations, three rows — one visible"
    );
}

/// The mirror of the above: a retraction *after* a re-assertion must win.
/// A resolution that keyed on "is there any assertion" rather than "what is
/// newest" would pass the previous test and fail this one.
#[tokio::test]
async fn assert_retract_assert_retract_leaves_the_fact_hidden() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    store.retract_flakes(&[named(2)]).await.expect("retract");
    store.assert_flakes(&[named(3)]).await.expect("re-assert");
    store.retract_flakes(&[named(4)]).await.expect("re-retract");

    assert!(current(&store).await.is_empty(), "the newest op wins");
    assert_eq!(rows_in_table(&store).await, 4);
}

/// Retracting something that was never asserted is not an error — a
/// reconciler that re-projects an entity cannot know which facts the previous
/// projection managed to write, so retracting the absent must be survivable.
#[tokio::test]
async fn retracting_a_fact_that_was_never_asserted_is_not_an_error() {
    let (store, _container) = store().await;

    store
        .retract_flakes(&[named(1)])
        .await
        .expect("retracting the absent must be a no-op, not a failure");

    assert!(current(&store).await.is_empty());
}

/// A retraction is scoped to the exact fact it names. Retracting one value of
/// a predicate must not silently clear the others — `dsc:tag` is many-valued,
/// and clearing every tag because one was removed is data loss.
#[tokio::test]
async fn retracting_one_value_leaves_the_others_of_the_same_predicate() {
    let (store, _container) = store().await;

    let tag = |value: &str, t: i64| {
        Flake::assert(
            subject(),
            Sid::dsc("tag"),
            FlakeValue::String(value.into()),
            t,
        )
    };

    store
        .assert_flakes(&[tag("pii", 1), tag("regulated", 1), tag("npci", 1)])
        .await
        .expect("assert");
    store
        .retract_flakes(&[tag("pii", 2)])
        .await
        .expect("retract");

    let remaining = current(&store).await;
    let mut values: Vec<&str> = remaining
        .iter()
        .filter_map(|f| match &f.o {
            FlakeValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    values.sort_unstable();
    assert_eq!(
        values,
        vec!["npci", "regulated"],
        "only the named value should have been withdrawn"
    );
}

/// A retraction is scoped to its predicate too.
#[tokio::test]
async fn retracting_one_predicate_leaves_the_others_of_the_same_subject() {
    let (store, _container) = store().await;

    store
        .assert_flakes(&[
            named(1),
            Flake::assert(
                subject(),
                Sid::dsc("deleted"),
                FlakeValue::Boolean(false),
                1,
            ),
        ])
        .await
        .expect("assert");
    store.retract_flakes(&[named(2)]).await.expect("retract");

    let remaining = current(&store).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].p.id, "deleted");
}

/// And to its graph. The same fact asserted in the default graph and in an
/// extraction graph is two facts; retracting the extraction must not withdraw
/// the catalog's own.
#[tokio::test]
async fn retracting_in_one_graph_leaves_the_same_fact_in_another() {
    let (store, _container) = store().await;
    let extraction = Sid::dsc("graph:extraction");
    let in_extraction = |t: i64| Flake {
        cx: Some(extraction.clone()),
        ..named(t)
    };

    store
        .assert_flakes(&[named(1), in_extraction(1)])
        .await
        .expect("assert");
    store
        .retract_flakes(&[in_extraction(2)])
        .await
        .expect("retract");

    let remaining = current(&store).await;
    assert_eq!(remaining.len(), 1, "got {remaining:?}");
    assert_eq!(remaining[0].cx, None, "the default graph's fact survives");
}

/// `count` must agree with what is *visible*, not with how many rows exist.
/// A count over raw rows would grow every time something was retracted.
#[tokio::test]
async fn count_reflects_visible_facts_not_stored_rows() {
    let (store, _container) = store().await;
    let pattern = TriplePattern {
        s: Some(subject()),
        ..TriplePattern::default()
    };

    store.assert_flakes(&[named(1)]).await.expect("assert");
    assert_eq!(store.count(&pattern).await.expect("count"), 1);

    store.retract_flakes(&[named(2)]).await.expect("retract");
    assert_eq!(
        store.count(&pattern).await.expect("count"),
        0,
        "two rows in the table, zero visible facts"
    );
    assert_eq!(rows_in_table(&store).await, 2);

    store.assert_flakes(&[named(3)]).await.expect("re-assert");
    assert_eq!(store.count(&pattern).await.expect("count"), 1);
    assert_eq!(rows_in_table(&store).await, 3);
}

/// A retraction carrying `op = true` would be an assertion. The method forces
/// the flag, so a caller can hand it the original assertion and get that
/// fact's retraction — which is what every projection update actually does.
#[tokio::test]
async fn retract_forces_the_flag_regardless_of_what_the_caller_passed() {
    let (store, _container) = store().await;

    store.assert_flakes(&[named(1)]).await.expect("assert");
    // named(2) has op = true; retract_flakes must not take it at face value.
    store.retract_flakes(&[named(2)]).await.expect("retract");

    assert!(current(&store).await.is_empty());
    let retracting: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes WHERE NOT op")
        .fetch_one(store.pool())
        .await
        .expect("count");
    assert_eq!(retracting, 1);
}

#[tokio::test]
async fn retracting_an_empty_batch_is_not_an_error() {
    let (store, _container) = store().await;
    store.retract_flakes(&[]).await.expect("empty is fine");
    assert_eq!(rows_in_table(&store).await, 0);
}

/// Retraction goes through the same namespace check as assertion. A retraction
/// with an uninitialized namespace names no fact at all, so it would silently
/// withdraw nothing while appearing to succeed.
#[tokio::test]
async fn a_retraction_with_an_uninitialized_namespace_is_refused() {
    let (store, _container) = store().await;
    let bad = Flake {
        p: Sid::new(graph_owl_core::flake::namespace::UNSET, "name"),
        ..named(1)
    };

    store
        .retract_flakes(&[bad])
        .await
        .expect_err("namespace 0 must be refused on the retract path too");
    assert_eq!(rows_in_table(&store).await, 0);
}
