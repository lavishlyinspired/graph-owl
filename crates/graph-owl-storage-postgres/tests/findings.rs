//! Findings, against a real database — Epic 105 P5.
//!
//! The unit tests in `graph_owl_core::finding` prove construction refuses an
//! unreviewable finding. These prove the half only a database can: that the
//! table refuses it *too*, that a re-run does not double the queue, and that
//! a decision sticks.

mod common;

use graph_owl_core::finding::{Evidence, Finding, FindingStatus};
use graph_owl_storage::FindingStore;
use graph_owl_storage_postgres::PostgresStorage;

async fn store() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("storage should connect and migrate");
    (storage, database, connection_string)
}

fn evidence() -> Vec<Evidence> {
    vec![Evidence {
        subject: "1025:pr-INV-1003".to_string(),
        predicate: "1025:taxAmount".to_string(),
        value: "45000.00".to_string(),
    }]
}

fn missing_invoice() -> Finding {
    Finding::new(
        "gst",
        "gst:MissingInGstr2b",
        "1025:pr-INV-1003",
        "Claimed in the register, never filed by the supplier",
        "gst:Section16",
        evidence(),
    )
    .expect("a complete finding")
}

#[tokio::test]
async fn a_finding_is_recorded_and_read_back_whole() {
    let (storage, _db, _url) = store().await;

    let created = storage
        .record_finding(&missing_invoice())
        .await
        .expect("record");
    assert!(created, "a first recording creates a row");

    let found = storage.list_findings(None, None).await.expect("list");
    assert_eq!(found.len(), 1);
    let finding = &found[0];
    assert_eq!(finding.label, "gst:MissingInGstr2b");
    assert_eq!(finding.governed_by, "gst:Section16");
    assert_eq!(finding.status, FindingStatus::Pending);
    assert_eq!(
        finding.evidence,
        evidence(),
        "the evidence round-trips through JSONB whole — a reviewer follows \
         each triple back into the graph, so losing a field makes it unusable"
    );
}

#[tokio::test]
async fn re_recording_the_same_pending_finding_creates_nothing() {
    // **The idempotence that makes a re-runnable reconciliation safe.** A
    // reviewer working through a queue must not watch it double because a
    // scheduled run fired.
    let (storage, _db, _url) = store().await;

    assert!(
        storage
            .record_finding(&missing_invoice())
            .await
            .expect("first")
    );
    assert!(
        !storage
            .record_finding(&missing_invoice())
            .await
            .expect("second"),
        "the second recording reports that it created nothing"
    );

    assert_eq!(
        storage.list_findings(None, None).await.expect("list").len(),
        1
    );
}

#[tokio::test]
async fn a_different_subject_or_label_is_a_different_finding() {
    // The negative half of idempotence: a key that collapsed everything would
    // let one recorded finding silently suppress every later one.
    let (storage, _db, _url) = store().await;
    storage
        .record_finding(&missing_invoice())
        .await
        .expect("first");

    let other_subject = Finding::new(
        "gst",
        "gst:MissingInGstr2b",
        "1025:pr-INV-9999",
        "s",
        "gst:Section16",
        evidence(),
    )
    .expect("valid");
    let other_label = Finding::new(
        "gst",
        "gst:TaxAmountMismatch",
        "1025:pr-INV-1003",
        "s",
        "gst:Section16",
        evidence(),
    )
    .expect("valid");

    assert!(
        storage
            .record_finding(&other_subject)
            .await
            .expect("subject")
    );
    assert!(storage.record_finding(&other_label).await.expect("label"));
    assert_eq!(
        storage.list_findings(None, None).await.expect("list").len(),
        3
    );
}

#[tokio::test]
async fn a_dismissal_survives_the_next_scheduled_run() {
    // **The correction V60 exists for.** V59 keyed the index on
    // `(pack, label, subject)` and made it partial on `status = 'pending'`,
    // reasoning that a recurrence deserves to be seen again. Running the real
    // GST reconciliation twice around a decision showed what that means: a
    // finding dismissed with a reason came straight back on the next run over
    // *identical* data. A reviewer who dismisses something on Monday and sees
    // it unchanged on Tuesday stops reading the queue.
    let (storage, _db, _url) = store().await;
    let first = missing_invoice();
    storage.record_finding(&first).await.expect("record");
    storage
        .decide_finding(
            first.id,
            FindingStatus::Rejected,
            "asha",
            Some("supplier filed late"),
        )
        .await
        .expect("decide");

    assert!(
        !storage
            .record_finding(&missing_invoice())
            .await
            .expect("re-run"),
        "the same conclusion from the same facts is the one already decided"
    );
    assert_eq!(
        storage.list_findings(None, None).await.expect("list").len(),
        1
    );
}

