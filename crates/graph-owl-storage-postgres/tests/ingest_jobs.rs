//! Epic 16 Slice C's reaper, against a real Postgres.
//!
//! The plan names this RED directly: "a crash test asserting the reaper marks
//! the job failed rather than leaving it `running` forever". It is the one part
//! of Slice C that cannot be tested in a pure function, because the whole
//! mechanism *is* the difference between two timestamps in the database.
//!
//! The failure it guards is the quiet one. A worker that dies mid-file leaves a
//! row saying `running`, and nothing ever contradicts it — so a client polls a
//! job that will never settle, and the only signal that anything is wrong is
//! that the answer never comes.

mod common;

use chrono::Utc;
use graph_owl_storage::{IngestJob, IngestProgress, RowFailure, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

fn running_job() -> IngestJob {
    IngestJob {
        id: Uuid::new_v4(),
        format: "jsonl".to_string(),
        state: "running".to_string(),
        rows_read: 0,
        accepted: 0,
        rejected: 0,
        failures: Vec::new(),
        halt_reason: None,
        cancel_requested: false,
        submitted_by: "system".to_string(),
        started_at: Utc::now(),
        heartbeat_at: Utc::now(),
        finished_at: None,
    }
}

/// Backdate a job's heartbeat, which is the only way to simulate a crash without
/// waiting: a crashed worker is precisely one that stopped writing this column.
async fn stop_reporting(connection_string: &str, id: Uuid, seconds_ago: i64) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("connect");
    sqlx::query("UPDATE ingest_jobs SET heartbeat_at = now() - ($2 || ' seconds')::interval WHERE id = $1")
        .bind(id)
        .bind(seconds_ago.to_string())
        .execute(&pool)
        .await
        .expect("backdate the heartbeat");
}

// **The crash test.** A job whose worker stopped reporting must not stay
// `running` — a client polling it would otherwise wait forever for an answer
// nothing is left alive to give.
#[tokio::test]
async fn a_job_that_stopped_reporting_is_failed_rather_than_left_running() {
    let (storage, _database, connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");
    stop_reporting(&connection_string, job.id, 600).await;

    let reaped = storage
        .reap_abandoned_ingest_jobs(300)
        .await
        .expect("reap");

    assert_eq!(reaped, 1);
    let after = storage
        .ingest_job(job.id)
        .await
        .expect("read")
        .expect("job still exists");
    assert_eq!(after.state, "failed");
    // The reason has to say *why*, not just that it failed. "Failed" alone sends
    // somebody looking for a bad row in a file whose rows were all fine.
    let reason = after.halt_reason.expect("a reaped job says why");
    assert!(
        reason.contains("stopped reporting") || reason.contains("abandoned"),
        "{reason}"
    );
    // And it has to be settled, or a client polling `finishedAt` still waits.
    assert!(after.finished_at.is_some());
}

// The negative half, and the one that makes the test worth having: a job that is
// merely *slow* must survive. A reaper that failed everything running would pass
// the test above and destroy every large job in production — which is exactly the
// mutant the plan's watch list is pointing at.
#[tokio::test]
async fn a_job_still_reporting_is_left_alone() {
    let (storage, _database, _connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");

    let reaped = storage
        .reap_abandoned_ingest_jobs(300)
        .await
        .expect("reap");

    assert_eq!(reaped, 0);
    let after = storage
        .ingest_job(job.id)
        .await
        .expect("read")
        .expect("job");
    assert_eq!(after.state, "running");
    assert!(after.finished_at.is_none());
}

// A heartbeat is what "still alive" means, so reporting progress has to move the
// job back out of reach of the reaper. If it did not, a job slower than the
// threshold would be reaped mid-write no matter how healthy it was.
#[tokio::test]
async fn reporting_progress_saves_a_job_from_the_reaper() {
    let (storage, _database, connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");
    stop_reporting(&connection_string, job.id, 600).await;

    let cancelled = storage
        .report_ingest_progress(
            job.id,
            IngestProgress {
                rows_read: 10,
                accepted: 9,
                rejected: 1,
            },
            &[RowFailure {
                row: 4,
                detail: "no `kind`".to_string(),
            }],
        )
        .await
        .expect("report");

    assert!(!cancelled, "nobody cancelled this job");
    assert_eq!(
        storage.reap_abandoned_ingest_jobs(300).await.expect("reap"),
        0,
        "a job that just reported progress is alive"
    );
    let after = storage
        .ingest_job(job.id)
        .await
        .expect("read")
        .expect("job");
    assert_eq!(after.rows_read, 10);
    assert_eq!(after.accepted, 9);
    // The failure is kept with its row number, not just counted — a client greps
    // their file with that number.
    assert_eq!(after.failures.len(), 1);
    assert_eq!(after.failures[0].row, 4);
}

// A finished job is not a candidate however old it is. The reaper's index is
// partial on `finished_at IS NULL` precisely so a year of settled jobs is not
// rescanned, and this asserts the semantics rather than the index.
#[tokio::test]
async fn a_settled_job_is_never_reaped_however_stale() {
    let (storage, _database, connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");
    storage
        .finish_ingest_job(job.id, "succeeded", None)
        .await
        .expect("finish");
    stop_reporting(&connection_string, job.id, 100_000).await;

    assert_eq!(
        storage.reap_abandoned_ingest_jobs(300).await.expect("reap"),
        0
    );
    let after = storage
        .ingest_job(job.id)
        .await
        .expect("read")
        .expect("job");
    assert_eq!(after.state, "succeeded", "a settled verdict is not rewritten");
}

// Cancelling is a request the worker honours, so it must not itself settle the
// job — the counts have to come from the worker, which is the only thing that
// knows what actually landed.
#[tokio::test]
async fn cancelling_records_the_request_without_settling_the_job() {
    let (storage, _database, _connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");

    assert!(storage.cancel_ingest_job(job.id).await.expect("cancel"));

    let after = storage
        .ingest_job(job.id)
        .await
        .expect("read")
        .expect("job");
    assert!(after.cancel_requested);
    assert_eq!(after.state, "running", "only the worker settles a job");
    // And the worker learns about it through the call it was already making.
    assert!(
        storage
            .report_ingest_progress(job.id, IngestProgress::default(), &[])
            .await
            .expect("report"),
        "the progress report is how a worker hears about cancellation"
    );
}

// Cancelling something already settled is `false` rather than an error: a client
// racing a job to the finish line has done nothing wrong.
#[tokio::test]
async fn cancelling_a_settled_job_reports_that_it_was_too_late() {
    let (storage, _database, _connection_string) = test_storage().await;
    let job = running_job();
    storage.create_ingest_job(&job).await.expect("create");
    storage
        .finish_ingest_job(job.id, "succeeded", None)
        .await
        .expect("finish");

    assert!(!storage.cancel_ingest_job(job.id).await.expect("cancel"));
}

#[tokio::test]
async fn polling_a_job_that_was_never_created_is_none() {
    let (storage, _database, _connection_string) = test_storage().await;

    assert!(
        storage
            .ingest_job(Uuid::new_v4())
            .await
            .expect("read")
            .is_none()
    );
}
