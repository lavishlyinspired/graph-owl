//! One-directional projection to an external property-graph store — Epic
//! 9a Slice D.
//!
//! **graph-owl → external, never back** (decision 2). The target is a
//! projection, not a backend (decision 3): nothing in graph-owl's own
//! query path may read from it, or a drift between the two becomes a
//! second source of truth graph-owl cannot detect. That invariant is
//! checked structurally — see
//! `graph_owl_lpg_io::projection::tests::no_query_crate_references_a_projection_target`
//! — rather than merely stated, since the moment it silently stops being
//! true is exactly the moment nobody would notice.

use graph_owl_lpg::{LpgEdge, LpgNode};

/// One batch to project. `retracted` exists now (rather than being added in
/// Slice E) because the target-facing shape should not change between
/// "projecting" and "projecting incrementally" — only *how often* `project`
/// is called, and with how small a batch, changes when Slice E adds
/// checkpoint-scoped incremental sends.
#[derive(Debug, Clone, Default)]
pub struct ElementBatch {
    pub nodes: Vec<LpgNode>,
    pub edges: Vec<LpgEdge>,
    pub retracted: Vec<graph_owl_lpg::ElementId>,
}

/// What a successful [`GraphProjectionTarget::project`] call actually
/// wrote — the same "never indistinguishable from success" reasoning
/// `ImportOutcome` (Epic 9 Slice E) already applies on the RDF side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectionAck {
    pub nodes_written: u64,
    pub edges_written: u64,
    pub retracted: u64,
}

/// The last transaction time successfully projected — what makes an
/// incremental projection (Slice E) resume rather than restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub last_projected_t: i64,
}

/// What [`GraphProjectionTarget::reset`] clears. `graph_id` names the same
/// scoping concept [`graph_owl_lpg::LpgNode`]'s own projection uses
/// elsewhere in this crate's family — `None` means the target's entire
/// graph-owl-projected estate.
#[derive(Debug, Clone, Default)]
pub struct ProjectionScope {
    pub graph_id: Option<String>,
}

/// Why a write to the external store failed.
#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("could not connect to the projection target: {0}")]
    Connection(String),
    #[error("write to the projection target failed: {0}")]
    Write(String),
}

/// A property-graph store graph-owl projects into — never reads from
/// (decision 3). One direction, one trait, so a Bolt-speaking store and a
/// Redis-protocol store (`FalkorDB`, Slice D's own plan names it
/// separately) can share every caller-facing behaviour while differing
/// entirely in transport.
#[async_trait::async_trait]
pub trait GraphProjectionTarget: Send + Sync {
    /// Writes `batch`, idempotently — projecting the same batch twice must
    /// yield one copy, not two.
    ///
    /// # Errors
    /// [`TargetError`] if the target rejects or cannot be reached for the
    /// write.
    async fn project(&self, batch: &ElementBatch) -> Result<ProjectionAck, TargetError>;

    /// The last transaction time this target has confirmed it projected.
    ///
    /// # Errors
    /// [`TargetError`] if the target cannot be reached.
    async fn checkpoint(&self) -> Result<Checkpoint, TargetError>;

    /// Records `t` as the last successfully projected transaction time —
    /// part of the trait (not only a concrete type's own inherent method,
    /// which is where Slice D first put it) because Slice E's incremental
    /// driver calls this over `&dyn GraphProjectionTarget`, generic across
    /// whichever target is configured. **The caller advances the
    /// checkpoint only after `project` has returned successfully** — that
    /// ordering, not anything in this method itself, is what makes a
    /// mid-batch failure leave a consistent checkpoint rather than one
    /// that has skipped past unwritten data.
    ///
    /// # Errors
    /// [`TargetError`] if the target cannot record the value.
    async fn advance_checkpoint(&self, t: i64) -> Result<(), TargetError>;

    /// Clears everything in `scope` from the target.
    ///
    /// # Errors
    /// [`TargetError`] if the target cannot be reached.
    async fn reset(&self, scope: &ProjectionScope) -> Result<(), TargetError>;
}

#[cfg(feature = "bolt-target")]
mod neo4j_target {
    use super::{
        Checkpoint, ElementBatch, GraphProjectionTarget, ProjectionAck, ProjectionScope,
        TargetError,
    };
    use graph_owl_lpg::PropertyValue;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// The reserved label + property a checkpoint is stored under, *inside*
    /// the target graph itself — the target has no other durable state
    /// graph-owl controls, and a checkpoint that lived only in this
    /// process's memory would reset to "nothing projected yet" on every
    /// restart, defeating Slice E's whole reason for existing.
    const CHECKPOINT_LABEL: &str = "__GraphOwlCheckpoint";