#[tokio::test]
async fn a_dismissal_does_not_suppress_the_same_problem_on_changed_facts() {
    // The other half, and the reason the digest is in the key rather than the
    // dismissal simply being permanent: the amount moved, so the reviewer's
    // earlier judgement was about a different situation.
    let (storage, _db, _url) = store().await;
    let first = missing_invoice();
    storage.record_finding(&first).await.expect("record");
    storage
        .decide_finding(
            first.id,
            FindingStatus::Rejected,
            "asha",
            Some("supplier filed late"),
        )
        .await
        .expect("decide");

    let moved = Finding::new(
        "gst",
        "gst:MissingInGstr2b",
        "1025:pr-INV-1003",
        "Claimed in the register, never filed by the supplier",
        "gst:Section16",
        vec![Evidence {
            subject: "1025:pr-INV-1003".to_string(),
            predicate: "1025:taxAmount".to_string(),
            value: "61000.00".to_string(),
        }],
    )
    .expect("valid");

    assert!(
        storage.record_finding(&moved).await.expect("changed"),
        "changed evidence is a new situation the reviewer must see"
    );
    assert_eq!(
        storage.list_findings(None, None).await.expect("list").len(),
        2
    );
}

#[tokio::test]
async fn a_decision_is_recorded_with_who_and_why() {
    let (storage, _db, _url) = store().await;
    let finding = missing_invoice();
    storage.record_finding(&finding).await.expect("record");

    let decided = storage
        .decide_finding(
            finding.id,
            FindingStatus::Rejected,
            "asha",
            Some("the supplier filed in the next period"),
        )
        .await
        .expect("decide");

    assert!(decided);
    let stored = storage
        .list_findings(None, Some(FindingStatus::Rejected))
        .await
        .expect("list");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].decided_by.as_deref(), Some("asha"));
    assert_eq!(
        stored[0].reason.as_deref(),
        Some("the supplier filed in the next period"),
        "the reason is what lets the next run tell 'considered and dismissed' \
         from 'not yet seen'"
    );
}

#[tokio::test]
async fn deciding_a_finding_that_does_not_exist_reports_it_rather_than_failing() {
    // A stale console tab, not a backend fault.
    let (storage, _db, _url) = store().await;

    let decided = storage
        .decide_finding(uuid::Uuid::new_v4(), FindingStatus::Accepted, "asha", None)
        .await
        .expect("no error");

    assert!(!decided);
}

#[tokio::test]
async fn a_rejection_without_a_reason_is_refused_by_the_table() {
    // **The invariant lives in two places on purpose.** The application
    // refuses it, and so does the `CHECK` — a check that exists only in
    // application code is a check the next writer skips.
    let (storage, _db, _url) = store().await;
    let finding = missing_invoice();
    storage.record_finding(&finding).await.expect("record");

    let refused = storage
        .decide_finding(finding.id, FindingStatus::Rejected, "asha", None)
        .await;

    assert!(
        refused.is_err(),
        "a rejection with no reason must not land — the next run could not \
         then tell it from a finding nobody has seen"
    );
}

#[tokio::test]
async fn findings_are_filterable_by_pack_so_one_queue_serves_every_domain() {
    // The property the console depends on: one generic queue, scoped by
    // pack. Without it every domain would need its own screen, which is the
    // thing the pack mechanism exists to avoid.
    let (storage, _db, _url) = store().await;
    storage
        .record_finding(&missing_invoice())
        .await
        .expect("gst");
    storage
        .record_finding(
            &Finding::new(
                "hospitality",
                "hosp:DuplicateGuest",
                "1024:guest-1",
                "Two records for one person",
                "hosp:GuestRecordPolicy",
                evidence(),
            )
            .expect("valid"),
        )
        .await
        .expect("hospitality");

    let gst = storage.list_findings(Some("gst"), None).await.expect("gst");
    let hosp = storage
        .list_findings(Some("hospitality"), None)
        .await
        .expect("hospitality");

    assert_eq!(gst.len(), 1);
    assert_eq!(hosp.len(), 1);
    assert_eq!(gst[0].label, "gst:MissingInGstr2b");
    assert_eq!(hosp[0].label, "hosp:DuplicateGuest");
}
