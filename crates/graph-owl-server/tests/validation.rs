//! Epic 5 Slices C, D and E, at the HTTP surface.
//!
//! The validator and the shape reader are exhaustively unit-tested without a
//! database. What only an end-to-end pass can show is the part that is about
//! *the estate*: that a shape stated in the graph reaches the real facts, that
//! the queue is stored rather than recomputed, and that a pass writes nothing
//! back into the graph it validated.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use graph_owl_engine::TripleStore;
use serde_json::Value;
use tower::ServiceExt;

fn a(id: &str) -> Sid {
    Sid::dsc(id)
}
fn sh(term: &str) -> Sid {
    Sid::new(namespace::SHACL, term)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

/// **Demo 4's shape**, stated as triples: every regulatory table must have an
/// owner and a retention tag.
async fn seed_shape(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    let shapes_graph = Sid::dsc("graph:shapes");
    let in_shapes = |s: Sid, p: Sid, o: FlakeValue| Flake {
        s,
        p,
        o,
        cx: Some(shapes_graph.clone()),
        t,
        op: true,
    };

    let facts = vec![
        in_shapes(
            a("RegulatoryShape"),
            rdf_type(),
            FlakeValue::Ref(sh("NodeShape")),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("targetClass"),
            FlakeValue::Ref(a("RegulatoryTable")),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("message"),
            FlakeValue::String("a regulatory table needs an owner and a retention tag".into()),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("property"),
            FlakeValue::Ref(a("RegulatoryShape/owner")),
        ),
        in_shapes(
            a("RegulatoryShape/owner"),
            sh("path"),
            FlakeValue::Ref(a("owner")),
        ),
        in_shapes(
            a("RegulatoryShape/owner"),
            sh("minCount"),
            FlakeValue::Int(1),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("property"),
            FlakeValue::Ref(a("RegulatoryShape/retention")),
        ),
        in_shapes(
            a("RegulatoryShape/retention"),
            sh("path"),
            FlakeValue::Ref(a("tag")),
        ),
        in_shapes(
            a("RegulatoryShape/retention"),
            sh("minCount"),
            FlakeValue::Int(1),
        ),
    ];
    store.assert_flakes(&facts).await.expect("seed the shape");
}

/// One regulatory table with neither an owner nor a retention tag.
async fn seed_offender(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[Flake::assert(
            a("payments"),
            rdf_type(),
            FlakeValue::Ref(a("RegulatoryTable")),
            t,
        )])
        .await
        .expect("seed the table");
}

async fn run_validation(app: &axum::Router) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/runs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn report(app: &axum::Router, query: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/validation/report{query}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// **The demo, end to end.** A shape stated as triples finds the table that
/// breaks it, and the queue fills with something a steward can act on.
#[tokio::test]
async fn a_shape_stated_in_the_graph_fills_the_queue() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 1, "the shape must have been read: {run}");
    assert_eq!(run["refusedShapes"], 0, "{run}");
    assert_eq!(run["conforms"], false, "{run}");
    assert_eq!(run["violations"], 2, "an owner and a tag: {run}");

    let queue = report(&app, "").await;
    let rows = queue["data"].as_array().expect("data");
    assert_eq!(rows.len(), 2, "{queue}");
    assert!(
        rows.iter().all(|r| r["focusNode"] == "1:payments"),
        "{queue}"
    );
    assert!(
        rows.iter()
            .all(|r| r["message"] == "a regulatory table needs an owner and a retention tag"),
        "the shape's own message must reach the queue: {queue}"
    );
    // A `MinCount` failure suggests asserting the missing value — the queue is
    // only actionable if it says what to do.
    assert_eq!(rows[0]["suggestion"]["action"], "assertMissing", "{queue}");
}