    /// [`GraphProjectionTarget`] for a Bolt-speaking store — Epic 9a Slice
    /// D. Batched `UNWIND` writes, never one round trip per element;
    /// `MERGE` keyed on the element id, never `CREATE`, so re-projecting a
    /// batch after a partial failure converges instead of duplicating.
    pub struct Neo4jProjectionTarget {
        graph: neo4rs::Graph,
        /// Tracked in-process too, purely as a fast local read for
        /// [`checkpoint`](GraphProjectionTarget::checkpoint) — the target's
        /// own stored value (read fresh on `connect`) remains the value
        /// that survives a restart; this is not a second source of truth,
        /// only a cache of the first.
        last_projected_t: AtomicI64,
    }

    impl Neo4jProjectionTarget {
        /// Connects, and ensures the target's own schema (a uniqueness
        /// constraint on the element id) exists — **idempotently**: `IF NOT
        /// EXISTS` is Neo4j's own guarantee that issuing this on every
        /// connect converges on exactly one constraint rather than erroring
        /// or accumulating duplicates.
        ///
        /// # Errors
        /// [`TargetError::Connection`] if the target cannot be reached or
        /// its schema cannot be ensured.
        pub async fn connect(uri: &str, user: &str, pass: &str) -> Result<Self, TargetError> {
            let graph = neo4rs::Graph::new(uri, user, pass)
                .map_err(|e| TargetError::Connection(e.to_string()))?;
            graph
                .run(neo4rs::query(
                    "CREATE CONSTRAINT graph_owl_element_id IF NOT EXISTS \
                     FOR (n:GraphOwlElement) REQUIRE n.id IS UNIQUE",
                ))
                .await
                .map_err(|e| TargetError::Connection(e.to_string()))?;

            let last_projected_t = read_checkpoint(&graph).await?;
            Ok(Self {
                graph,
                last_projected_t: AtomicI64::new(last_projected_t),
            })
        }
    }

    async fn read_checkpoint(graph: &neo4rs::Graph) -> Result<i64, TargetError> {
        let mut result = graph
            .execute(neo4rs::query(&format!(
                "MATCH (c:{CHECKPOINT_LABEL}) RETURN c.t AS t"
            )))
            .await
            .map_err(|e| TargetError::Connection(e.to_string()))?;
        match result
            .next()
            .await
            .map_err(|e| TargetError::Connection(e.to_string()))?
        {
            Some(row) => row
                .get::<i64>("t")
                .map_err(|e| TargetError::Connection(e.to_string())),
            None => Ok(0),
        }
    }

