//! Slice A: flakes round-trip through Postgres.
//!
//! Every assertion here is against a real database. The value encoding is
//! unit-tested in `src/value.rs`; what cannot be tested without Postgres is
//! whether the columns, the CHECK constraints and the identity index actually
//! agree with that encoding — which is exactly what these cover.

mod common;

use chrono::{DateTime, TimeZone, Utc};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use graph_owl_engine::{PredicateDef, PredicateRegistry, TripleStore};
use graph_owl_engine_postgres::PostgresTripleStore;

/// The database handle must be returned and bound by the caller. If it is
/// dropped here, Docker tears the database down and the next query fails with
/// a pool timeout that looks nothing like the actual cause.
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

fn flake(predicate: &str, value: FlakeValue, t: i64) -> Flake {
    Flake::assert(subject(), Sid::dsc(predicate), value, t)
}

/// Where a test's own vocabulary goes.
///
/// The value-variant tests below need ten distinct predicates to tell ten
/// variants apart, and the catalog vocabulary has no `Bytes`, `Uuid` or
/// `Duration` predicate to borrow. Inventing them in the `dsc:` namespace
/// would put runtime definitions in the range core migrations own; the
/// runtime range is where an organisation's own terms belong, and defining
/// them before use is the extension path Slice H exists to provide.
const TEST_NS: u16 = namespace::RUNTIME_START;

fn runtime_flake(predicate: &str, value: FlakeValue, t: i64) -> Flake {
    Flake::assert(subject(), Sid::new(TEST_NS, predicate), value, t)
}

/// Defines each predicate with the value type it is actually used with, so
/// the registry is not left describing a vocabulary the test contradicts.
async fn define_test_vocabulary(store: &PostgresTripleStore, cases: &[(&str, FlakeValue)]) {
    for (name, value) in cases {
        store
            .define(&PredicateDef {
                namespace: TEST_NS,
                name: (*name).to_string(),
                value_type: value.value_type(),
                many: false,
                core: false,
            })
            .await
            .unwrap_or_else(|e| panic!("define {name}: {e}"));
    }
}

async fn all_for_subject(store: &PostgresTripleStore) -> Vec<Flake> {
    store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        })
        .await
        .expect("query")
}

#[tokio::test]
async fn an_asserted_flake_comes_back_exactly_as_written() {
    let (store, _container) = store().await;
    let written = flake("name", FlakeValue::String("upi_transactions".into()), 1);

    store
        .assert_flakes(std::slice::from_ref(&written))
        .await
        .expect("write");
    let read = all_for_subject(&store).await;

    assert_eq!(read, vec![written], "a flake must survive its own storage");
}

/// One test per variant would pass with a discriminant that collapsed two of
/// them, as long as each was checked in isolation. Writing all ten at once and
/// matching them back by predicate is what catches the collapse.
#[tokio::test]
async fn every_value_variant_round_trips() {
    let (store, _container) = store().await;
    let instant = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let cases: Vec<(&str, FlakeValue)> = vec![
        ("owner", FlakeValue::Ref(Sid::dsc("team-payments"))),
        ("name", FlakeValue::String("upi_transactions".into())),
        ("deleted", FlakeValue::Boolean(false)),
        ("ordinalPosition", FlakeValue::Int(-3)),
        ("confidence", FlakeValue::Float(0.95)),
        ("updatedAt", FlakeValue::Instant(instant)),
        ("extension", FlakeValue::Json("{\"npci\":true}".into())),
        ("checksum", FlakeValue::Bytes(vec![0, 1, 0x0a, 255])),
        ("entityId", FlakeValue::Uuid(uuid::Uuid::from_u128(9))),
        ("freshnessSla", FlakeValue::Duration(86_400)),
    ];

    define_test_vocabulary(&store, &cases).await;
    let flakes: Vec<Flake> = cases
        .iter()
        .enumerate()
        .map(|(i, (predicate, value))| {
            let t = i64::try_from(i).expect("index fits") + 1;
            runtime_flake(predicate, value.clone(), t)
        })
        .collect();
    store.assert_flakes(&flakes).await.expect("write");

    let read = all_for_subject(&store).await;
    assert_eq!(read.len(), cases.len(), "every flake should be readable");

    for (predicate, expected) in &cases {
        let found = read
            .iter()
            .find(|f| f.p.id == *predicate)
            .unwrap_or_else(|| panic!("{predicate} is missing from {read:?}"));
        assert_eq!(&found.o, expected, "{predicate} did not round-trip");
    }
}