/// And the negative: an estate that satisfies the shape empties the queue. A
/// pass that only ever adds rows is a queue nobody can finish.
#[tokio::test]
async fn fixing_the_data_clears_the_queue_on_the_next_pass() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;
    assert_eq!(report(&app, "").await["total"], 2);

    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[
            Flake::assert(a("payments"), a("owner"), FlakeValue::Ref(a("finance")), t),
            Flake::assert(
                a("payments"),
                a("tag"),
                FlakeValue::String("retain-7y".into()),
                t,
            ),
        ])
        .await
        .expect("fix the data");

    let run = run_validation(&app).await;

    assert_eq!(run["conforms"], true, "{run}");
    let queue = report(&app, "").await;
    assert_eq!(queue["total"], 0, "{queue}");
    // But the report still says *when* it ran. An empty queue that cannot
    // prove it is current is indistinguishable from one that never ran.
    assert!(
        queue["computedAtT"].as_i64().expect("computedAtT") > 0,
        "{queue}"
    );
}

/// **A pass writes nothing into the graph it validated.** Validation that
/// mutated the estate would make running it a decision rather than a diagnostic.
#[tokio::test]
async fn a_validation_pass_leaves_the_graph_untouched() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    let before = store
        .query_pattern(&TriplePattern::default())
        .await
        .expect("everything");
    run_validation(&app).await;
    let after = store
        .query_pattern(&TriplePattern::default())
        .await
        .expect("everything");

    assert_eq!(before.len(), after.len(), "a validation pass wrote a flake");
}

/// The queue is filterable, because a steward works one severity or one asset
/// at a time. A filter that returns everything is a filter that looks like it
/// worked.
#[tokio::test]
async fn the_queue_can_be_narrowed() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;

    let mine = report(&app, "?focusNode=1:payments").await;
    let theirs = report(&app, "?focusNode=1:something-else").await;
    let by_shape = report(&app, "?shape=1:RegulatoryShape").await;
    let other_shape = report(&app, "?shape=1:NoSuchShape").await;
    let violations = report(&app, "?severity=violation").await;
    let warnings = report(&app, "?severity=warning").await;

    assert_eq!(mine["total"], 2, "{mine}");
    assert_eq!(theirs["total"], 0, "{theirs}");
    assert_eq!(by_shape["total"], 2, "{by_shape}");
    assert_eq!(other_shape["total"], 0, "{other_shape}");
    assert_eq!(violations["total"], 2, "{violations}");
    assert_eq!(warnings["total"], 0, "{warnings}");
}

/// Paging, per `00d`. `total` counts the whole queue rather than the page, or
/// a client cannot tell how much work is left.
#[tokio::test]
async fn the_queue_is_paged_and_reports_the_whole_total() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;

    let first = report(&app, "?limit=1").await;
    let second = report(&app, "?limit=1&offset=1").await;

    assert_eq!(first["data"].as_array().expect("data").len(), 1);
    assert_eq!(first["total"], 2, "the total is the queue, not the page");
    assert_ne!(first["data"][0]["id"], second["data"][0]["id"]);
}

/// **A malformed shape does not stop the others.** An estate goes unvalidated
/// the moment one bad shape can veto the pass — and nobody notices, because the
/// report simply looks clean.
#[tokio::test]
async fn a_broken_shape_is_counted_without_stopping_the_pass() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    // A second shape with no target: unreadable, and on its own.
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[Flake {
            s: a("BrokenShape"),
            p: rdf_type(),
            o: FlakeValue::Ref(sh("NodeShape")),
            cx: Some(Sid::dsc("graph:shapes")),
            t,
            op: true,
        }])
        .await
        .expect("seed the broken shape");

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 1, "the good shape still ran: {run}");
    assert_eq!(run["refusedShapes"], 1, "and the bad one is counted: {run}");
    assert_eq!(run["violations"], 2, "{run}");
}

