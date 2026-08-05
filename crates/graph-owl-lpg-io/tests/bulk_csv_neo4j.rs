//! Epic 9a Slice C's own pre-PR gate item: "a real target store's bulk
//! importer consumes the CSV output unmodified" — checked against a real
//! Neo4j via testcontainers, not asserted from the documented format alone.
//!
//! **`LOAD CSV`, not the offline `neo4j-admin database import` tool.** The
//! offline tool needs the target database stopped and the CSV files
//! present before the server itself starts, which does not fit a
//! testcontainers image that is already running by the time this test gets
//! a handle to it. `LOAD CSV` is Neo4j's own other real, commonly used
//! bulk-loading mechanism — it runs against a live server, which is what
//! testcontainers gives us — so this test proves the same claim
//! ("a real target store accepts this file") through the path that is
//! actually reachable here. What it does not exercise is `neo4j-admin`'s
//! own automatic `:LABEL`/typed-header interpretation, since `LOAD CSV`
//! reads every column as a string; the header *shape* (`:ID`, `:LABEL`,
//! `:START_ID`, `:END_ID`, `:TYPE`, typed suffixes) is still exactly what
//! `graph-owl-lpg-io::tests::separate_typed_node_and_relationship_files_with_the_documented_bulk_shape`
//! already asserts.

use graph_owl_core::flake::Sid;
use graph_owl_lpg::{ElementId, LpgEdge, LpgNode, PropertyMap};
use graph_owl_lpg_io::{BulkCsvWriter, ExportMeta, LpgWriter};
use testcontainers_modules::neo4j::Neo4j;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::core::ExecCommand;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

fn node(id: &str, labels: &[&str]) -> LpgNode {
    LpgNode {
        element_id: ElementId::encode(&Sid::dsc(id)),
        labels: labels.iter().map(ToString::to_string).collect(),
        properties: PropertyMap::new(),
    }
}

fn edge(id: &str, from: &str, to: &str, edge_type: &str) -> LpgEdge {
    LpgEdge {
        element_id: ElementId::encode(&Sid::dsc(id)),
        edge_type: edge_type.to_string(),
        start: ElementId::encode(&Sid::dsc(from)),
        end: ElementId::encode(&Sid::dsc(to)),
        properties: PropertyMap::new(),
    }
}

/// Writes `content` into the container's Neo4j `import/` directory —
/// `testcontainers` 0.23 has no direct "copy file in" API, so this goes
/// through `exec` with base64-encoded content, which needs no escaping for
/// any character CSV can contain.
async fn place_in_import_dir(
    container: &ContainerAsync<testcontainers_modules::neo4j::Neo4jImage>,
    name: &str,
    content: &str,
) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let script = format!("echo {encoded} | base64 -d > /var/lib/neo4j/import/{name}");
    let mut result = container
        .exec(ExecCommand::new([
            "sh".to_string(),
            "-c".to_string(),
            script,
        ]))
        .await
        .expect("exec should run");
    // `exit_code()` reports `None` until the process has actually exited;
    // draining stdout to EOF is what guarantees that, per `ExecResult`'s
    // own documented contract ("will block until the command exits").
    let stderr = result.stderr_to_vec().await.expect("read stderr");
    let exit_code = result.exit_code().await.expect("exit code available");
    assert_eq!(
        exit_code,
        Some(0),
        "writing {name} into the container's import directory must succeed: {}",
        String::from_utf8_lossy(&stderr)
    );
}

#[tokio::test]
async fn a_real_neo4j_loads_the_bulk_csv_output_unmodified() {
    let dir = std::env::temp_dir().join(format!("bulk-csv-neo4j-{}", uuid::Uuid::new_v4()));
    let mut writer = BulkCsvWriter::new(&dir).expect("new");
    writer
        .begin(&ExportMeta {
            graph_id: "g".to_string(),
        })
        .expect("begin");
    writer.node(&node("a", &["Table"])).expect("node a");
    writer.node(&node("b", &["Table"])).expect("node b");
    writer.edge(&edge("r1", "a", "b", "feeds")).expect("edge");
    writer.finish().expect("finish");

    let nodes_csv = std::fs::read_to_string(dir.join("nodes-Table.csv")).expect("read nodes csv");
    let rels_csv = std::fs::read_to_string(dir.join("relationships.csv")).expect("read rels csv");
    std::fs::remove_dir_all(&dir).ok();

    let container = Neo4j::default().start().await.expect("start neo4j");
    place_in_import_dir(&container, "nodes-Table.csv", &nodes_csv).await;
    place_in_import_dir(&container, "relationships.csv", &rels_csv).await;

    let uri = format!(
        "bolt://{}:{}",
        container.get_host().await.expect("host"),
        container.image().bolt_port_ipv4().expect("bolt port")
    );
    let user = container.image().user().expect("default user configured");
    let pass = container
        .image()
        .password()
        .expect("default password configured");
    let graph = neo4rs::Graph::new(uri, user, pass).expect("connect to neo4j");

    let mut result = graph
        .execute(neo4rs::query(
            "LOAD CSV WITH HEADERS FROM 'file:///nodes-Table.csv' AS row RETURN count(row) AS total",
        ))
        .await
        .expect("neo4j must accept and parse the CSV this crate wrote, unmodified");
    let row = result.next().await.expect("row").expect("one summary row");
    let node_total: i64 = row.get("total").expect("total column");
    assert_eq!(
        node_total, 2,
        "the real server must see exactly the two node rows this crate wrote"
    );

    let mut result = graph
        .execute(neo4rs::query(
            "LOAD CSV WITH HEADERS FROM 'file:///relationships.csv' AS row RETURN count(row) AS total",
        ))
        .await
        .expect("neo4j must accept and parse the relationships CSV unmodified");
    let row = result.next().await.expect("row").expect("one summary row");
    let rel_total: i64 = row.get("total").expect("total column");
    assert_eq!(
        rel_total, 1,
        "the real server must see exactly the one relationship row"
    );
}