    fn property_value_literal(value: &PropertyValue) -> neo4rs::BoltType {
        match value {
            PropertyValue::Boolean(b) => neo4rs::BoltType::Boolean(neo4rs::BoltBoolean::new(*b)),
            PropertyValue::Integer(i) => neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(*i)),
            PropertyValue::Float(f) => neo4rs::BoltType::Float(neo4rs::BoltFloat::new(*f)),
            PropertyValue::List(items) => neo4rs::BoltType::List(neo4rs::BoltList::from(
                items.iter().map(property_value_literal).collect::<Vec<_>>(),
            )),
            other => neo4rs::BoltType::String(neo4rs::BoltString::new(
                &super::super::property_text(other),
            )),
        }
    }

    fn node_map(node: &graph_owl_lpg::LpgNode) -> neo4rs::BoltMap {
        let mut map = neo4rs::BoltMap::new();
        map.put(
            "id".into(),
            neo4rs::BoltType::String(neo4rs::BoltString::new(node.element_id.as_str())),
        );
        map.put(
            "labels".into(),
            neo4rs::BoltType::String(neo4rs::BoltString::new(&node.labels.join(":"))),
        );
        let mut props = neo4rs::BoltMap::new();
        for key in node.properties.keys() {
            if let Some(value) = node.properties.get(key) {
                props.put(key.clone().into(), property_value_literal(value));
            }
        }
        map.put("props".into(), neo4rs::BoltType::Map(props));
        map
    }

    fn edge_map(edge: &graph_owl_lpg::LpgEdge) -> neo4rs::BoltMap {
        let mut map = neo4rs::BoltMap::new();
        map.put(
            "start".into(),
            neo4rs::BoltType::String(neo4rs::BoltString::new(edge.start.as_str())),
        );
        map.put(
            "end".into(),
            neo4rs::BoltType::String(neo4rs::BoltString::new(edge.end.as_str())),
        );
        map.put(
            "type".into(),
            neo4rs::BoltType::String(neo4rs::BoltString::new(&edge.edge_type)),
        );
        let mut props = neo4rs::BoltMap::new();
        for key in edge.properties.keys() {
            if let Some(value) = edge.properties.get(key) {
                props.put(key.clone().into(), property_value_literal(value));
            }
        }
        map.put("props".into(), neo4rs::BoltType::Map(props));
        map
    }

    #[async_trait::async_trait]
    impl GraphProjectionTarget for Neo4jProjectionTarget {
        async fn project(&self, batch: &ElementBatch) -> Result<ProjectionAck, TargetError> {
            let mut ack = ProjectionAck::default();

            if !batch.nodes.is_empty() {
                let rows: Vec<neo4rs::BoltType> = batch
                    .nodes
                    .iter()
                    .map(|n| neo4rs::BoltType::Map(node_map(n)))
                    .collect();
                // Real per-node Neo4j labels (rather than `n.labels` as a
                // plain property) need `apoc.create.setLabels` — a label
                // list is not expressible as a plain `SET` target in
                // Cypher itself — which would make this target depend on
                // the APOC plugin being installed. Kept as a queryable
                // property instead: every projected node still carries
                // `:GraphOwlElement` for the uniqueness constraint and
                // `labels` for anyone filtering on the catalog's own kind.
                self.graph
                    .run(
                        neo4rs::query(
                            "UNWIND $rows AS row \
                             MERGE (n:GraphOwlElement {id: row.id}) \
                             SET n += row.props, n.labels = row.labels",
                        )
                        .param("rows", rows),
                    )
                    .await
                    .map_err(|e| TargetError::Write(e.to_string()))?;
                ack.nodes_written = batch.nodes.len() as u64;
            }

            if !batch.edges.is_empty() {
                let rows: Vec<neo4rs::BoltType> = batch
                    .edges
                    .iter()
                    .map(|e| neo4rs::BoltType::Map(edge_map(e)))
                    .collect();
                self.graph
                    .run(
                        neo4rs::query(
                            "UNWIND $rows AS row \
                             MATCH (a:GraphOwlElement {id: row.start}), (b:GraphOwlElement {id: row.end}) \
                             MERGE (a)-[r:GRAPH_OWL_EDGE {id: row.start + '->' + row.end + ':' + row.type}]->(b) \
                             SET r += row.props, r.type = row.type",
                        )
                        .param("rows", rows),
                    )
                    .await
                    .map_err(|e| TargetError::Write(e.to_string()))?;
                ack.edges_written = batch.edges.len() as u64;
            }

            if !batch.retracted.is_empty() {
                let ids: Vec<String> = batch
                    .retracted
                    .iter()
                    .map(|id| id.as_str().to_string())
                    .collect();
                self.graph
                    .run(
                        neo4rs::query(
                            "UNWIND $ids AS id \
                             MATCH (n:GraphOwlElement {id: id}) \
                             DETACH DELETE n",
                        )
                        .param("ids", ids),
                    )
                    .await
                    .map_err(|e| TargetError::Write(e.to_string()))?;
                ack.retracted = batch.retracted.len() as u64;
            }

            Ok(ack)
        }

        async fn checkpoint(&self) -> Result<Checkpoint, TargetError> {
            Ok(Checkpoint {
                last_projected_t: self.last_projected_t.load(Ordering::SeqCst),
            })
        }

        async fn advance_checkpoint(&self, t: i64) -> Result<(), TargetError> {
            self.graph
                .run(
                    neo4rs::query(&format!(
                        "MERGE (c:{CHECKPOINT_LABEL} {{singleton: true}}) SET c.t = $t"
                    ))
                    .param("t", t),
                )
                .await
                .map_err(|e| TargetError::Write(e.to_string()))?;
            self.last_projected_t.store(t, Ordering::SeqCst);
            Ok(())
        }

        async fn reset(&self, scope: &ProjectionScope) -> Result<(), TargetError> {
            let statement = if scope.graph_id.is_some() {
                // A `graph_id`-scoped reset needs the projection to have
                // carried that scope onto each element in the first place —
                // Slice E's own concern (the `_graph` carrier Epic 7c
                // defines). Slice D's own scope is "clear everything this
                // target holds", so a named scope narrower than that is
                // refused rather than silently treated as "clear all".
                return Err(TargetError::Write(
                    "scoped reset needs per-element graph_id, added when Slice E wires \
                     incremental projection through this target"
                        .to_string(),
                ));
            } else {
                "MATCH (n:GraphOwlElement) DETACH DELETE n"
            };
            self.graph
                .run(neo4rs::query(statement))
                .await
                .map_err(|e| TargetError::Write(e.to_string()))?;
            self.last_projected_t.store(0, Ordering::SeqCst);
            self.graph
                .run(neo4rs::query(&format!(
                    "MERGE (c:{CHECKPOINT_LABEL} {{singleton: true}}) SET c.t = 0"
                )))
                .await
                .map_err(|e| TargetError::Write(e.to_string()))?;
            Ok(())
        }
    }
}