/// A shape lives in its own graph, so it is not itself an asset the catalog
/// validates — and validating the shapes graph would make `TableShape` a focus
/// node for `EnvelopeShape`.
#[tokio::test]
async fn shapes_are_not_themselves_validated() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;

    // A shape targeting *everything with a name* would catch the shape nodes
    // too, if they were in the default graph.
    let t = store.next_time().await.expect("a transaction time");
    let shapes_graph = Sid::dsc("graph:shapes");
    store
        .assert_flakes(&[
            Flake {
                s: a("EverythingShape"),
                p: rdf_type(),
                o: FlakeValue::Ref(sh("NodeShape")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape"),
                p: sh("targetSubjectsOf"),
                o: FlakeValue::Ref(sh("path")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape"),
                p: sh("property"),
                o: FlakeValue::Ref(a("EverythingShape/p")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape/p"),
                p: sh("path"),
                o: FlakeValue::Ref(a("owner")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape/p"),
                p: sh("minCount"),
                o: FlakeValue::Int(1),
                cx: Some(shapes_graph),
                t,
                op: true,
            },
        ])
        .await
        .expect("seed");

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 2, "{run}");
    assert_eq!(
        run["violations"], 0,
        "the property shapes in the shapes graph were validated as data: {run}"
    );
}

/// **The seed shapes ship and they run.** A rule the product includes and never
/// executes is worse than none: it reads as governance that is in place.
#[tokio::test]
async fn the_seeded_shapes_run_against_a_real_estate() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    let seeded = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/shapes/seed")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(seeded.status(), StatusCode::OK);
    let body = json_body(seeded).await;
    let flakes = body["flakes"].as_array().expect("flakes must be an array");
    assert!(!flakes.is_empty(), "the seed wrote nothing");
    assert!(
        flakes[0].get("s").is_some() && flakes[0].get("p").is_some(),
        "each entry must be a real flake, not just a count: {flakes:?}"
    );
    // Plan 126 Slice 4: a reader must be able to see *which* shapes those
    // 57 flakes are, and whether the estate already conforms to them —
    // not just a raw triple count.
    let details = body["shapeDetails"].as_array().expect("shapeDetails");
    assert_eq!(details.len(), 5, "{details:#?}");
    assert!(
        details.iter().any(|d| d["id"] == "1:TableShape"),
        "{details:#?}"
    );
    let table_shape = details
        .iter()
        .find(|d| d["id"] == "1:TableShape")
        .expect("TableShape");
    let constraints = table_shape["constraints"].as_array().expect("constraints");
    assert!(!constraints.is_empty(), "{table_shape:#?}");
    assert_eq!(body["conforms"], true, "an empty estate: {body}");
    assert_eq!(body["violations"], 0, "{body}");

    // A column with no parent table and an out-of-range confidence: two
    // different seed shapes, so a pass that only ran one would still look busy.
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[
            Flake::assert(a("orphan"), rdf_type(), FlakeValue::Ref(a("column")), t),
            Flake::assert(a("guess"), a("confidence"), FlakeValue::Float(1.5), t),
        ])
        .await
        .expect("seed the estate");

    let run = run_validation(&app).await;

    assert_eq!(
        run["refusedShapes"], 0,
        "a shipped shape did not compile: {run}"
    );
    assert_eq!(run["conforms"], false, "{run}");

    let queue = report(&app, "").await;
    let offenders: Vec<&str> = queue["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|row| row["focusNode"].as_str().expect("focusNode"))
        .collect();
    assert!(offenders.contains(&"1:orphan"), "{queue}");
    assert!(offenders.contains(&"1:guess"), "{queue}");
}

const REGULATORY_OWNER_SHAPE: &str = r#"
    @prefix sh: <http://www.w3.org/ns/shacl#> .
    @prefix dsc: <https://graph-owl.dev/ns/catalog#> .

    dsc:RegulatoryOwnerShape a sh:NodeShape ;
        sh:targetClass dsc:RegulatoryTable ;
        sh:property [ sh:path dsc:owner ; sh:minCount 1 ] .
"#;

async fn post_shapes(app: &axum::Router, path: &str, document: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "format": "turtle", "document": document }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// Plan 126 Slice 2: previewing must find the same violation a real pass
/// would, and must leave the graph exactly as it found it — checked here
/// through the real HTTP surface, against real Postgres, not only the
/// unit-level `RecordingGraph` fixture `graph-owl-api`'s own tests use.
#[tokio::test]
async fn previewing_a_shape_finds_a_real_violation_and_writes_nothing() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_offender(&store).await;

    let (status, body) =
        post_shapes(&app, "/validation/shapes/preview", REGULATORY_OWNER_SHAPE).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "checked", "{body}");
    assert_eq!(body["shapes"], 1, "{body}");
    assert_eq!(body["conforms"], false, "{body}");
    assert_eq!(body["violations"], 1, "{body}");

    // The sample itself, not only its count — a `describe_violation` that
    // silently returned nothing would still pass a count-only assertion.
    let sample = body["sample"].as_array().expect("sample must be an array");
    assert_eq!(sample.len(), 1, "{body}");
    assert_eq!(sample[0]["focusNode"], "1:payments", "{body}");
    assert_eq!(sample[0]["constraint"], "minCount", "{body}");
    assert_eq!(sample[0]["severity"], "violation", "{body}");

    // Nothing was actually committed: a real run against the estate still
    // finds zero shapes, because preview never touched the shapes graph.
    let run = run_validation(&app).await;
    assert_eq!(
        run["shapes"], 0,
        "a preview must never land in the shapes graph a real run reads: {run}"
    );
}