/// Postgres `TIMESTAMPTZ` is microsecond precision and `chrono` is nanosecond.
/// The nanoseconds are lost on write, so the value read back differs from the
/// value handed in — which is fine only if it is *known*, because an equality
/// assertion on a round-tripped timestamp elsewhere would fail mysteriously.
#[tokio::test]
async fn instants_round_trip_at_microsecond_precision() {
    let (store, _container) = store().await;

    let micros: DateTime<Utc> = Utc.timestamp_opt(1_700_000_000, 123_456_000).unwrap();

    store
        .assert_flakes(&[flake("updatedAt", FlakeValue::Instant(micros), 1)])
        .await
        .expect("write");

    let read = all_for_subject(&store).await;
    assert_eq!(
        read[0].o,
        FlakeValue::Instant(micros),
        "microsecond precision must survive intact"
    );
}

/// `NaN`, `inf` and `-inf` are real `f64` values and Postgres stores all three.
/// `NaN != NaN` in Rust, so this also pins that the comparison used here is
/// the stored bit pattern rather than IEEE equality.
#[tokio::test]
async fn non_finite_floats_survive_storage() {
    let (store, _container) = store().await;
    let cases: Vec<(&str, FlakeValue)> = vec![
        ("nan", FlakeValue::Float(f64::NAN)),
        ("inf", FlakeValue::Float(f64::INFINITY)),
        ("negInf", FlakeValue::Float(f64::NEG_INFINITY)),
    ];
    define_test_vocabulary(&store, &cases).await;
    store
        .assert_flakes(&[
            runtime_flake("nan", FlakeValue::Float(f64::NAN), 1),
            runtime_flake("inf", FlakeValue::Float(f64::INFINITY), 2),
            runtime_flake("negInf", FlakeValue::Float(f64::NEG_INFINITY), 3),
        ])
        .await
        .expect("write");

    let read = all_for_subject(&store).await;
    let by = |name: &str| -> f64 {
        match read.iter().find(|f| f.p.id == name).map(|f| &f.o) {
            Some(FlakeValue::Float(f)) => *f,
            other => panic!("{name} came back as {other:?}"),
        }
    };
    assert!(by("nan").is_nan(), "NaN must not become 0 or NULL");
    // Exact comparison is the point: these are stored bit patterns round-
    // tripped through Postgres, not the result of any arithmetic.
    assert!(by("inf") == f64::INFINITY, "positive infinity was altered");
    assert!(
        by("negInf") == f64::NEG_INFINITY,
        "negative infinity was altered"
    );
}

/// A batch is one statement, not one per flake.
///
/// Measured through `pg_stat_database.xact_commit`: each `execute` runs in its
/// own implicit transaction, so a thousand separate inserts would show a
/// thousand commits. Asserting on the count of *statements* would need
/// `pg_stat_statements`, which is not loaded by default; transactions are
/// observable everywhere and answer the same question.
#[tokio::test]
async fn a_thousand_flakes_are_written_in_one_statement() {
    let (store, _container) = store().await;

    let commits = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT xact_commit FROM pg_stat_database WHERE datname = current_database()",
        )
        .fetch_one(store.pool())
        .await
        .expect("stat read")
    };

    let flakes: Vec<Flake> = (0..1_000)
        .map(|i| {
            Flake::assert(
                Sid::dsc(format!("column-{i}")),
                Sid::dsc("ordinalPosition"),
                FlakeValue::Int(i),
                1,
            )
        })
        .collect();

    let before = commits().await;
    store.assert_flakes(&flakes).await.expect("write");
    let after = commits().await;

    assert_eq!(
        store
            .count(&TriplePattern {
                p: Some(Sid::dsc("ordinalPosition")),
                ..TriplePattern::default()
            })
            .await
            .expect("count"),
        1_000,
        "all thousand must actually land"
    );
    assert!(
        after - before < 10,
        "1000 flakes took {} transactions — the batch is not one statement",
        after - before
    );
}

