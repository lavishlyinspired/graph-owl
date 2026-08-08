//! Epic 31 Slices A and B against a real Postgres.
//!
//! Everything here is a claim about the *adapter and the schema* — link
//! resolution across two foreign-key columns, supersession as an atomic pair,
//! and the constraints that hold when nothing in Rust is looking. The pure
//! decisions (anchoring, validation, ranking, contradiction detection) are unit
//! tested in `graph-owl-core` and are deliberately not re-tested here.

mod common;

use chrono::Utc;
use graph_owl_core::contradiction::{Review, Verdict};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::memory::{Authorship, LinkRelation, Memory, MemoryKind, MemoryLink};
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{
    ConflictKind, MemoryWrite, Storage, StorageError, StoredUser, SupersedeOutcome,
};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

/// The author every fixture cites.
///
/// Seeded rather than assumed: `memories.author_user_id` is a real foreign key,
/// and the first run of this suite proved it by refusing every write — which is
/// the constraint working, not the test being awkward. A human author exists so
/// that somebody can be *asked*, and a dangling name cannot be asked.
const AUTHOR: &str = "sakshi";

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    reviewer(&storage, AUTHOR).await;
    (storage, database, connection_string)
}

/// One table to hang memories off. Memories are about *something*, so every test
/// here needs a real asset id — which is the point of the foreign key.
async fn subject(storage: &PostgresStorage, name: &str) -> Uuid {
    let now = Utc::now();
    let asset = Asset {
        id: Uuid::new_v4(),
        kind: AssetKind::Table,
        name: name.to_string(),
        fully_qualified_name: format!("warehouse.public.{name}"),
        parent_id: None,
        description: None,
        properties: None,
        extension: None,
        owners: Vec::new(),
        version: EntityVersion::initial(),
        updated_by: "system".to_string(),
        change_description: None,
        deleted: false,
        deleted_at: None,
        created_at: now,
        updated_at: now,
        lifecycle: Default::default(),
        deprecation: None,
    };
    storage.upsert_asset(asset).await.expect("asset").id
}

async fn reviewer(storage: &PostgresStorage, id: &str) -> String {
    storage
        .upsert_user(&StoredUser {
            id: id.to_string(),
            display_name: id.to_string(),
            email: None,
            is_admin: false,
            is_bot: false,
            roles: vec![],
        })
        .await
        .expect("user");
    id.to_string()
}

fn about(target: Uuid) -> MemoryLink {
    MemoryLink {
        relation: LinkRelation::About,
        target,
    }
}

fn memory(kind: MemoryKind, content: &str, links: Vec<MemoryLink>) -> Memory {
    Memory::new(
        kind,
        content.to_string(),
        Authorship::Human {
            user_id: AUTHOR.to_string(),
        },
        None,
        links,
        Utc::now(),
    )
    .expect("a memory the domain accepts")
}

#[tokio::test]
async fn a_memory_round_trips_with_its_links() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let other = subject(&storage, "revenue_mart").await;
    let written = memory(
        MemoryKind::Rationale,
        "Refunds are excluded from revenue from 2025 onward.",
        vec![
            about(table),
            MemoryLink {
                relation: LinkRelation::Affects,
                target: other,
            },
        ],
    );

    assert_eq!(
        storage.save_memory(&written).await.expect("save"),
        MemoryWrite::Saved
    );
    let read = storage
        .find_memory(written.id)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(read.content, written.content);
    assert_eq!(read.kind, written.kind);
    assert_eq!(read.authorship, written.authorship);
    assert!((read.confidence - written.confidence).abs() < f64::EPSILON);
    assert_eq!(read.links.len(), 2);
    assert_eq!(read.anchors(), vec![table]);
}

/// **`MemoryKind::Investigation` round-trips through the real `CHECK`
/// constraint.** Epic 32's `record_investigation` was always meant to write
/// this kind (`plans/32-agent-capabilities.md`), but the variant — and the
/// migration widening `memories_kind_check` to admit it — did not exist until
/// Phase 3 item 3.6. Nothing in `graph-owl-core`'s own unit tests can catch a
/// missing `CHECK` entry; only a real Postgres write can.
#[tokio::test]
async fn an_investigation_memory_round_trips_through_the_check_constraint() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let written = memory(
        MemoryKind::Investigation,
        "The nightly load silently drops late-arriving rows past the 10-minute window.",
        vec![about(table)],
    );

    assert_eq!(
        storage.save_memory(&written).await.expect("save"),
        MemoryWrite::Saved
    );
    let read = storage
        .find_memory(written.id)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(read.kind, MemoryKind::Investigation);
}