/// Malformed Turtle previewed against the real server reports its own
/// line, not a generic 400 — the same contract the unit-level test proves
/// against `RecordingGraph`, now through the real HTTP body-parsing path.
#[tokio::test]
async fn previewing_malformed_turtle_reports_a_syntax_error_with_its_own_line() {
    let (app, _database, _) = test_app().await;

    let (status, body) = post_shapes(
        &app,
        "/validation/shapes/preview",
        "<https://graph-owl.dev/ns/catalog#a> <https://graph-owl.dev/ns/catalog#b> \"unterminated\n",
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "syntaxError", "{body}");
    assert_eq!(body["line"], 1, "{body}");
}

/// Plan 126 Slice 3: committing closes the loop — the flakes land for
/// real, and the stored report (not just the import's own response)
/// reflects them on the next run.
#[tokio::test]
async fn importing_a_shape_commits_it_and_a_real_run_finds_it() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_offender(&store).await;

    let (status, body) =
        post_shapes(&app, "/validation/shapes/import", REGULATORY_OWNER_SHAPE).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "checked", "{body}");
    let flakes = body["flakes"].as_array().expect("flakes must be an array");
    assert!(!flakes.is_empty(), "import wrote nothing");

    let run = run_validation(&app).await;
    assert_eq!(
        run["shapes"], 1,
        "the imported shape must run for real: {run}"
    );
    assert_eq!(run["conforms"], false, "{run}");

    let queue = report(&app, "").await;
    let offenders: Vec<&str> = queue["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|row| row["focusNode"].as_str().expect("focusNode"))
        .collect();
    assert!(offenders.contains(&"1:payments"), "{queue}");
}

/// **A real, live-caught bug**: `preview_shapes` parses candidate Turtle and
/// never assigns it a transaction time, so it lands at whatever `oxttl`
/// defaults an unstamped [`Flake`] to (`t: 0`). Committing after the
/// built-in shapes are already seeded (real `t`, well above zero) means the
/// imported shape's own `t: 0` flakes land in Postgres correctly — proven
/// directly, `docker exec ... psql` shows the rows — but a subsequent real
/// `run_validation` never sees them: `shapes` stays at the built-in set's
/// count, not `+1`. An empty-database test (this file's other import test)
/// cannot reproduce it, because nothing else has claimed a higher `t` yet
/// for `t: 0` to fall behind. Ordering — seed *before* import — is what
/// makes this the regression test, not incidental setup.
#[tokio::test]
async fn importing_after_the_built_in_shapes_are_already_seeded_still_takes_effect() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_offender(&store).await;

    let seed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/shapes/seed")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(seed.status(), StatusCode::OK);

    let baseline = run_validation(&app).await;
    let baseline_shapes = baseline["shapes"].as_u64().expect("shapes");
    assert!(
        baseline_shapes > 0,
        "the built-in seed must have landed: {baseline}"
    );

    let (status, body) =
        post_shapes(&app, "/validation/shapes/import", REGULATORY_OWNER_SHAPE).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "checked", "{body}");

    let run = run_validation(&app).await;
    assert_eq!(
        run["shapes"],
        baseline_shapes + 1,
        "the imported shape must be counted alongside the already-seeded ones: {run}"
    );
}