/// Postgres carries its parameter count as an `int16`, so one statement binds
/// at most 65535 values — about 3,200 flakes at twenty columns each. A batch
/// past that is not slow, it is a hard driver error, and a wide table's
/// projection crosses the line easily.
///
/// The thousand-flake test above passes either way; only a batch bigger than
/// one statement can hold proves the write is chunked.
#[tokio::test]
async fn a_batch_larger_than_one_statement_can_hold_is_still_written_whole() {
    let (store, _container) = store().await;

    // Comfortably past 65535/20, and past twice it, so a chunking loop that
    // wrote only the first or last chunk is caught too.
    let count = 8_000;
    let flakes: Vec<Flake> = (0..count)
        .map(|i| {
            Flake::assert(
                Sid::dsc(format!("column-{i}")),
                Sid::dsc("ordinalPosition"),
                FlakeValue::Int(i),
                1,
            )
        })
        .collect();

    store
        .assert_flakes(&flakes)
        .await
        .expect("a batch past the bind-parameter ceiling must still be written");

    assert_eq!(
        store
            .count(&TriplePattern {
                p: Some(Sid::dsc("ordinalPosition")),
                ..TriplePattern::default()
            })
            .await
            .expect("count"),
        u64::try_from(count).expect("the batch size is positive"),
        "every flake in the batch must land, not just the first chunk"
    );

    // Spot-check both ends, which a loop that dropped a chunk boundary would
    // fail even when the total happened to come out right.
    for edge in [0, count - 1] {
        let found = store
            .query_pattern(&TriplePattern {
                s: Some(Sid::dsc(format!("column-{edge}"))),
                ..TriplePattern::default()
            })
            .await
            .expect("query");
        assert_eq!(found.len(), 1, "column-{edge} is missing");
        assert_eq!(found[0].o, FlakeValue::Int(edge));
    }
}

/// Chunking must not cost atomicity. All the flakes or none of them — a
/// half-written projection reconciles to a state no version of the entity was
/// ever in.
#[tokio::test]
async fn a_large_batch_commits_as_one_transaction() {
    let (store, _container) = store().await;

    let commits = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT xact_commit FROM pg_stat_database WHERE datname = current_database()",
        )
        .fetch_one(store.pool())
        .await
        .expect("stat read")
    };

    let flakes: Vec<Flake> = (0..8_000)
        .map(|i| {
            Flake::assert(
                Sid::dsc(format!("column-{i}")),
                Sid::dsc("ordinalPosition"),
                FlakeValue::Int(i),
                1,
            )
        })
        .collect();

    let before = commits().await;
    store.assert_flakes(&flakes).await.expect("write");
    let after = commits().await;

    assert!(
        after - before < 5,
        "8000 flakes spanned {} commits — chunking broke atomicity",
        after - before
    );
}

/// A count computed by a different path than the rows is a count that can
/// disagree with them, and the disagreement always surfaces somewhere far from
/// its cause.
#[tokio::test]
async fn count_agrees_with_the_rows_for_every_pattern_shape() {
    let (store, _container) = store().await;
    let other = Sid::dsc("table-neft");
    store
        .assert_flakes(&[
            flake("name", FlakeValue::String("upi_transactions".into()), 1),
            flake("deleted", FlakeValue::Boolean(false), 1),
            Flake::assert(
                other.clone(),
                Sid::dsc("name"),
                FlakeValue::String("neft_transactions".into()),
                1,
            ),
        ])
        .await
        .expect("write");

    let patterns = vec![
        TriplePattern::default(),
        TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        },
        TriplePattern {
            p: Some(Sid::dsc("name")),
            ..TriplePattern::default()
        },
        TriplePattern {
            p: Some(Sid::dsc("name")),
            o: Some(FlakeValue::String("neft_transactions".into())),
            ..TriplePattern::default()
        },
        TriplePattern {
            s: Some(other),
            p: Some(Sid::dsc("name")),
            ..TriplePattern::default()
        },
        TriplePattern {
            s: Some(Sid::dsc("nothing-matches-this")),
            ..TriplePattern::default()
        },
    ];

    for pattern in patterns {
        let rows = store.query_pattern(&pattern).await.expect("query").len();
        let counted = store.count(&pattern).await.expect("count");
        assert_eq!(
            counted, rows as u64,
            "count and rows disagree for {pattern:?}"
        );
    }
}