// The two authorship shapes take different columns and a `CHECK` that ties them
// to the discriminant, so both have to be proven to survive a round trip. An
// agent read back as a human would be the exact relabelling the domain refuses.
#[tokio::test]
async fn an_agent_authored_memory_keeps_its_agent_and_model() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let written = Memory::new(
        MemoryKind::Incident,
        "The nightly load double-counted refunds.".to_string(),
        Authorship::Agent {
            agent_id: "lineage-explainer".to_string(),
            model: "claude-opus-5".to_string(),
        },
        Some(0.6),
        vec![about(table)],
        Utc::now(),
    )
    .expect("memory");

    storage.save_memory(&written).await.expect("save");
    let read = storage
        .find_memory(written.id)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(
        read.authorship,
        Authorship::Agent {
            agent_id: "lineage-explainer".to_string(),
            model: "claude-opus-5".to_string(),
        }
    );
    assert!((read.confidence - 0.6).abs() < f64::EPSILON);
}

// Slice A: "a link to a nonexistent target → 400 naming the index." The index is
// the whole point — "one of your links is wrong" is not actionable with four of
// them.
#[tokio::test]
async fn an_unresolvable_link_names_which_one_it_was() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let ghost = Uuid::new_v4();
    let written = memory(
        MemoryKind::Caveat,
        "Read this before trusting the totals.",
        vec![
            about(table),
            MemoryLink {
                relation: LinkRelation::Evidence,
                target: ghost,
            },
        ],
    );

    let outcome = storage.save_memory(&written).await.expect("no hard error");

    assert_eq!(
        outcome,
        MemoryWrite::UnknownLinkTarget {
            index: 1,
            target: ghost
        }
    );
}

// **And nothing is left behind.** A rejected write that stored the row and not
// the links would leave an unanchored memory — stored, permanently
// unretrievable, holding the id the client was told failed.
#[tokio::test]
async fn a_rejected_link_leaves_no_memory_behind() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let written = memory(
        MemoryKind::Caveat,
        "Read this first.",
        vec![
            about(table),
            MemoryLink {
                relation: LinkRelation::Evidence,
                target: Uuid::new_v4(),
            },
        ],
    );

    storage.save_memory(&written).await.expect("no hard error");

    assert!(
        storage
            .find_memory(written.id)
            .await
            .expect("read")
            .is_none()
    );
}

// A link may point at another *memory* — `Follows` and `Contradicts` both do —
// so the adapter has to resolve into the second foreign-key column. A design
// that only checked `assets` would reject every memory chain.
#[tokio::test]
async fn a_link_to_another_memory_resolves() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let first = memory(
        MemoryKind::Decision,
        "We exclude refunds.",
        vec![about(table)],
    );
    storage.save_memory(&first).await.expect("save");

    let second = memory(
        MemoryKind::Rationale,
        "Because the finance close needs gross.",
        vec![
            about(table),
            MemoryLink {
                relation: LinkRelation::Follows,
                target: first.id,
            },
        ],
    );

    assert_eq!(
        storage.save_memory(&second).await.expect("save"),
        MemoryWrite::Saved
    );
    let read = storage
        .find_memory(second.id)
        .await
        .expect("read")
        .expect("present");
    assert!(
        read.links
            .iter()
            .any(|edge| edge.relation == LinkRelation::Follows && edge.target == first.id)
    );
}

#[tokio::test]
async fn a_duplicate_id_is_a_conflict_naming_its_own_kind() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let written = memory(
        MemoryKind::Decision,
        "We exclude refunds.",
        vec![about(table)],
    );
    storage.save_memory(&written).await.expect("save");

    let err = storage.save_memory(&written).await.expect_err("conflict");

    assert!(
        matches!(
            err,
            StorageError::Conflict {
                kind: ConflictKind::MemoryExists,
                ..
            }
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn retrieval_finds_every_memory_linked_to_the_subject() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let elsewhere = subject(&storage, "unrelated").await;
    let anchored = memory(
        MemoryKind::Decision,
        "Refunds excluded.",
        vec![about(table)],
    );
    let mentioning = memory(
        MemoryKind::Incident,
        "The load failed; orders was named in the alert.",
        vec![
            about(elsewhere),
            MemoryLink {
                relation: LinkRelation::Mentions,
                target: table,
            },
        ],
    );
    let unrelated = memory(
        MemoryKind::Caveat,
        "Nothing to do with it.",
        vec![about(elsewhere)],
    );
    for one in [&anchored, &mentioning, &unrelated] {
        storage.save_memory(one).await.expect("save");
    }

    let found = storage.memories_about(table, false).await.expect("read");

    // **Weak links included.** Ranking is what decides that a `Mentions` memory
    // is less relevant; filtering here would hard-code a relevance decision into
    // a read, and the anchoring term would have nothing to distinguish.
    let ids: Vec<Uuid> = found.iter().map(|memory| memory.id).collect();
    assert!(ids.contains(&anchored.id));
    assert!(ids.contains(&mentioning.id));
    assert!(!ids.contains(&unrelated.id));
}

// Slice B: the original stays readable, both halves are set, and the default read
// returns only the current one.
#[tokio::test]
async fn a_correction_marks_both_halves_and_leaves_the_original_readable() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let original = memory(
        MemoryKind::Decision,
        "Refunds are included.",
        vec![about(table)],
    );
    storage.save_memory(&original).await.expect("save");
    let correction = memory(
        MemoryKind::Decision,
        "Refunds are excluded.",
        vec![about(table)],
    );

    let outcome = storage
        .supersede_memory(original.id, &correction)
        .await
        .expect("supersede");

    assert_eq!(outcome, SupersedeOutcome::Superseded);
    let before = storage
        .find_memory(original.id)
        .await
        .expect("read")
        .expect("the original is still readable");
    let after = storage
        .find_memory(correction.id)
        .await
        .expect("read")
        .expect("present");

    assert_eq!(before.superseded_by, Some(correction.id));
    assert_eq!(after.supersedes, Some(original.id));
    // The content of the original is untouched. Overwriting in place would
    // destroy the record of what people believed, which is most of the reason to
    // keep a record.
    assert_eq!(before.content, "Refunds are included.");
}