/// Seeding twice is a no-op in meaning. A restart that re-imposed rules would
/// make removing one impossible.
#[tokio::test]
async fn seeding_twice_does_not_double_the_shapes() {
    let (app, _database, _) = test_app().await;
    let seed = || {
        app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/shapes/seed")
                .body(Body::empty())
                .expect("request should build"),
        )
    };

    seed().await.expect("first");
    seed().await.expect("second");

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 5, "the seed set doubled: {run}");
    assert_eq!(run["refusedShapes"], 0, "{run}");
}

async fn waive(app: &axum::Router, row: &Value, reason: &str) -> (StatusCode, Value) {
    let body = serde_json::json!({
        "shape": row["shape"],
        "focusNode": row["focusNode"],
        "path": row["path"],
        "constraint": row["constraint"],
        "reason": reason,
        "expiresAt": (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339(),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/waivers")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// **A waiver survives the next pass.** Findings are replaced wholesale and
/// every row gets a fresh id, so a waiver keyed on a row id works once and then
/// points at nothing — a failure that reads as the waiver having been
/// forgotten rather than as a design error.
#[tokio::test]
async fn a_waiver_survives_a_re_run_because_it_names_the_finding_not_the_row() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;

    let queue = report(&app, "").await;
    let row = &queue["data"][0];
    let original_id = row["id"].as_str().expect("id").to_string();
    let (status, _) = waive(&app, row, "accepted until the migration lands").await;
    assert_eq!(status, StatusCode::CREATED);

    run_validation(&app).await;

    let after = report(&app, "").await;
    let rows = after["data"].as_array().expect("data");
    // Marked, not hidden: the queue still shows it, so the acceptance stays
    // reviewable — including the fact that it will lapse.
    assert_eq!(rows.len(), 2, "{after}");
    let waived = rows
        .iter()
        .find(|r| !r["waiver"].is_null())
        .expect("one finding should carry its waiver");
    assert_ne!(
        waived["id"].as_str().expect("id"),
        original_id,
        "row regenerated"
    );
    assert_eq!(
        waived["waiver"]["reason"], "accepted until the migration lands",
        "{after}"
    );
    assert_eq!(waived["waiver"]["expired"], false, "{after}");
}

/// A waiver has to say why, and has to expire. Both are governance rules, and
/// both are refused at the API rather than silently accepted.
#[tokio::test]
async fn a_waiver_needs_a_reason_and_a_future_expiry() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;
    let queue = report(&app, "").await;
    let row = &queue["data"][0];

    let (blank, _) = waive(&app, row, "   ").await;
    assert_eq!(
        blank,
        StatusCode::BAD_REQUEST,
        "a blank reason was accepted"
    );

    let past = serde_json::json!({
        "shape": row["shape"],
        "focusNode": row["focusNode"],
        "path": row["path"],
        "constraint": row["constraint"],
        "reason": "accepted",
        "expiresAt": (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/waivers")
                .header("content-type", "application/json")
                .body(Body::from(past.to_string()))
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a past expiry accepts nothing and was accepted"
    );
}

/// One live waiver per finding, and revoking puts it back in play.
#[tokio::test]
async fn a_finding_cannot_be_waived_twice_and_a_waiver_can_be_withdrawn() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;
    let queue = report(&app, "").await;
    let row = &queue["data"][0];

    let (first, waiver) = waive(&app, row, "the live reason").await;
    assert_eq!(first, StatusCode::CREATED);
    let (second, problem) = waive(&app, row, "a competing reason").await;
    assert_eq!(
        second,
        StatusCode::CONFLICT,
        "a second waiver would hide which reason is live: {problem}"
    );

    let revoked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/validation/waivers/{}",
                    waiver["id"].as_str().expect("id")
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);

    let after = report(&app, "").await;
    assert!(
        after["data"]
            .as_array()
            .expect("data")
            .iter()
            .all(|r| r["waiver"].is_null()),
        "the waiver outlived its revocation: {after}"
    );
}

/// A `sh:sparql`/`sh:SPARQLConstraint` shape — the SHACL-SPARQL escape
/// hatch (Epic 96 Slice A). `Catalog::run_validation_as` is the only method
/// that evaluates it; `POST /validation/runs` must call that one, not the
/// plain `run_validation` that silently treats every SPARQL constraint as
/// satisfied. Fixture mirrors `graph_owl_api`'s own
/// `sparql_constraint_validation` unit tests.
async fn seed_no_owner_shape(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    const NO_OWNER_QUERY: &str = "SELECT $this WHERE { \
        FILTER NOT EXISTS { $this <https://graph-owl.dev/ns/catalog#owner> ?o } }";
    let t = store.next_time().await.expect("a transaction time");
    let shapes_graph = Sid::dsc("graph:shapes");
    let in_shapes = |s: Sid, p: Sid, o: FlakeValue| Flake {
        s,
        p,
        o,
        cx: Some(shapes_graph.clone()),
        t,
        op: true,
    };
    let facts = vec![
        in_shapes(
            a("NoOwnerShape"),
            rdf_type(),
            FlakeValue::Ref(sh("NodeShape")),
        ),
        in_shapes(
            a("NoOwnerShape"),
            sh("targetClass"),
            FlakeValue::Ref(a("Unowned")),
        ),
        in_shapes(
            a("NoOwnerShape"),
            sh("sparql"),
            FlakeValue::Ref(a("NoOwnerShape/constraint")),
        ),
        in_shapes(
            a("NoOwnerShape/constraint"),
            sh("select"),
            FlakeValue::String(NO_OWNER_QUERY.to_string()),
        ),
    ];
    store.assert_flakes(&facts).await.expect("seed the shape");
}

async fn seed_unowned_offender(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[Flake::assert(
            a("orphaned-table"),
            rdf_type(),
            FlakeValue::Ref(a("Unowned")),
            t,
        )])
        .await
        .expect("seed the table");
}

/// **Phase 1.1 of `plans/EPIC-COMPLETION-PLAN.md`**: `POST /validation/runs`
/// called `catalog.run_validation()`, which never evaluates `sh:sparql` at
/// all — a shape using it would report zero violations forever, regardless
/// of the data. Only `run_validation_as` evaluates it. This is the RED test
/// for that fix: it drives the real HTTP endpoint, not `Catalog` directly.
#[tokio::test]
async fn a_sparql_constraint_shape_is_actually_evaluated_through_the_http_endpoint() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_no_owner_shape(&store).await;
    seed_unowned_offender(&store).await;

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 1, "{run}");
    assert_eq!(run["conforms"], false, "{run}");
    assert_eq!(
        run["violations"], 1,
        "the SPARQL constraint must have actually run against the seeded offender: {run}"
    );

    let queue = report(&app, "").await;
    let rows = queue["data"].as_array().expect("data");
    assert_eq!(rows.len(), 1, "{queue}");
    assert_eq!(rows[0]["focusNode"], "1:orphaned-table", "{queue}");
    assert_eq!(rows[0]["constraint"], "sparql", "{queue}");
}
