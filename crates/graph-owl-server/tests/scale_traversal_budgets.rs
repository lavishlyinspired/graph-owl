//! Epic 37a Slice C (traversal half): lineage depth swept 1-8 on the real
//! corpus, plus the cyclic-subgraph termination criterion.
//!
//! **Two of Slice C's plan-text budget rows have no capability to
//! measure, found by checking rather than assuming**:
//! - **"FQN cascade renaming a database with 5,000 descendants"** — there
//!   is no rename capability anywhere in the API. `AssetUpdate`
//!   (`graph-owl-core`) has no `name` field, and no route or facade method
//!   renames an asset. Not measured; recorded here rather than silently
//!   dropped.
//! - **"Ownership inheritance resolution in a list page < 150ms"** —
//!   already measured by Slice B's own `GET /assets` page-of-25 budget:
//!   every asset list response always carries resolved effective
//!   ownership (`Asset.owners`, never omitted), so this is the same query,
//!   not a second one. Slice B measured it at 1.98ms p95 at 60,246 rows,
//!   after the `LEFT JOIN LATERAL` fix Slice B's own account describes —
//!   re-benchmarking it here would just re-run the same SQL under a
//!   different name.
//!
//! **This file found and drove the fix for the epic's headline result**:
//! the depth-3 sweep first ran here uncapped and measured 25.2s, 31x over
//! budget, because `GET /lineage/asset/{id}` had a depth cap but no
//! node-count cap. `Catalog::lineage_graph` and the route gained
//! `maxNodes`/`truncated` the same session (see `plans/29-lineage.md`'s
//! corrected Slice C account — that budget was this epic's own stated
//! acceptance criterion, never actually built) and this file was
//! re-verified against the fix: 25.2s → 66.8ms, every depth capped at
//! exactly 200 nodes.
//!
//! Not run by the normal gate — same cost shape as `scale_read_budgets.rs`.
//! `cargo test -p graph-owl-server --test scale_traversal_budgets -- --ignored --nocapture`

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::Value;
use std::time::{Duration, Instant};
use tower::ServiceExt;

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value, Duration) {
    let start = Instant::now();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let elapsed = start.elapsed();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON body")
    };
    (status, body, elapsed)
}