#[tokio::test]
async fn the_default_read_returns_only_the_current_memory() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let original = memory(
        MemoryKind::Decision,
        "Refunds are included.",
        vec![about(table)],
    );
    storage.save_memory(&original).await.expect("save");
    let correction = memory(
        MemoryKind::Decision,
        "Refunds are excluded.",
        vec![about(table)],
    );
    storage
        .supersede_memory(original.id, &correction)
        .await
        .expect("supersede");

    let current = storage.memories_about(table, false).await.expect("read");
    let history = storage.memories_about(table, true).await.expect("read");

    assert_eq!(
        current.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![correction.id]
    );
    assert_eq!(history.len(), 2);
}

// "A chain of three supersessions is traversable end to end."
#[tokio::test]
async fn a_three_deep_chain_is_traversable_from_either_end() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let first = memory(MemoryKind::Decision, "First belief.", vec![about(table)]);
    storage.save_memory(&first).await.expect("save");

    let mut chain = vec![first.id];
    let mut current = first.id;
    for text in ["Second belief.", "Third belief.", "Fourth belief."] {
        let next = memory(MemoryKind::Decision, text, vec![about(table)]);
        assert_eq!(
            storage
                .supersede_memory(current, &next)
                .await
                .expect("supersede"),
            SupersedeOutcome::Superseded
        );
        chain.push(next.id);
        current = next.id;
    }

    // Forwards, following `superseded_by` from the oldest.
    let mut walked = vec![first.id];
    let mut cursor = first.id;
    while let Some(next) = storage
        .find_memory(cursor)
        .await
        .expect("read")
        .expect("present")
        .superseded_by
    {
        walked.push(next);
        cursor = next;
    }
    assert_eq!(walked, chain);

    // And backwards, following `supersedes` from the newest — a chain that only
    // walks one way is a chain a reader cannot enter from the answer they were
    // given.
    let mut back = vec![current];
    let mut cursor = current;
    while let Some(previous) = storage
        .find_memory(cursor)
        .await
        .expect("read")
        .expect("present")
        .supersedes
    {
        back.push(previous);
        cursor = previous;
    }
    back.reverse();
    assert_eq!(back, chain);
}

// "Superseding an already-superseded memory → 409 pointing at the current one."
// Naming it is the requirement: a client with only "no" cannot retry correctly.
#[tokio::test]
async fn superseding_a_corrected_memory_names_the_current_one() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let original = memory(MemoryKind::Decision, "First.", vec![about(table)]);
    storage.save_memory(&original).await.expect("save");
    let correction = memory(MemoryKind::Decision, "Second.", vec![about(table)]);
    storage
        .supersede_memory(original.id, &correction)
        .await
        .expect("supersede");

    let late = memory(MemoryKind::Decision, "Also second.", vec![about(table)]);
    let outcome = storage
        .supersede_memory(original.id, &late)
        .await
        .expect("no hard error");

    assert_eq!(
        outcome,
        SupersedeOutcome::AlreadySuperseded {
            current: correction.id
        }
    );
    // And the losing correction was not written — a stored memory that supersedes
    // nothing is an orphan the queue would show as current.
    assert!(storage.find_memory(late.id).await.expect("read").is_none());
}

