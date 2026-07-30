//! Epic 16 Slice C at the wire: a batch file is a job, not a request.
//!
//! The unit tests in `graph-owl-api` cover what a job *decides*. What can only
//! be checked here is that the upload streams in, the handle comes back before
//! the work is done, and a poll eventually settles — which is the whole shape
//! decision 2 asks for.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn upload(app: &axum::Router, content_type: &str, body: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/ingest/batch")
        .header("content-type", content_type)
        .body(Body::from(body.to_string()))
        .expect("request should build");
    read(app.clone().oneshot(request).await.expect("handled")).await
}

async fn send(app: &axum::Router, method: &str, uri: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request should build");
    read(app.clone().oneshot(request).await.expect("handled")).await
}

async fn read(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, parsed)
}

/// Poll until the job settles.
///
/// A fixed sleep would be either flaky or slow; this waits on the *state* and
/// gives up after a bound that is far above anything a handful of rows can take.
async fn settled(app: &axum::Router, id: &str) -> Value {
    for _ in 0..200 {
        let (status, job) = send(app, "GET", &format!("/ingest/jobs/{id}")).await;
        assert_eq!(status, StatusCode::OK, "{job}");
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "partial" | "failed")
        ) {
            return job;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the job never settled");
}

/// **The shape of decision 2.** The response is a handle, not a result: a
/// 500k-row file cannot be answered synchronously, and a `200` here would be
/// claiming an outcome nobody has yet.
#[tokio::test]
async fn a_jsonl_upload_returns_a_handle_and_settles_as_succeeded() {
    let (app, _db, _) = test_app().await;

    let (status, accepted) = upload(
        &app,
        "application/x-ndjson",
        "{\"kind\":\"service\",\"name\":\"payments\"}\n\
         {\"kind\":\"database\",\"name\":\"core\",\"parentFqn\":\"payments\"}\n",
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let id = accepted["id"].as_str().expect("a job id").to_string();
    assert_eq!(accepted["state"], json!("queued"));
    assert_eq!(accepted["poll"], json!(format!("/ingest/jobs/{id}")));

    let job = settled(&app, &id).await;
    assert_eq!(job["state"], json!("succeeded"), "{job}");
    assert_eq!(job["accepted"], json!(2), "{job}");

    // The assets are really there — a job that reports success and wrote nothing
    // would pass every assertion above.
    let (found, assets) = send(&app, "GET", "/assets?limit=100").await;
    assert_eq!(found, StatusCode::OK);
    let fqns: Vec<&str> = assets["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|a| a["fullyQualifiedName"].as_str())
        .collect();
    assert!(fqns.contains(&"payments.core"), "{fqns:?}");
}

#[tokio::test]
async fn a_csv_upload_takes_its_field_names_from_the_header_row() {
    let (app, _db, _) = test_app().await;

    let (status, accepted) = upload(&app, "text/csv", "kind,name\nservice,billing\n").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");

    let job = settled(&app, accepted["id"].as_str().expect("id")).await;
    assert_eq!(job["state"], json!("succeeded"), "{job}");
    assert_eq!(job["accepted"], json!(1), "{job}");
}

/// One bad line must not cost the rest of the file, and the report has to name
/// the **line** — a client cannot read a 500k-row file by eye.
#[tokio::test]
async fn a_file_with_one_bad_line_is_partial_and_names_the_line() {
    let (app, _db, _) = test_app().await;

    let (_, accepted) = upload(
        &app,
        "application/x-ndjson",
        "{\"kind\":\"service\",\"name\":\"one\"}\n\
         not json at all\n\
         {\"kind\":\"service\",\"name\":\"two\"}\n",
    )
    .await;

    let job = settled(&app, accepted["id"].as_str().expect("id")).await;
    assert_eq!(job["state"], json!("partial"), "{job}");
    assert_eq!(job["accepted"], json!(2), "{job}");
    assert_eq!(job["failures"][0]["row"], json!(2), "{job}");
}

/// **Refused by name, not guessed at.** A columnar file fed to a line parser
/// reports every row as malformed, which buries "this build does not read
/// Parquet" under half a million parse errors.
#[tokio::test]
async fn a_format_this_build_cannot_read_is_refused_before_a_job_exists() {
    let (app, _db, _) = test_app().await;

    let (status, problem) = upload(&app, "application/vnd.apache.parquet", "not really").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    let rendered = problem.to_string();
    assert!(rendered.contains("Parquet"), "{rendered}");
    assert!(rendered.contains("x-ndjson"), "{rendered}");
}

#[tokio::test]
async fn polling_a_job_that_does_not_exist_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        "/ingest/jobs/00000000-0000-0000-0000-000000000000",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Cancelling something that already finished is **not** an error: a client that
/// cancels the instant a job completes has done nothing wrong, and the response
/// still has to say what landed.
#[tokio::test]
async fn cancelling_a_settled_job_returns_it_rather_than_failing() {
    let (app, _db, _) = test_app().await;

    let (_, accepted) = upload(
        &app,
        "application/x-ndjson",
        "{\"kind\":\"service\",\"name\":\"late\"}\n",
    )
    .await;
    let id = accepted["id"].as_str().expect("id").to_string();
    settled(&app, &id).await;

    let (status, job) = send(&app, "DELETE", &format!("/ingest/jobs/{id}")).await;

    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["state"], json!("succeeded"), "{job}");
    assert_eq!(job["cancelRequested"], json!(false), "{job}");
}

/// The upload never touches the catalog when it names an entity kind that does
/// not exist — the row is rejected, the file keeps going. Asserted here as well
/// as in the unit tests because the wire is where a client actually sees it.
#[tokio::test]
async fn an_unknown_kind_rejects_only_its_own_row() {
    let (app, _db, _) = test_app().await;

    let (_, accepted) = upload(
        &app,
        "text/csv",
        "kind,name\nnonsense,first\nservice,second\n",
    )
    .await;

    let job = settled(&app, accepted["id"].as_str().expect("id")).await;
    assert_eq!(job["state"], json!("partial"), "{job}");
    assert_eq!(job["accepted"], json!(1), "{job}");
    assert_eq!(job["failures"][0]["row"], json!(2), "{job}");
}