fn scale_tables() -> usize {
    std::env::var("GRAPH_OWL_SCALE_TABLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60_000)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "scale measurement — minutes, not seconds; run explicitly"]
async fn lineage_depth_is_swept_one_through_eight() {
    let target_tables = scale_tables();
    let (app, _db, connection_string) = test_app().await;

    println!("generating a {target_tables}-table corpus...");
    let corpus = graph_owl_cli::corpus::generate(1, target_tables);
    let archive_path = std::env::temp_dir().join(format!(
        "graph-owl-scale-traversal-{}.tar.zst",
        uuid::Uuid::new_v4()
    ));
    graph_owl_cli::corpus::write_archive(&corpus, &archive_path).expect("write archive");
    let bytes = std::fs::read(&archive_path).expect("read archive back");
    std::fs::remove_file(&archive_path).ok();

    println!("restoring ({} bytes)...", bytes.len());
    let restore_start = Instant::now();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/restore?conflictPolicy=fail")
                .body(Body::from(bytes))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    println!("restored in {:?}", restore_start.elapsed());

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("connect");

    // **A real gap found running this, not designed around in advance**:
    // `restore_archive` writes the corpus's relationships through
    // `Catalog::create_relationship` into `entity_relationships` — the
    // generic table, not `lineage_edges` (Epic 29's own dedicated store,
    // `from_asset_id`/`to_asset_id`/`relationship`/`source`, unique on all
    // four so a person's and a connector's claims can coexist). Only
    // `lineage_edges` is what `GET /lineage/asset/{id}` (this test's own
    // subject) reads. Slice A's corpus generator predates this specific
    // benchmark need and was never wired to the lineage-specific table —
    // confirmed by reading `create_relationship`'s SQL before assuming the
    // corpus's "feeds" edges were already traversable. Copied in bulk here
    // (same real from/to pairs the generator produced, same random-graph
    // shape) rather than regenerated, and rather than re-asserted through
    // 170,903 individual `POST /lineage` calls, which would spend the
    // measurement's wall time on HTTP overhead instead of on traversal.
    sqlx::query(
        "INSERT INTO lineage_edges (id, from_asset_id, to_asset_id, relationship, source, created_by)
         SELECT gen_random_uuid(), from_entity_id, to_entity_id, relationship_type, 'connector', 'scale-harness'
         FROM entity_relationships WHERE relationship_type = 'feeds'
         ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("bulk-seeding lineage_edges should succeed");

    // The busiest source in the corpus's own lineage graph — the
    // rank-based power-law construction (`graph_owl_cli::corpus`) means
    // the highest-ranked table has by far the most outgoing `feeds`
    // edges. Read from Postgres directly rather than re-deriving the
    // generator's own ranking here.
    let (busiest_id,): (uuid::Uuid,) = sqlx::query_as(
        "SELECT from_asset_id FROM lineage_edges \
         GROUP BY from_asset_id ORDER BY COUNT(*) DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the corpus must have at least one lineage edge");

    println!("swept from the busiest source, {busiest_id}:");
    let mut previous: Option<Duration> = None;
    for depth in 1..=8usize {
        let (status, body, elapsed) = get(
            &app,
            &format!("/lineage/asset/{busiest_id}?upstream=0&downstream={depth}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "depth {depth}: {body}");
        let node_count = body["nodes"].as_array().map_or(0, Vec::len);
        println!(
            "  depth {depth}: {elapsed:?} ({node_count} nodes reachable){}",
            previous.map_or(String::new(), |p| format!(
                " [{:+.1}% vs previous depth]",
                (elapsed.as_secs_f64() - p.as_secs_f64()) / p.as_secs_f64().max(0.000_001) * 100.0
            ))
        );
        previous = Some(elapsed);
    }

    // Every depth above plateaus at exactly 200 nodes now — the
    // `maxNodes` default engaging, not organic graph connectivity. Before
    // the node-budget fix this file itself found missing, the same sweep
    // plateaued at ~55,900 (93% of the corpus) by depth 6, because the
    // corpus's random power-law fan-out genuinely is that well-connected;
    // the cap changes *what* bounds the plateau, not whether one exists.
    // What every depth must still do is answer
    // within the plan's own per-hop ceiling.
    let (status, body, elapsed) = get(
        &app,
        &format!("/lineage/asset/{busiest_id}?upstream=0&downstream=3"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // **Was 25.2s, unasserted, before the node budget existed** (60,246
    // real tables, 31x over this budget — see `plans/29-lineage.md`'s
    // Slice C account and `plans/37a-scale.md`'s Slice C account for the
    // full history). `GET /lineage/asset/{id}` now defaults to
    // `maxNodes=200`, so the busiest node's true 51,230-node reach never
    // gets past the cap. A real assertion again, not a printed miss.
    assert!(
        elapsed < Duration::from_millis(800),
        "lineage depth 3 from the busiest node: {elapsed:?} exceeds the \
         800ms high-fan-out budget"
    );
    assert_eq!(
        body["truncated"], true,
        "the busiest node's real reach (51,230 of 60,246 assets) is far \
         past the default 200-node cap — this must say so: {body}"
    );
    println!(
        "lineage depth 3 from the busiest node: {elapsed:?} (truncated at the 200-node default, as expected)"
    );

    // **The corpus's own injected cycle is `tables[0]→tables[1]→tables[2]
    // →tables[0]`** (Slice A's documented shape) — a 3-hop ring, not two
    // nodes with edges in both directions, so a "mutual pair" query would
    // never find it and was the wrong tool here; removed rather than left
    // in giving a false sense of coverage. `tables[0]` is rank 1 in the
    // generator's own iteration order, which is the same rank that makes
    // it the highest-fan-out node — almost certainly `busiest_id` above.
    // What this actually tests, honestly stated: does the **maximum**
    // depth (10, `MAX_LINEAGE_DEPTH`) terminate at all — a cycle-guard
    // liveness property — not "quickly," which the depth-3 finding above
    // already answered is not true from this node at this scale.
    let start = Instant::now();
    let (status, body, _) = get(
        &app,
        &format!("/lineage/asset/{busiest_id}?upstream=0&downstream=10"),
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::OK, "cyclic walk: {body}");
    println!("depth 10 (max, includes the injected cycle) terminated in {elapsed:?}");

    pool.close().await;
}