#[tokio::test]
async fn superseding_something_absent_is_not_found() {
    let (storage, _database, _url) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let correction = memory(MemoryKind::Decision, "Second.", vec![about(table)]);

    let outcome = storage
        .supersede_memory(Uuid::new_v4(), &correction)
        .await
        .expect("no hard error");

    assert_eq!(outcome, SupersedeOutcome::NotFound);
}

/// Two competing decisions and a reviewer, ready for a verdict.
async fn reviewable(storage: &PostgresStorage) -> (Uuid, Uuid, String) {
    let table = subject(storage, "orders").await;
    let who = reviewer(storage, "priya").await;
    let one = memory(MemoryKind::Decision, "First.", vec![about(table)]);
    let two = memory(MemoryKind::Decision, "Second.", vec![about(table)]);
    for m in [&one, &two] {
        storage.save_memory(m).await.expect("save");
    }
    (one.id, two.id, who)
}

// A verdict recorded in either order has to apply to the same pair. The schema
// enforces `a < b`, so this proves the adapter normalises *before* the CHECK sees
// it — otherwise a reviewer's click becomes a 500 half the time, depending on
// which id happens to sort first.
#[tokio::test]
async fn a_review_is_normalised_whichever_order_it_arrives_in() {
    let (storage, _database, _url) = test_storage().await;
    let (one, two, who) = reviewable(&storage).await;
    let (low, high) = if one < two { (one, two) } else { (two, one) };

    storage
        .review_contradiction(
            Review {
                a: high,
                b: low,
                verdict: Verdict::Dismissed,
            },
            &who,
            Some("different quarters"),
        )
        .await
        .expect("review");

    let stored = storage.contradiction_reviews().await.expect("read");

    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].a, low);
    assert_eq!(stored[0].b, high);
    assert_eq!(stored[0].verdict, Verdict::Dismissed);
}

// **Changing your mind is an update, not a second row.** One table with a verdict
// column is what makes that true; two tables would have made "confirmed *and*
// dismissed" representable and the change non-atomic.
#[tokio::test]
async fn a_reviewer_can_change_their_verdict() {
    let (storage, _database, _url) = test_storage().await;
    let (one, two, who) = reviewable(&storage).await;

    for verdict in [Verdict::Confirmed, Verdict::Dismissed] {
        storage
            .review_contradiction(
                Review {
                    a: one,
                    b: two,
                    verdict,
                },
                &who,
                None,
            )
            .await
            .expect("review");
    }

    let stored = storage.contradiction_reviews().await.expect("read");

    assert_eq!(stored.len(), 1, "a change of mind must not add a row");
    assert_eq!(stored[0].verdict, Verdict::Dismissed);
}

#[tokio::test]
async fn a_confirmed_verdict_round_trips() {
    let (storage, _database, _url) = test_storage().await;
    let (one, two, who) = reviewable(&storage).await;

    storage
        .review_contradiction(
            Review {
                a: one,
                b: two,
                verdict: Verdict::Confirmed,
            },
            &who,
            Some("both are live and they disagree"),
        )
        .await
        .expect("review");

    let stored = storage.contradiction_reviews().await.expect("read");

    assert_eq!(stored[0].verdict, Verdict::Confirmed);
}

// The reviewed memories are real foreign keys, so deleting one takes its verdict
// with it — a verdict about a pair where one half is gone is a queue item nobody
// can act on.
#[tokio::test]
async fn deleting_a_reviewed_memory_removes_its_verdict() {
    let (storage, _database, connection_string) = test_storage().await;
    let (one, two, who) = reviewable(&storage).await;
    storage
        .review_contradiction(
            Review {
                a: one,
                b: two,
                verdict: Verdict::Dismissed,
            },
            &who,
            None,
        )
        .await
        .expect("review");

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("pool");
    sqlx::query("DELETE FROM memories WHERE id = $1")
        .bind(one)
        .execute(&pool)
        .await
        .expect("delete");

    assert!(
        storage
            .contradiction_reviews()
            .await
            .expect("read")
            .is_empty()
    );
}

// The subject is a real foreign key, so deleting the asset takes its links with
// it — but **not the memory**. A memory whose subject was dropped is still
// somebody's knowledge, and `staleness` has a `SubjectUnknown` verdict precisely
// so it can be reported rather than deleted.
#[tokio::test]
async fn deleting_the_subject_removes_the_link_and_keeps_the_memory() {
    let (storage, _database, connection_string) = test_storage().await;
    let table = subject(&storage, "orders").await;
    let written = memory(
        MemoryKind::Decision,
        "Refunds excluded.",
        vec![about(table)],
    );
    storage.save_memory(&written).await.expect("save");

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("pool");
    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(table)
        .execute(&pool)
        .await
        .expect("delete");

    let read = storage
        .find_memory(written.id)
        .await
        .expect("read")
        .expect("the memory survives its subject");

    assert!(read.links.is_empty());
}