/// A projection that failed halfway and is retried must converge, not
/// duplicate. This is what makes Slice G's reconciler safe to run repeatedly.
#[tokio::test]
async fn asserting_the_same_flake_twice_at_the_same_time_is_idempotent() {
    let (store, _container) = store().await;
    let written = flake("name", FlakeValue::String("upi_transactions".into()), 1);

    store
        .assert_flakes(std::slice::from_ref(&written))
        .await
        .expect("first");
    store
        .assert_flakes(std::slice::from_ref(&written))
        .await
        .expect("second must not error");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count");
    assert_eq!(rows, 1, "the second assertion wrote a duplicate row");
    assert_eq!(all_for_subject(&store).await, vec![written]);
}

/// The *same fact* at a *different* `t` is a different row — that is what
/// makes history recoverable. Only re-asserting at an identical `t` collapses.
#[tokio::test]
async fn the_same_fact_at_a_later_time_is_a_new_row_not_a_duplicate() {
    let (store, _container) = store().await;
    let value = FlakeValue::String("upi_transactions".into());

    store
        .assert_flakes(&[flake("name", value.clone(), 1)])
        .await
        .expect("first");
    store
        .assert_flakes(&[flake("name", value.clone(), 2)])
        .await
        .expect("second");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count");
    assert_eq!(rows, 2, "history needs both rows");

    let current = all_for_subject(&store).await;
    assert_eq!(current.len(), 1, "but only the newest is current");
    assert_eq!(current[0].t, 2);
}

/// Every flake in one logical change shares a `t`, which is what makes "the
/// state after change N" well-defined. A clock that handed out the same number
/// twice would merge two changes into one indistinguishable state.
#[tokio::test]
async fn the_transaction_clock_is_monotonic_and_starts_above_zero() {
    let (store, _container) = store().await;

    let first = store.next_time().await.expect("clock");
    let second = store.next_time().await.expect("clock");
    let third = store.next_time().await.expect("clock");

    assert_eq!(
        first, 1,
        "t=0 must mean 'before anything happened', so the first reserved t is 1"
    );
    assert!(second > first && third > second, "{first} {second} {third}");
}

/// Concurrent callers must not receive the same `t`.
#[tokio::test]
async fn concurrent_callers_never_share_a_transaction_time() {
    let (store, _container) = store().await;
    let store = std::sync::Arc::new(store);

    let mut handles = Vec::new();
    for _ in 0..20 {
        let store = std::sync::Arc::clone(&store);
        handles.push(tokio::spawn(async move { store.next_time().await }));
    }
    let mut times = Vec::new();
    for handle in handles {
        times.push(handle.await.expect("join").expect("clock"));
    }

    let total = times.len();
    times.sort_unstable();
    times.dedup();
    assert_eq!(total, times.len(), "two callers shared a t: {times:?}");
}

/// Namespace 0 means "nobody set this". Writing it would put a row in the
/// graph that cannot be attributed to any vocabulary, and time-travel makes
/// that permanent rather than transient.
#[tokio::test]
async fn a_flake_with_an_uninitialized_namespace_is_refused() {
    let (store, _container) = store().await;
    let bad = Flake::assert(
        Sid::new(namespace::UNSET, "x"),
        Sid::dsc("name"),
        FlakeValue::String("x".into()),
        1,
    );

    store
        .assert_flakes(&[bad])
        .await
        .expect_err("namespace 0 must be refused");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count");
    assert_eq!(rows, 0, "nothing should have been written");
}

/// A named graph scopes provenance. `None` is the default graph, and asking
/// for it specifically must not return facts from named ones.
#[tokio::test]
async fn named_graphs_are_queryable_separately_from_the_default_graph() {
    let (store, _container) = store().await;
    let extraction = Sid::dsc("graph:extraction");
    store
        .assert_flakes(&[
            flake("name", FlakeValue::String("from-catalog".into()), 1),
            Flake {
                cx: Some(extraction.clone()),
                ..flake(
                    "description",
                    FlakeValue::String("from-a-document".into()),
                    1,
                )
            },
        ])
        .await
        .expect("write");

    let any = store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    assert_eq!(any.len(), 2, "an unbound graph matches both");

    let default_only = store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            cx: Some(None),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    assert_eq!(default_only.len(), 1, "got {default_only:?}");
    assert_eq!(default_only[0].p.id, "name");

    let extraction_only = store
        .query_pattern(&TriplePattern {
            s: Some(subject()),
            cx: Some(Some(extraction)),
            ..TriplePattern::default()
        })
        .await
        .expect("query");
    assert_eq!(extraction_only.len(), 1);
    assert_eq!(extraction_only[0].p.id, "description");
}