#[cfg(feature = "bolt-target")]
pub use neo4j_target::Neo4jProjectionTarget;

#[cfg(test)]
mod tests {
    /// **Decision 3, checked structurally rather than merely stated**: the
    /// query-answering crates must never import this module's own types.
    /// Grep-based rather than a compile-time check, because the violation
    /// this guards against is a *future* PR wiring the target into a query
    /// path — something a type system cannot forbid on its own, since
    /// nothing stops a future author from writing exactly that code and
    /// having it compile perfectly.
    ///
    /// **One sanctioned, named exception**: Epic 9a Slice E's
    /// `Catalog::project_incremental` (`graph-owl-api`) is a deliberate,
    /// push-only consumer — it reads `graph-owl-api`'s own store and
    /// writes to the target, never the reverse. It is walled off with
    /// `// decision-3-exception: begin`/`: end` markers rather than
    /// exempting the whole crate, so a *different*, unmarked reference
    /// anywhere else in `graph-owl-api` still fails this test.
    /// `graph-owl-query` and `graph-owl-engine` get no such exception —
    /// there is no legitimate reason for either to reference a projection
    /// target at all.
    #[test]
    fn no_query_crate_references_a_projection_target() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for crate_name in ["graph-owl-query", "graph-owl-engine", "graph-owl-api"] {
            let src = workspace_root.join("crates").join(crate_name).join("src");
            if !src.exists() {
                continue;
            }
            for entry in walk_rs_files(&src) {
                let contents = std::fs::read_to_string(&entry).unwrap_or_default();
                let checked = if crate_name == "graph-owl-api" {
                    strip_decision_3_exceptions(&contents, &entry)
                } else {
                    contents
                };
                assert!(
                    !checked.contains("GraphProjectionTarget")
                        && !checked.contains("Neo4jProjectionTarget"),
                    "{} references a projection target outside a `decision-3-exception` \
                     block — decision 3 forbids the query path reading from a projection \
                     target",
                    entry.display()
                );
            }
        }
    }

    /// Removes text between paired `// decision-3-exception: begin`/`: end`
    /// marker lines before the caller checks for a forbidden reference.
    /// Panics on an unbalanced or nested marker pair, so a marker that
    /// stops matching its counterpart (a bad merge, a careless edit) fails
    /// loudly rather than silently exempting more of the file than
    /// intended.
    fn strip_decision_3_exceptions(contents: &str, path: &std::path::Path) -> String {
        const BEGIN: &str = "// decision-3-exception: begin";
        const END: &str = "// decision-3-exception: end";
        let mut result = String::new();
        let mut inside = false;
        for line in contents.lines() {
            if line.trim_start().starts_with(BEGIN) {
                assert!(
                    !inside,
                    "{}: nested decision-3-exception blocks",
                    path.display()
                );
                inside = true;
                continue;
            }
            if line.trim_start().starts_with(END) {
                assert!(
                    inside,
                    "{}: decision-3-exception end with no matching begin",
                    path.display()
                );
                inside = false;
                continue;
            }
            if inside {
                continue;
            }
            result.push_str(line);
            result.push('\n');
        }
        assert!(
            !inside,
            "{}: unterminated decision-3-exception block",
            path.display()
        );
        result
    }

    fn walk_rs_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return files;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk_rs_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files
    }
}