/// The same fact in two different graphs is two facts. Collapsing them would
/// let an unconfirmed extraction silently overwrite a catalog fact.
#[tokio::test]
async fn the_same_fact_in_two_graphs_stays_two_facts() {
    let (store, _container) = store().await;
    let value = FlakeValue::String("upi_transactions".into());
    store
        .assert_flakes(&[
            flake("name", value.clone(), 1),
            Flake {
                cx: Some(Sid::dsc("graph:extraction")),
                ..flake("name", value, 1)
            },
        ])
        .await
        .expect("write");

    assert_eq!(all_for_subject(&store).await.len(), 2);
}

#[tokio::test]
async fn an_empty_batch_writes_nothing_and_is_not_an_error() {
    let (store, _container) = store().await;
    store.assert_flakes(&[]).await.expect("empty is fine");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flakes")
        .fetch_one(store.pool())
        .await
        .expect("row count");
    assert_eq!(rows, 0);
}

/// A reference object is stored in its own columns so the OPST index can find
/// it. Querying by that reference is the reverse traversal Slice E depends on.
#[tokio::test]
async fn a_reference_object_is_findable_by_the_node_it_points_at() {
    let (store, _container) = store().await;
    let team = Sid::dsc("team-payments");
    store
        .assert_flakes(&[
            flake("owner", FlakeValue::Ref(team.clone()), 1),
            flake("name", FlakeValue::String("team-payments".into()), 1),
        ])
        .await
        .expect("write");

    let pointing_at_team = store
        .query_pattern(&TriplePattern {
            o: Some(FlakeValue::Ref(team)),
            ..TriplePattern::default()
        })
        .await
        .expect("query");

    assert_eq!(
        pointing_at_team.len(),
        1,
        "a Ref object and a String with the same text are different values"
    );
    assert_eq!(pointing_at_team[0].p.id, "owner");
}

/// The wall-clock → logical-`t` mapping that makes as-of queries askable.
///
/// Covered end-to-end by the server's time-travel tests, but those live in
/// another crate, so a mutation run scoped to this adapter never executes
/// them — and `time_at` was in fact returning a constant under mutation with
/// nothing here to notice.
#[tokio::test]
async fn time_at_resolves_the_newest_transaction_at_or_before_an_instant() {
    let (store, _container) = store().await;

    // Nothing has happened yet: the graph is younger than any question.
    assert_eq!(
        store
            .time_at(Utc.timestamp_opt(1_700_000_000, 0).unwrap())
            .await
            .expect("resolve"),
        None,
        "before any transaction there is no state to return"
    );

    let first = store.next_time().await.expect("clock");
    // Postgres records `at` with now(); a real gap keeps the two instants
    // distinguishable rather than colliding inside one clock tick.
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let between = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let second = store.next_time().await.expect("clock");

    assert_eq!(
        store.time_at(between).await.expect("resolve"),
        Some(first),
        "an instant between two transactions resolves to the earlier one"
    );

    let after_both = Utc::now() + chrono::Duration::seconds(1);
    assert_eq!(
        store.time_at(after_both).await.expect("resolve"),
        Some(second),
        "the newest at or before, not the oldest"
    );
    assert_ne!(first, second, "the clock must have advanced");
}

/// A transaction time must never exist without the instant it happened at, or
/// an as-of query can never resolve to it.
#[tokio::test]
async fn every_reserved_transaction_time_is_recorded_with_its_instant() {
    let (store, _container) = store().await;

    for _ in 0..5 {
        store.next_time().await.expect("clock");
    }

    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_transactions")
        .fetch_one(store.pool())
        .await
        .expect("count");
    assert_eq!(recorded, 5, "each next_time must leave a row behind");

    let clock: i64 = sqlx::query_scalar("SELECT t FROM graph_clock")
        .fetch_one(store.pool())
        .await
        .expect("clock row");
    let newest: i64 = sqlx::query_scalar("SELECT MAX(t) FROM graph_transactions")
        .fetch_one(store.pool())
        .await
        .expect("max");
    assert_eq!(clock, newest, "the clock and its record must not diverge");
}
