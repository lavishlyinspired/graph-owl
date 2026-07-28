use std::sync::Arc;

use chrono::{DateTime, Utc};
use graph_owl_authz::{
    AccessPredicate, DecisionCache, DecisionKey, MetadataOperation, Policy, Subject, compile,
};
use graph_owl_connectors::DeletionPlan;
use graph_owl_core::projection;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Principal, Relationship, Table, TableUpdate,
    envelope::{ChangeDescription, EntityVersion},
    fqn,
    page::{Page, PageRequest},
    relationship_type::{EntityKind, RelationshipType, is_legal},
};
use graph_owl_engine::TripleStore;
use graph_owl_events::{ChangeEvent, EventSink, EventSubject};
use graph_owl_storage::{ConflictKind, Storage, StorageError, StoredUser, UpdateOutcome};
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, Subgraph, TraversalEngine};
use serde::Deserialize;
use uuid::Uuid;

pub mod validation;
use validation::{
    FieldError, FieldErrorCode, FieldPath, ValidateBody, optional_string, require_non_empty_string,
};

#[derive(utoipa::ToSchema, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTable {
    pub name: String,
    pub fully_qualified_name: String,
    pub description: Option<String>,
}

impl ValidateBody for CreateTable {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("fullyQualifiedName"),
            &mut errors,
        );
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

#[derive(utoipa::ToSchema, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertAsset {
    pub kind: AssetKind,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

impl ValidateBody for UpsertAsset {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("kind"), &mut errors);
        if let Some(kind) = value.get("kind").and_then(serde_json::Value::as_str) {
            if AssetKind::parse(kind).is_err() {
                errors.push(FieldError::new(
                    "kind",
                    FieldErrorCode::Type,
                    format!(
                        "`{kind}` is not an asset kind; expected one of: {}",
                        AssetKind::ALL
                            .iter()
                            .map(|k| k.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
        require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

#[derive(utoipa::ToSchema, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRelationship {
    pub to_table_id: Uuid,
    pub relationship_type: String,
}

/// PATCH semantics: every field is optional, so absence is never an error.
/// But a field the client *did* send must still be usable — `name: ""` is a
/// request to blank a required value, not a no-op.
impl ValidateBody for TableUpdate {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value.get("name").is_some_and(|v| !v.is_null()) {
            require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        }
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

/// PATCH: absence is never an error. But a description the client *did* send
/// must be usable — a blank string is a request to clear a field, and explicit
/// null is how that is expressed.
impl ValidateBody for AssetUpdate {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|d| d.trim().is_empty())
        {
            errors.push(FieldError::new(
                "description",
                FieldErrorCode::Empty,
                "`description` must not be blank; send null to clear it",
            ));
        }
        errors
    }
}

impl ValidateBody for CreateRelationship {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("toTableId"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("relationshipType"),
            &mut errors,
        );
        errors
    }
}

/// One error taxonomy for the whole facade.
///
/// Replaces a per-operation error enum. Handlers now *map* a domain failure to
/// a status code rather than each deciding what a failure means, which is what
/// keeps a fifth endpoint from inventing a sixth notion of "not found".
#[derive(Debug)]
pub enum CatalogError {
    /// The addressed entity does not exist.
    NotFound,
    /// A uniqueness constraint rejected the write.
    Conflict {
        detail: String,
        existing_id: Option<Uuid>,
        kind: ConflictKind,
    },
    /// A field-level failure that got past boundary validation, or one that
    /// only the domain can detect.
    Validation(Vec<FieldError>),
    /// The `(from, type, to)` triple is not in the legality table. Distinct
    /// from `Validation` because the *shape* is fine and the *meaning* is not —
    /// a client fixes it by choosing a different relationship, not a different
    /// value.
    IllegalRelationship {
        from: EntityKind,
        relationship: RelationshipType,
        to: EntityKind,
    },
    /// The caller sent `If-Match` naming a version that is no longer current.
    /// Carries the current one, so a client can show what it was about to
    /// overwrite rather than only that it failed.
    PreconditionFailed {
        current: EntityVersion,
    },
    Storage(StorageError),
}

impl From<StorageError> for CatalogError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Conflict {
                detail,
                existing_id,
                kind,
            } => CatalogError::Conflict {
                detail,
                existing_id,
                kind,
            },
            StorageError::Unexpected(message) => {
                CatalogError::Storage(StorageError::Unexpected(message))
            }
        }
    }
}

fn subject_of(principal: &Principal) -> Subject {
    Subject {
        id: principal.id.clone(),
        roles: principal.roles.clone(),
        is_admin: principal.is_admin,
    }
}

/// How much of the graph one query may touch.
///
/// **Nothing adopted enforces a budget** (`00l`), so this is the project's own
/// and it is not optional: an unbounded query over a growing graph is an
/// outage waiting for the estate to get large enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparqlBudget {
    pub max_facts: usize,
}

impl Default for SparqlBudget {
    fn default() -> Self {
        // 50k facts is roughly a 5,000-asset estate fully projected — large
        // enough that no realistic catalog question is refused, small enough
        // that the materialised set stays well inside the memory budget in
        // `00a`. Raised deliberately per deployment, not per query, because a
        // caller who could raise their own budget does not have one.
        Self { max_facts: 50_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SparqlOutcome {
    /// One map per solution: variable name to its bound term, rendered.
    pub rows: Vec<std::collections::BTreeMap<String, String>>,
    pub facts_scanned: usize,
    /// The budget cut the fact set short, so the answer may be incomplete.
    /// **Always reported** — a truncated answer presented as complete is the
    /// failure mode this project refuses everywhere else.
    pub truncated: bool,
    /// The transaction time the answer was computed at, when one was asked for.
    pub as_of: Option<i64>,
}

/// Keep only the facts this principal may see, up to the budget.
///
/// A flake is visible when its subject is a visible asset. A **relationship**
/// node is not an asset, so it is visible only when *both* endpoints are —
/// otherwise the existence of an edge would disclose the existence of the
/// asset at its far end, which is precisely what the policy hid.
fn scope_facts(
    all: &[graph_owl_core::flake::Flake],
    visible: &std::collections::HashSet<String>,
    max_facts: usize,
) -> (Vec<graph_owl_core::flake::Flake>, bool) {
    use graph_owl_core::flake::FlakeValue;

    // Which relationship nodes have both endpoints visible. Computed first,
    // because a relationship's own flakes are spread across several rows and a
    // single pass would have to decide before seeing them all.
    //
    // Tracked as (endpoints seen, endpoints permitted) rather than a
    // from/to pair. The first version kept the two positions apart and then
    // required both to be permitted — which made the branch deciding *which*
    // position to record indistinguishable from its opposite, and a mutation
    // run said so. Direction does not matter to this question; only "did we
    // see both ends, and may the caller see each of them".
    let mut endpoints: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();
    for flake in all {
        if flake.p.id != "fromEntity" && flake.p.id != "toEntity" {
            continue;
        }
        let FlakeValue::Ref(target) = &flake.o else {
            continue;
        };
        let entry = endpoints.entry(flake.s.id.as_str()).or_insert((0, 0));
        entry.0 += 1;
        if visible.contains(&target.id) {
            entry.1 += 1;
        }
    }
    let visible_edges: std::collections::HashSet<&str> = endpoints
        .iter()
        // Both ends present *and* both permitted. Requiring `seen == 2` is what
        // stops a half-written projection — an edge with only one endpoint
        // recorded — from counting as fully permitted.
        .filter(|(_, (seen, permitted))| *seen == 2 && seen == permitted)
        .map(|(id, _)| *id)
        .collect();

    let permitted: Vec<_> = all
        .iter()
        .filter(|flake| {
            visible.contains(&flake.s.id) || visible_edges.contains(flake.s.id.as_str())
        })
        .cloned()
        .collect();

    let truncated = permitted.len() > max_facts;
    let mut kept = permitted;
    kept.truncate(max_facts);
    (kept, truncated)
}

fn collect(results: spareval::QueryResults<'_>) -> Vec<std::collections::BTreeMap<String, String>> {
    match results {
        spareval::QueryResults::Solutions(iter) => iter
            .filter_map(Result::ok)
            .map(|solution| {
                solution
                    .iter()
                    .map(|(var, term)| (var.as_str().to_string(), term.to_string()))
                    .collect()
            })
            .collect(),
        // An ASK is one row with one column, so a caller reading `rows` gets a
        // usable answer without branching on query form.
        spareval::QueryResults::Boolean(answer) => {
            vec![std::iter::once(("answer".to_string(), answer.to_string())).collect()]
        }
        spareval::QueryResults::Graph(_) => Vec::new(),
    }
}

/// The landing page's answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total: i64,
    pub by_kind: Vec<(AssetKind, i64)>,
    /// Assets carrying a non-empty description.
    pub described: i64,
    /// The denominator for `described`. Equal to `total`, carried separately so
    /// a future coverage metric over a narrower scope does not have to redefine
    /// what it is a fraction of.
    pub documented_total: i64,
    pub recently_changed: Vec<Asset>,
    /// `None` when no graph engine is configured — distinct from a graph of
    /// size zero, which is what a configured-but-empty projection looks like.
    pub graph: Option<GraphSize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSize {
    pub flakes: u64,
}

/// An asset as an event names it.
///
/// Carried in full so a subscriber never reads the entity back — one that did
/// would race the next write and index a version nobody told it about.
fn event_subject(asset: &Asset) -> EventSubject {
    EventSubject {
        kind: asset.kind,
        id: asset.id.to_string(),
        fqn: asset.fully_qualified_name.clone(),
    }
}

/// The fields a re-ingest can actually move.
///
/// Deliberately **not** the whole serialized entity. `updatedAt` is rewritten on
/// every upsert whether anything changed or not, so a whole-entity diff would
/// report a change for every asset on every nightly connector run — and a search
/// index told that everything changed reindexes everything, nightly.
///
/// The FQN is absent because it is the identity the upsert matched on: it cannot
/// differ between the two sides. `id` is absent for the same reason in reverse —
/// a connector supplies a fresh one each run and it is discarded on conflict, so
/// including it would make every re-ingest look changed.
fn syncable_fields(asset: &Asset) -> serde_json::Value {
    serde_json::json!({
        "name": asset.name,
        "description": asset.description,
        "parentId": asset.parent_id,
        "properties": asset.properties,
    })
}

#[derive(Clone)]
pub struct Catalog {
    storage: Arc<dyn Storage>,
    /// The graph view of what `storage` holds. Optional because the catalog is
    /// fully functional without it — that is decision 6 made structural rather
    /// than promised: if the projection were required, a graph outage would be
    /// a catalog outage.
    graph: Option<Arc<dyn TripleStore>>,
    /// Compiled authorization predicates. `Arc` because `Catalog` is cloned per
    /// request by axum's state extraction, and a cache cloned with it would be
    /// a fresh empty cache on every request — present, warm to nobody.
    decisions: Arc<DecisionCache>,
    /// The same backend seen through its traversal capability. Two fields
    /// rather than one combined trait, because storing flakes and walking them
    /// are genuinely separate contracts — a backend could reasonably implement
    /// one and not the other.
    traversal: Option<Arc<dyn TraversalEngine>>,
    /// Where committed changes are announced. Optional for the same reason
    /// `graph` is: a catalog with no subscriber is fully functional, and making
    /// the sink required would turn "nothing is listening" into an outage.
    events: Option<Arc<dyn EventSink>>,
}

impl Catalog {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            graph: None,
            traversal: None,
            events: None,
            decisions: Arc::new(DecisionCache::default()),
        }
    }

    /// The catalog, projecting into a graph as it writes.
    #[must_use]
    pub fn with_graph(mut self, graph: Arc<dyn TripleStore>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// Announce committed changes to `sink`.
    #[must_use]
    pub fn with_events(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.events = Some(sink);
        self
    }

    /// Announce a change **after** the write that caused it has returned.
    ///
    /// Every call site sits past an early return on failure, so a mutation that
    /// did not commit cannot reach here — the ordering is a property of where
    /// this is called, not of a flag it checks.
    fn announce(&self, event: Option<ChangeEvent>) {
        if let (Some(sink), Some(event)) = (self.events.as_ref(), event) {
            sink.emit(&event);
        }
    }

    /// The traversal capability of the same backend.
    #[must_use]
    pub fn with_traversal(mut self, traversal: Arc<dyn TraversalEngine>) -> Self {
        self.traversal = Some(traversal);
        self
    }

    /// The neighbourhood around an asset, as a graph.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist or the caller may not see it.
    /// `Storage` if no traversal engine is configured — answering a graph
    /// question with an empty graph would read as "nothing is connected",
    /// which is a wrong answer rather than a missing feature.
    pub async fn asset_subgraph(
        &self,
        principal: &Principal,
        id: Uuid,
        direction: Direction,
        bounds: Bounds,
        as_of: Option<DateTime<Utc>>,
    ) -> Result<Subgraph, CatalogError> {
        // Visibility first, and against relational state — decision 7. The
        // projection lags by design, so a permission revoked in that window
        // would still be honoured by a check that read from the graph.
        self.get_asset_for(principal, id).await?;

        let traversal = self.traversal.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no traversal engine configured".to_string(),
            ))
        })?;

        let as_of_t = match (as_of, &self.graph) {
            (Some(at), Some(graph)) => graph
                .time_at(at)
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?,
            _ => None,
        };

        traversal
            .subgraph(
                &[graph_owl_core::flake::Sid::new(
                    graph_owl_core::flake::namespace::DSC,
                    id.to_string(),
                )],
                direction,
                bounds,
                &EdgeFilter {
                    relationship_types: None,
                    as_of: as_of_t,
                },
            )
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))
    }

    /// Project a relationship into the graph, or withdraw it.
    ///
    /// Same failure isolation as [`project`]: an edge that fails to reach the
    /// graph leaves the relational row intact and the graph view stale.
    ///
    /// [`project`]: Self::project
    async fn project_relationship(&self, relationship: &Relationship, asserting: bool) {
        let Some(graph) = &self.graph else {
            return;
        };

        let outcome = async {
            let t = graph.next_time().await?;
            let flakes = projection::relationship_to_flakes(relationship, t);
            if asserting {
                graph.assert_flakes(&flakes).await
            } else {
                // Every flake of the edge, withdrawn together. Retracting only
                // the endpoints would leave an orphan node still carrying
                // `rdf:type dsc:Relationship` — an edge to nowhere, which a
                // traversal would count and then fail to follow.
                graph.retract_flakes(&flakes).await
            }
        }
        .await;

        if let Err(error) = outcome {
            eprintln!(
                "graph projection failed for relationship {} ({error}). The edge \
                 is intact; the graph view is stale until reconciliation.",
                relationship.id
            );
        }
    }

    /// Answer a SPARQL query over the graph, scoped to what this principal may
    /// see and to the transaction time asked for.
    ///
    /// **The ordering is the security property.** Visibility is resolved
    /// against *relational* state, the fact set is filtered before it is built,
    /// and only then does the evaluator run. The evaluator therefore never
    /// holds a fact the caller may not see, so no amount of optimisation inside
    /// it can surface one — decision 7 made structural rather than trusted.
    ///
    /// # Errors
    ///
    /// `Validation` if the query does not parse. `Storage` if no graph engine
    /// is configured, or if the scan fails.
    #[tracing::instrument(name = "catalog.sparql", skip_all)]
    pub async fn sparql(
        &self,
        principal: &Principal,
        query: &str,
        as_of: Option<DateTime<Utc>>,
        budget: SparqlBudget,
    ) -> Result<SparqlOutcome, CatalogError> {
        let parsed = spargebra::SparqlParser::new()
            .parse_query(query)
            .map_err(|error| {
                CatalogError::Validation(vec![FieldError::new(
                    "query",
                    FieldErrorCode::Type,
                    error.to_string(),
                )])
            })?;

        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        // 1. Who can see what — from relational, never from the projection.
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        let visible: std::collections::HashSet<String> = self
            .storage
            .list_assets_under_fqn("")
            .await?
            .into_iter()
            .filter(|asset| predicate.admits(&asset.fully_qualified_name))
            .map(|asset| asset.id.to_string())
            .collect();

        // 2. When.
        let at = match as_of {
            None => None,
            Some(instant) => graph
                .time_at(instant)
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?,
        };

        // 3. The facts, narrowed by the query and filtered before the
        //    evaluator exists.
        //
        //    Pushdown turns "read the estate and let the evaluator filter" into
        //    "read what the question names". `None` means the query could not
        //    be bounded — a property path reaches an unknown number of hops —
        //    and a full scan is then the only correct answer, never a guess.
        let scans = graph_owl_query::pushdown::scans_for(&parsed)
            .unwrap_or_else(|| vec![graph_owl_core::flake::TriplePattern::default()]);

        let mut all = Vec::new();
        for mut scan in scans {
            scan.as_of = at;
            all.extend(
                graph
                    .query_pattern(&scan)
                    .await
                    .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?,
            );
        }
        // Patterns overlap — `?s dsc:name ?n` and `?s ?p ?o` both return the
        // name flakes. A duplicate quad would make the evaluator emit a
        // solution twice, so the union has to be a set.
        all.sort_by(|a, b| (&a.s.id, &a.p.id, a.t).cmp(&(&b.s.id, &b.p.id, b.t)));
        all.dedup();

        let (facts, truncated) = scope_facts(&all, &visible, budget.max_facts);

        // 4. Evaluate.
        let dataset = graph_owl_query::dataset::FlakeDataset::from_flakes(&facts)
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        let results = spareval::QueryEvaluator::new()
            .prepare(&parsed)
            .execute(&dataset)
            .map_err(|e| {
                CatalogError::Validation(vec![FieldError::new(
                    "query",
                    FieldErrorCode::Type,
                    e.to_string(),
                )])
            })?;

        Ok(SparqlOutcome {
            rows: collect(results),
            facts_scanned: facts.len(),
            truncated,
            as_of: at,
        })
    }

    /// Assets the graph does not represent.
    ///
    /// Deliberately computed by **comparison, not from a queue**. A queue of
    /// failed projections is itself state that can be lost — by a crash
    /// between the failure and the enqueue, or by the queue's own storage
    /// failing — and a drift detector that can silently miss drift is worse
    /// than none, because it reports zero and is believed.
    ///
    /// Comparison cannot miss: relational is the source of truth, so anything
    /// present there and absent here is drift by definition, however it got
    /// that way.
    ///
    /// # Errors
    ///
    /// `Storage` if either side cannot be read.
    pub async fn projection_drift(&self) -> Result<Vec<Asset>, CatalogError> {
        let Some(graph) = &self.graph else {
            return Ok(Vec::new());
        };

        // Every asset the catalog holds, live or tombstoned: a tombstoned
        // asset still projects (with `dsc:deleted true`), so an unprojected
        // one is drift regardless of its state.
        let assets = self.storage.list_assets_under_fqn("").await?;

        let mut drifted = Vec::new();
        for asset in assets {
            let projected = graph
                .count(&graph_owl_core::flake::TriplePattern {
                    s: Some(projection::asset_sid(&asset)),
                    p: Some(graph_owl_core::flake::Sid::dsc("fqn")),
                    ..Default::default()
                })
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
            if projected == 0 {
                drifted.push(asset);
            }
        }
        Ok(drifted)
    }

    /// Re-project everything the graph is missing.
    ///
    /// **One-directional, structurally.** This reads relational and writes the
    /// graph; it has no path that writes relational, which is decision 1's
    /// invariant. A reconciler that could write back would let the graph view
    /// — which lags by design — overwrite the source of truth, and the two
    /// stores would then fight.
    ///
    /// Idempotent: re-asserting an identical fact at the same `t` is a no-op,
    /// so running this twice converges rather than duplicating.
    ///
    /// # Errors
    ///
    /// `Storage` if the scan fails. A failure to project one asset does not
    /// abort the rest — the point of reconciling is to make progress.
    pub async fn reconcile_projection(&self) -> Result<usize, CatalogError> {
        let drifted = self.projection_drift().await?;
        let mut repaired = 0;
        for asset in &drifted {
            // `None` before: an unprojected asset has nothing to diff against,
            // so this asserts its whole state rather than a change to it.
            self.project(None, asset).await;
            repaired += 1;
        }
        Ok(repaired)
    }

    /// The asset as it stood at a past instant.
    ///
    /// Reconstructed from the graph rather than read from a snapshot table:
    /// history recoverable *by construction* is the whole claim of the flake
    /// model, and a parallel snapshot table is exactly the thing that can
    /// drift from the facts it claims to summarise.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset did not exist at that instant — including when
    /// the graph is younger than the question. `Unexpected` if no graph is
    /// configured, because silently answering a time-travel question from
    /// current state would be a wrong answer rather than a missing feature.
    #[tracing::instrument(name = "catalog.get_asset_as_of", skip_all)]
    pub async fn get_asset_as_of(
        &self,
        id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<Asset, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let t = graph
            .time_at(at)
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?
            // Nothing had happened yet. Distinct from "the entity did not
            // exist", but indistinguishable to a caller asking about one id —
            // and both are honestly a 404 for that id at that instant.
            .ok_or(CatalogError::NotFound)?;

        let flakes = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(graph_owl_core::flake::Sid::new(
                    graph_owl_core::flake::namespace::DSC,
                    id.to_string(),
                )),
                as_of: Some(t),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        projection::asset_from_flakes(id, &flakes).ok_or(CatalogError::NotFound)
    }

    /// Project an asset's new state into the graph, after the relational write
    /// has already succeeded.
    ///
    /// **Never propagates a failure.** Decision 6: relational is the source of
    /// truth, and failing an entity write because its graph projection failed
    /// would make the graph a single point of failure for the catalog. The
    /// entity exists; the graph view catches up.
    ///
    /// `before` is read here rather than passed in, because the diff belongs
    /// to the projection and not to the write path — a caller that had to
    /// supply it would be doing the projection's bookkeeping for it, and would
    /// eventually forget to.
    async fn project(&self, before: Option<Asset>, after: &Asset) {
        let Some(graph) = &self.graph else {
            return;
        };

        let outcome = async {
            let t = graph.next_time().await?;
            let flakes = match &before {
                Some(before) => projection::asset_update_flakes(before, after, t),
                None => projection::asset_to_flakes(after, t),
            };
            // Retractions and assertions go through their own verbs; the flag
            // is not carried on the struct.
            let (retractions, assertions): (Vec<_>, Vec<_>) =
                flakes.into_iter().partition(|f| !f.op);
            graph.retract_flakes(&retractions).await?;
            graph.assert_flakes(&assertions).await
        }
        .await;

        if let Err(error) = outcome {
            // Logged, not returned. A silent failure here would be a drift bug
            // nobody could diagnose; a returned one would be decision 6
            // violated. Epic 4 Slice G turns this into a queued reconciliation.
            eprintln!(
                "graph projection failed for asset {} ({}): {error}. The entity \
                 is intact; the graph view is stale until reconciliation.",
                after.id, after.fully_qualified_name
            );
        }
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails, e.g. a duplicate `fully_qualified_name`.
    pub async fn create_table(
        &self,
        principal: &Principal,
        request: CreateTable,
    ) -> Result<Table, CatalogError> {
        // Epic 3 puts this on the envelope as `updated_by`. Until then the
        // principal is threaded and observable, so Epic 12 changes an extractor
        // rather than forty signatures.
        let _ = principal;
        let now = Utc::now();
        let table = Table {
            id: Uuid::new_v4(),
            name: request.name,
            fully_qualified_name: request.fully_qualified_name,
            description: request.description,
            created_at: now,
            updated_at: now,
        };
        Ok(self.storage.insert_table(table).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_table(&self, id: Uuid) -> Result<Option<Table>, CatalogError> {
        Ok(self.storage.get_table(id).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, CatalogError> {
        Ok(self.storage.list_tables(page).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn update_table(
        &self,
        principal: &Principal,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, CatalogError> {
        let _ = principal;
        Ok(self.storage.update_table(id, update).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_table(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<bool, CatalogError> {
        let _ = principal;
        Ok(self.storage.delete_table(id).await?)
    }

    /// # Errors
    ///
    /// Returns `CatalogError::Validation` if `relationshipType` is not in the
    /// vocabulary, `CatalogError::IllegalRelationship` if the triple is not in the
    /// legality table, `CatalogError::NotFound` if either table doesn't exist, or
    /// `CatalogError::Conflict` if storage rejects it (e.g. a duplicate
    /// relationship).
    pub async fn create_relationship(
        &self,
        principal: &Principal,
        from_table_id: Uuid,
        request: CreateRelationship,
    ) -> Result<Relationship, CatalogError> {
        let _ = principal;
        // Vocabulary and legality are checked *before* existence, deliberately:
        // an illegal triple between two nonexistent tables is a triple problem,
        // and reporting 404 would send the client hunting for the wrong bug.
        let relationship_type =
            RelationshipType::parse(&request.relationship_type).map_err(|unknown| {
                CatalogError::Validation(vec![FieldError::new(
                    "relationshipType",
                    FieldErrorCode::Type,
                    format!(
                        "`{}` is not a relationship type; expected one of: {}",
                        unknown.got,
                        RelationshipType::ALL
                            .iter()
                            .map(|r| r.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )])
            })?;

        let (from, to) = (EntityKind::Table, EntityKind::Table);
        if !is_legal(from, relationship_type, to) {
            return Err(CatalogError::IllegalRelationship {
                from,
                relationship: relationship_type,
                to,
            });
        }

        if self.storage.get_table(from_table_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        if self.storage.get_table(request.to_table_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }

        let relationship = Relationship {
            id: Uuid::new_v4(),
            from_entity_type: from.as_str().to_string(),
            from_entity_id: from_table_id,
            relationship_type: relationship_type.as_str().to_string(),
            to_entity_type: to.as_str().to_string(),
            to_entity_id: request.to_table_id,
            created_at: Utc::now(),
        };

        let created = self.storage.create_relationship(relationship).await?;
        self.project_relationship(&created, true).await;
        Ok(created)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails. Returns `Ok(None)` if the table
    /// itself doesn't exist.
    pub async fn list_relationships_for_table(
        &self,
        table_id: Uuid,
    ) -> Result<Option<Vec<Relationship>>, CatalogError> {
        if self.storage.get_table(table_id).await?.is_none() {
            return Ok(None);
        }

        let relationships = self
            .storage
            .list_relationships_for_entity("table", table_id)
            .await?;
        Ok(Some(relationships))
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_relationship(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<bool, CatalogError> {
        let _ = principal;
        // Read before deleting: a retraction has to name the exact facts it
        // withdraws, and after the row is gone there is nothing left to name
        // them from.
        let existing = self.storage.get_relationship(id).await.unwrap_or(None);
        let deleted = self.storage.delete_relationship(id).await?;
        if let (true, Some(relationship)) = (deleted, existing) {
            self.project_relationship(&relationship, false).await;
        }
        Ok(deleted)
    }

    // ---- asset hierarchy (Epic 2) ----

    /// Creates or converges an asset, deriving its FQN from the parent chain.
    ///
    /// # Errors
    ///
    /// `Validation` if the FQN cannot be derived or the parent is the wrong
    /// kind; `NotFound` if the parent does not exist.
    #[tracing::instrument(name = "catalog.upsert_asset", skip_all)]
    pub async fn upsert_asset(
        &self,
        principal: &Principal,
        request: UpsertAsset,
    ) -> Result<Asset, CatalogError> {
        let _ = principal;

        // Containment is checked against the *actual* parent, not a claim in
        // the request: a column under a schema is a hierarchy corruption every
        // later traversal has to cope with.
        let parent = match request.parent_id {
            Some(parent_id) => {
                let parent = self
                    .storage
                    .get_asset(parent_id)
                    .await?
                    .ok_or(CatalogError::NotFound)?;
                if request.kind.parent_kind() != Some(parent.kind) {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "parentId",
                        FieldErrorCode::Type,
                        format!(
                            "a `{}` is contained by a `{}`, not a `{}`",
                            request.kind,
                            request
                                .kind
                                .parent_kind()
                                .map_or_else(|| "nothing".to_string(), |k| k.to_string()),
                            parent.kind
                        ),
                    )]));
                }
                Some(parent)
            }
            None => {
                if request.kind.parent_kind().is_some() {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "parentId",
                        FieldErrorCode::Required,
                        format!("a `{}` requires a parent", request.kind),
                    )]));
                }
                None
            }
        };

        let fully_qualified_name = match &parent {
            Some(parent) => fqn::child_of(&parent.fully_qualified_name, &request.name),
            None => fqn::derive(&[&request.name]),
        }
        .map_err(|error| {
            CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Type,
                error.to_string(),
            )])
        })?;

        let now = Utc::now();
        // Read before the write so the projection can diff against it. A
        // create has no prior state and projects its whole self; an upsert
        // over an existing FQN is an update and must retract what it replaces.
        let before = self
            .storage
            .get_asset_by_fqn(&fully_qualified_name)
            .await
            .unwrap_or(None);

        let written = self
            .storage
            .upsert_asset(Asset {
                id: Uuid::new_v4(),
                kind: request.kind,
                name: request.name,
                fully_qualified_name,
                parent_id: request.parent_id,
                description: request.description,
                properties: request.properties,
                version: EntityVersion::initial(),
                updated_by: principal.id.clone(),
                // No diff on the initial version: there was nothing before it,
                // and an empty diff would read as "nothing changed" rather than
                // "this is where it began".
                change_description: None,
                deleted: false,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            })
            .await?;

        self.project(before.clone(), &written).await;
        // Past every early return above, so the write has committed.
        //
        // `upsert_asset` is create-or-update behind one method, and the caller
        // never says which it meant — a connector supplies a fresh Uuid on
        // every run and lets the FQN decide. Prior state is therefore the only
        // honest signal: no `before` is a creation, a `before` is an update.
        self.announce(match &before {
            None => Some(ChangeEvent::created(
                event_subject(&written),
                written.version,
                &principal.id,
            )),
            // Storage does not version or diff an upsert — a connector re-run
            // is a mechanical sync, not a curated edit (`03-versioning.md`).
            // The facade holds both states, so it computes the diff here, and
            // `ChangeEvent::updated` drops the event when nothing moved.
            Some(before) => ChangeEvent::updated(
                event_subject(&written),
                before.version,
                written.version,
                ChangeDescription::between(&syncable_fields(before), &syncable_fields(&written)),
                &principal.id,
            ),
        });

        Ok(written)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    #[tracing::instrument(name = "catalog.get_asset", skip_all)]
    pub async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, CatalogError> {
        Ok(self.storage.get_asset(id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, CatalogError> {
        Ok(self.storage.get_asset_by_fqn(fqn).await?)
    }

    /// Cheapest possible round trip to storage, for readiness.
    ///
    /// # Errors
    ///
    /// Returns an error if storage is unreachable.
    pub async fn ping(&self) -> Result<(), CatalogError> {
        self.storage.ping().await.map_err(Into::into)
    }

    /// Resolves a principal's policies once per request.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn policies_for(&self, principal: &Principal) -> Result<Vec<Policy>, CatalogError> {
        if principal.is_admin {
            return Ok(Vec::new());
        }
        Ok(self.storage.policies_for_roles(&principal.roles).await?)
    }

    /// The predicate for one principal and operation, cached.
    ///
    /// The cache sits **here** rather than around `policies_for`, because the
    /// compiled predicate is what every read path actually consumes and
    /// compiling it is the other half of the cost. Caching the policies alone
    /// would keep the round trip and pay the compile on every request.
    #[tracing::instrument(name = "catalog.predicate_for", skip_all)]
    async fn predicate_for(
        &self,
        principal: &Principal,
        operation: MetadataOperation,
    ) -> Result<AccessPredicate, CatalogError> {
        let subject = subject_of(principal);
        let key = DecisionKey::new(&subject, operation);
        if let Some(cached) = self.decisions.get(&key) {
            return Ok(cached);
        }

        let policies = self.policies_for(principal).await?;
        let predicate = compile(&subject, operation, &policies);
        self.decisions.insert(key, predicate.clone());
        Ok(predicate)
    }

    /// Forget every cached authorization decision.
    ///
    /// Called by anything that changes what a decision was computed *from* — a
    /// role assignment, a policy edit. **Invalidation is the only thing that
    /// expires an entry**: there is no TTL, deliberately, because a TTL makes
    /// staleness the normal case and the window in which a revoked role still
    /// works is invisible to whoever revoked it.
    pub fn invalidate_authorization(&self) {
        self.decisions.invalidate();
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    #[tracing::instrument(name = "catalog.list_assets_for", skip_all)]
    pub async fn list_assets_for(
        &self,
        principal: &Principal,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .list_assets_visible(kind, page, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    #[tracing::instrument(name = "catalog.search_assets_for", skip_all)]
    pub async fn search_assets_for(
        &self,
        principal: &Principal,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .search_assets_visible(query, kind, page, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_children_for(
        &self,
        principal: &Principal,
        parent_id: Option<Uuid>,
    ) -> Result<Vec<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .list_children_visible(parent_id, &predicate)
            .await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn count_assets_by_kind_for(
        &self,
        principal: &Principal,
    ) -> Result<Vec<(AssetKind, i64)>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .count_assets_by_kind_visible(&predicate)
            .await?)
    }

    /// Reads one asset, or `NotFound` if policy hides it.
    ///
    /// **Hidden reads as missing, deliberately.** A `403` on a specific id
    /// confirms that id exists, which is exactly what the policy was meant to
    /// conceal.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist or is not visible.
    pub async fn get_asset_for(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Asset, CatalogError> {
        let asset = self
            .storage
            .get_asset(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        if predicate.admits(&asset.fully_qualified_name) {
            Ok(asset)
        } else {
            Err(CatalogError::NotFound)
        }
    }

    /// Auto-provisions a user on first sight, so ownership works without a
    /// directory sync (`12-13-security.md` decision 7).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn resolve_principal(
        &self,
        id: &str,
        display_name: &str,
    ) -> Result<Principal, CatalogError> {
        let user = match self.storage.find_user(id).await? {
            Some(user) => user,
            None => {
                let user = StoredUser {
                    id: id.to_string(),
                    display_name: display_name.to_string(),
                    email: None,
                    is_admin: false,
                    is_bot: false,
                    roles: Vec::new(),
                };
                self.storage.upsert_user(&user).await?;
                user
            }
        };
        Ok(Principal {
            id: user.id,
            name: user.display_name,
            kind: if user.is_bot {
                graph_owl_core::PrincipalKind::Service
            } else {
                graph_owl_core::PrincipalKind::User
            },
            roles: user.roles,
            is_admin: user.is_admin,
        })
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        Ok(self.storage.list_assets(kind, page).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, CatalogError> {
        Ok(self.storage.list_children(parent_id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, CatalogError> {
        Ok(self.storage.ancestors_of(id).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        Ok(self.storage.search_assets(query, kind, page).await?)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, CatalogError> {
        Ok(self.storage.count_assets_by_kind().await?)
    }

    /// Writes one connector record, resolving its path to a parent id.
    ///
    /// # Errors
    ///
    /// `NotFound` if the record's parent has not been written yet — which is a
    /// connector contract violation, since `Connector::fetch` promises parents
    /// before children.
    pub async fn ingest_record(
        &self,
        principal: &Principal,
        kind: AssetKind,
        path: &[String],
        description: Option<String>,
        properties: Option<serde_json::Value>,
    ) -> Result<Asset, CatalogError> {
        let parent_id = if path.len() > 1 {
            let parent_fqn = fqn::derive(
                &path[..path.len() - 1]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| {
                CatalogError::Validation(vec![FieldError::new(
                    "path",
                    FieldErrorCode::Type,
                    error.to_string(),
                )])
            })?;
            Some(
                self.storage
                    .get_asset_by_fqn(&parent_fqn)
                    .await?
                    .ok_or(CatalogError::NotFound)?
                    .id,
            )
        } else {
            None
        };

        let name = path.last().cloned().unwrap_or_default();
        self.upsert_asset(
            principal,
            UpsertAsset {
                kind,
                name,
                parent_id,
                description,
                properties,
            },
        )
        .await
    }

    // ---- envelope (Epic 3) ----

    /// Applies a partial update, advancing the version by the size of the change.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    #[tracing::instrument(name = "catalog.update_asset", skip_all)]
    pub async fn update_asset(
        &self,
        principal: &Principal,
        id: Uuid,
        update: &AssetUpdate,
        expected_version: Option<EntityVersion>,
    ) -> Result<Asset, CatalogError> {
        let before = self.storage.get_asset(id).await.unwrap_or(None);
        let updated = match self
            .storage
            .update_asset(id, update, &principal.id, expected_version)
            .await?
        {
            UpdateOutcome::Updated(asset) => *asset,
            UpdateOutcome::NotFound => return Err(CatalogError::NotFound),
            UpdateOutcome::VersionMismatch(current) => {
                return Err(CatalogError::PreconditionFailed { current });
            }
        };

        self.project(before.clone(), &updated).await;
        // Past the early returns above, so the write has committed. `updated`
        // returning `None` for a no-op is `ChangeEvent::updated`'s own rule, so
        // a no-op emits nothing without this call site deciding anything.
        self.announce(ChangeEvent::updated(
            event_subject(&updated),
            before.map_or(updated.version, |b| b.version),
            updated.version,
            updated.change_description.clone().unwrap_or_default(),
            &principal.id,
        ));
        Ok(updated)
    }

    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    pub async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, CatalogError> {
        if self.storage.get_asset(id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.asset_versions(id).await?)
    }

    /// Tombstones the asset and its subtree, returning how many were affected.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    #[tracing::instrument(name = "catalog.soft_delete_asset", skip_all)]
    pub async fn soft_delete_asset(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<u64, CatalogError> {
        let Some(before) = self.storage.get_asset(id).await? else {
            return Err(CatalogError::NotFound);
        };
        let affected = self.storage.soft_delete_asset(id, &principal.id).await?;
        self.announce(Some(ChangeEvent::soft_deleted(
            event_subject(&before),
            before.version,
            before.version,
            &principal.id,
        )));
        Ok(affected)
    }

    /// Everything the landing page needs, in one answer.
    ///
    /// One method rather than six: a dashboard that fans out to six endpoints
    /// renders in six stages and shows a different partial truth in each. Every
    /// number here goes through the same access predicate as list and search,
    /// so a total cannot leak the size of what the reader may not see.
    ///
    /// # Errors
    ///
    /// `Storage` if any of the underlying queries fails.
    pub async fn overview(&self, principal: &Principal) -> Result<Overview, CatalogError> {
        // `ViewBasic`: the overview shows names and counts, not field contents.
        // Asking for ViewDetails would hide an asset from the totals that the
        // reader can legitimately see listed.
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;

        let by_kind = self
            .storage
            .count_assets_by_kind_visible(&predicate)
            .await?;
        let (described, total) = self.storage.count_documented_visible(&predicate).await?;
        // Ten is what fits above the fold without scrolling. A longer list is
        // a worse answer to "what changed lately", not a more complete one.
        let recently_changed = self
            .storage
            .recently_changed_visible(10, &predicate)
            .await?;

        // The graph's own size. Deliberately raw counts rather than a health
        // score: a score would need a definition nothing here can defend.
        //
        // It doubles as the honest surface for projection lag — a node count
        // trailing the asset total means the graph view is behind, and a
        // number on the page beats a log line nobody reads.
        let graph = match &self.graph {
            Some(graph) => {
                let flakes = graph
                    .count(&graph_owl_core::flake::TriplePattern::default())
                    .await
                    .unwrap_or(0);
                Some(GraphSize { flakes })
            }
            None => None,
        };

        Ok(Overview {
            total: by_kind.iter().map(|(_, n)| n).sum(),
            by_kind,
            described,
            documented_total: total,
            recently_changed,
            graph,
        })
    }

    /// Tombstone the assets under `service_fqn` that the source no longer
    /// reports.
    ///
    /// The threshold guard runs **before** anything is deleted, and a refusal
    /// deletes nothing at all. A guard that stopped partway through would
    /// leave the estate in a state neither the source nor the catalog
    /// describes, which is worse than either outcome it is choosing between.
    ///
    /// # Errors
    ///
    /// `Storage` if the scan or a delete fails.
    pub async fn reconcile_deletions(
        &self,
        principal: &Principal,
        service_fqn: &str,
        seen: &std::collections::HashSet<String>,
        threshold: f64,
    ) -> Result<DeletionPlan, CatalogError> {
        let live = self.storage.list_assets_under_fqn(service_fqn).await?;
        let absent: Vec<&Asset> = live
            .iter()
            .filter(|asset| !seen.contains(&asset.fully_qualified_name))
            .collect();

        let plan = DeletionPlan::decide(absent.len(), live.len(), threshold);
        if !plan.is_allowed() {
            return Ok(plan);
        }

        // Delete only the *shallowest* absent assets. Soft delete cascades, so
        // tombstoning a table already tombstones its columns; deleting both
        // would double-count the result and issue N redundant writes for a
        // wide table.
        let absent_fqns: std::collections::HashSet<&str> = absent
            .iter()
            .map(|a| a.fully_qualified_name.as_str())
            .collect();
        for asset in &absent {
            let parent_is_also_absent = fqn::parent(&asset.fully_qualified_name)
                .is_some_and(|parent| absent_fqns.contains(parent));
            if parent_is_also_absent {
                continue;
            }
            self.storage
                .soft_delete_asset(asset.id, &principal.id)
                .await?;
        }

        Ok(plan)
    }

    /// # Errors
    ///
    /// `NotFound` if the asset does not exist.
    #[tracing::instrument(name = "catalog.restore_asset", skip_all)]
    pub async fn restore_asset(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<u64, CatalogError> {
        let Some(before) = self.storage.get_asset(id).await? else {
            return Err(CatalogError::NotFound);
        };
        let affected = self.storage.restore_asset(id, &principal.id).await?;
        self.announce(Some(ChangeEvent::restored(
            event_subject(&before),
            before.version,
            before.version,
            &principal.id,
        )));
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::page::Cursor;

    /// An asset and everything beneath it. Used by the fake's cascade, which
    /// must match Postgres's recursive CTE or a cascade bug passes here.
    fn descendants(assets: &[Asset], root: Uuid) -> Vec<Uuid> {
        let mut found = vec![root];
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for child in assets.iter().filter(|a| a.parent_id == Some(parent)) {
                if !found.contains(&child.id) {
                    found.push(child.id);
                    frontier.push(child.id);
                }
            }
        }
        found
    }
    use graph_owl_storage::Storage;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub(super) struct InMemoryStorage {
        assets: Mutex<Vec<Asset>>,
        versions: Mutex<Vec<AssetVersion>>,
        users: Mutex<Vec<StoredUser>>,
        pub(super) policies: Mutex<Vec<Policy>>,
        inserted: Mutex<Vec<Table>>,
        relationships: Mutex<Vec<Relationship>>,
        /// When armed, any relational write panics. Lets a test assert "this
        /// code path writes nothing" structurally instead of by reading it and
        /// believing what it says.
        writes_forbidden: std::sync::atomic::AtomicBool,
        /// How many times policies were read from storage. The decision cache
        /// is invisible in the *result* — a cached and an uncached predicate
        /// are the same predicate — so the only observable is whether the
        /// question reached storage at all.
        pub(super) policy_reads: std::sync::atomic::AtomicUsize,
    }

    impl InMemoryStorage {
        pub(super) fn forbid_writes(&self) {
            self.writes_forbidden
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        fn guard_write(&self, what: &str) {
            assert!(
                !self
                    .writes_forbidden
                    .load(std::sync::atomic::Ordering::SeqCst),
                "{what} wrote to relational storage while writes were forbidden"
            );
        }
    }

    #[async_trait::async_trait]
    impl Storage for InMemoryStorage {
        async fn ping(&self) -> Result<(), StorageError> {
            Ok(())
        }

        // The fake honours the same identity rule as Postgres: the FQN is the
        // identity, so a re-upsert converges instead of duplicating.
        async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError> {
            self.guard_write("upsert_asset");
            let mut assets = self.assets.lock().unwrap();
            if let Some(existing) = assets
                .iter_mut()
                .find(|a| a.fully_qualified_name == asset.fully_qualified_name)
            {
                existing.name = asset.name;
                existing.parent_id = asset.parent_id;
                existing.description = asset.description.or(existing.description.clone());
                existing.properties = asset.properties.or(existing.properties.clone());
                existing.updated_at = asset.updated_at;
                return Ok(existing.clone());
            }
            assets.push(asset.clone());
            Ok(asset)
        }

        async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.id == id)
                .cloned())
        }

        async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.fully_qualified_name == fqn)
                .cloned())
        }

        async fn list_assets(
            &self,
            kind: Option<AssetKind>,
            page: &PageRequest,
        ) -> Result<Page<Asset>, StorageError> {
            let mut assets: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| kind.is_none_or(|k| a.kind == k))
                .cloned()
                .collect();
            assets.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                assets.retain(|a| {
                    (a.fully_qualified_name.as_str(), a.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            assets.truncate(page.limit + 1);
            Ok(Page::from_overfetch(assets, page.limit, |a| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError> {
            let mut children: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.parent_id == parent_id)
                .cloned()
                .collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(children)
        }

        async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError> {
            let assets = self.assets.lock().unwrap().clone();
            let mut chain = Vec::new();
            let mut current = assets.iter().find(|a| a.id == id).cloned();
            while let Some(asset) = current {
                current = asset
                    .parent_id
                    .and_then(|pid| assets.iter().find(|a| a.id == pid).cloned());
                chain.push(asset);
            }
            chain.reverse();
            Ok(chain)
        }

        async fn search_assets(
            &self,
            query: &str,
            kind: Option<AssetKind>,
            page: &PageRequest,
        ) -> Result<Page<Asset>, StorageError> {
            let needle = query.to_lowercase();
            let mut assets: Vec<Asset> = self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|a| {
                    (a.name.to_lowercase().contains(&needle)
                        || a.fully_qualified_name.to_lowercase().contains(&needle))
                        && kind.is_none_or(|k| a.kind == k)
                })
                .cloned()
                .collect();
            assets.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
            assets.truncate(page.limit + 1);
            Ok(Page::from_overfetch(assets, page.limit, |a| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError> {
            // Same boundary rule as Postgres: `hdfc-core` must not sweep in
            // `hdfc-core-archive`. A fake that matched more loosely would let a
            // scope bug pass here and fail only against the real database.
            let assets = self.assets.lock().unwrap();
            Ok(assets
                .iter()
                .filter(|a| {
                    !a.deleted
                        && (prefix.is_empty()
                            || a.fully_qualified_name == prefix
                            || a.fully_qualified_name.starts_with(&format!("{prefix}.")))
                })
                .cloned()
                .collect())
        }

        async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError> {
            let assets = self.assets.lock().unwrap();
            Ok(AssetKind::ALL
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        i64::try_from(assets.iter().filter(|a| a.kind == kind).count())
                            .unwrap_or(i64::MAX),
                    )
                })
                .filter(|(_, n)| *n > 0)
                .collect())
        }

        // The fake honours the same envelope contract as Postgres, including
        // the no-op rule: a fake that always bumped would let a version-inflation
        // bug pass here and fail only against a real database.
        async fn update_asset(
            &self,
            id: Uuid,
            update: &AssetUpdate,
            updated_by: &str,
            expected_version: Option<EntityVersion>,
        ) -> Result<UpdateOutcome, StorageError> {
            use graph_owl_core::envelope::{ChangeDescription, ChangeKind, classify};
            self.guard_write("update_asset");
            let mut assets = self.assets.lock().unwrap();
            let Some(existing) = assets.iter_mut().find(|a| a.id == id) else {
                return Ok(UpdateOutcome::NotFound);
            };
            let before = existing.clone();
            // The fake enforces the precondition too. One that ignored it would
            // let a lost-update bug pass here and fail only against Postgres.
            if expected_version.is_some_and(|expected| before.version != expected) {
                return Ok(UpdateOutcome::VersionMismatch(before.version));
            }
            let mut after = before.clone();
            if let Some(description) = &update.description {
                after.description = description.clone();
            }
            let diff = ChangeDescription::between(
                &serde_json::to_value(&before).unwrap_or_default(),
                &serde_json::to_value(&after).unwrap_or_default(),
            );
            let kind = classify(&diff);
            if matches!(kind, ChangeKind::None) {
                return Ok(UpdateOutcome::Updated(Box::new(before)));
            }
            after.version = before.version.bump(kind);
            after.updated_by = updated_by.to_string();
            after.change_description = Some(diff.clone());
            after.updated_at = Utc::now();
            *existing = after.clone();
            self.versions.lock().unwrap().push(AssetVersion {
                version: after.version,
                snapshot: after.clone(),
                change_description: Some(diff),
                updated_by: updated_by.to_string(),
                updated_at: after.updated_at,
            });
            Ok(UpdateOutcome::Updated(Box::new(after)))
        }

        async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError> {
            let mut versions: Vec<AssetVersion> = self
                .versions
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.snapshot.id == id)
                .cloned()
                .collect();
            versions.sort_by(|a, b| b.version.cmp(&a.version));
            Ok(versions)
        }

        async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError> {
            self.guard_write("soft_delete_asset");
            let mut assets = self.assets.lock().unwrap();
            let subtree = descendants(&assets, id);
            let mut affected = 0;
            for asset in assets
                .iter_mut()
                .filter(|a| subtree.contains(&a.id) && !a.deleted)
            {
                asset.deleted = true;
                asset.deleted_at = Some(Utc::now());
                asset.updated_by = deleted_by.to_string();
                affected += 1;
            }
            Ok(affected)
        }

        async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError> {
            let mut assets = self.assets.lock().unwrap();
            let subtree = descendants(&assets, id);
            let mut affected = 0;
            for asset in assets
                .iter_mut()
                .filter(|a| subtree.contains(&a.id) && a.deleted)
            {
                asset.deleted = false;
                asset.deleted_at = None;
                asset.updated_by = restored_by.to_string();
                affected += 1;
            }
            Ok(affected)
        }

        // The fake applies the *same* AccessPredicate::admits used by the real
        // adapter's reference semantics, so a lowering bug shows as a
        // disagreement rather than passing here and failing in Postgres.
        async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError> {
            Ok(self
                .users
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id == id)
                .cloned())
        }

        async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError> {
            let mut users = self.users.lock().unwrap();
            if let Some(existing) = users.iter_mut().find(|u| u.id == user.id) {
                *existing = user.clone();
            } else {
                users.push(user.clone());
            }
            Ok(())
        }

        /// **Honours `roles`**, because the port does.
        ///
        /// The real adapter joins `role_policies` on the roles it was given, so
        /// a principal holding no matching role gets no policy. A double that
        /// returned every policy regardless made role-scoped authorization
        /// unobservable in this crate — a decision cache keyed on the wrong
        /// thing would have passed every test here.
        ///
        /// A policy is attached to the role of the same name. That is the
        /// fake's convention, not the schema's, and it is enough to model the
        /// one property that matters: different roles resolve different
        /// policies.
        async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError> {
            self.policy_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self
                .policies
                .lock()
                .unwrap()
                .iter()
                .filter(|policy| roles.contains(&policy.name))
                .cloned()
                .collect())
        }

        async fn list_assets_visible(
            &self,
            kind: Option<AssetKind>,
            page: &PageRequest,
            predicate: &AccessPredicate,
        ) -> Result<Page<Asset>, StorageError> {
            let all = self.list_assets(kind, page).await?;
            let visible: Vec<Asset> = all
                .data
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect();
            Ok(Page::from_overfetch(visible, page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn search_assets_visible(
            &self,
            query: &str,
            kind: Option<AssetKind>,
            page: &PageRequest,
            predicate: &AccessPredicate,
        ) -> Result<Page<Asset>, StorageError> {
            let all = self.search_assets(query, kind, page).await?;
            let visible: Vec<Asset> = all
                .data
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect();
            Ok(Page::from_overfetch(visible, page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn list_children_visible(
            &self,
            parent_id: Option<Uuid>,
            predicate: &AccessPredicate,
        ) -> Result<Vec<Asset>, StorageError> {
            Ok(self
                .list_children(parent_id)
                .await?
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .collect())
        }

        async fn count_documented_visible(
            &self,
            predicate: &AccessPredicate,
        ) -> Result<(i64, i64), StorageError> {
            let assets = self.assets.lock().unwrap();
            let visible: Vec<_> = assets
                .iter()
                .filter(|a| !a.deleted && predicate.admits(&a.fully_qualified_name))
                .collect();
            let described = visible
                .iter()
                .filter(|a| {
                    a.description
                        .as_deref()
                        .is_some_and(|d| !d.trim().is_empty())
                })
                .count();
            Ok((
                i64::try_from(described).unwrap_or(i64::MAX),
                i64::try_from(visible.len()).unwrap_or(i64::MAX),
            ))
        }

        async fn recently_changed_visible(
            &self,
            limit: i64,
            predicate: &AccessPredicate,
        ) -> Result<Vec<Asset>, StorageError> {
            let assets = self.assets.lock().unwrap();
            let mut visible: Vec<Asset> = assets
                .iter()
                .filter(|a| !a.deleted && predicate.admits(&a.fully_qualified_name))
                .cloned()
                .collect();
            visible.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
            visible.truncate(usize::try_from(limit).unwrap_or(0));
            Ok(visible)
        }

        async fn count_assets_by_kind_visible(
            &self,
            predicate: &AccessPredicate,
        ) -> Result<Vec<(AssetKind, i64)>, StorageError> {
            let assets = self.assets.lock().unwrap();
            Ok(AssetKind::ALL
                .into_iter()
                .map(|kind| {
                    let n = assets
                        .iter()
                        .filter(|a| a.kind == kind && predicate.admits(&a.fully_qualified_name))
                        .count();
                    (kind, i64::try_from(n).unwrap_or(i64::MAX))
                })
                .filter(|(_, n)| *n > 0)
                .collect())
        }

        async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
            self.inserted.lock().unwrap().push(table.clone());
            Ok(table)
        }

        async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
            Ok(self
                .inserted
                .lock()
                .unwrap()
                .iter()
                .find(|table| table.id == id)
                .cloned())
        }

        async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
            // The fake honours the same ordering and keyset contract as the
            // Postgres adapter. A fake that returns insertion order would let
            // a pagination bug pass here and fail only against a real database,
            // which is the whole failure mode a port is supposed to prevent.
            let mut tables = self.inserted.lock().unwrap().clone();
            tables.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                tables.retain(|t| {
                    (t.fully_qualified_name.as_str(), t.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            tables.truncate(page.limit + 1);
            Ok(Page::from_overfetch(tables, page.limit, |t| {
                Cursor::new(t.fully_qualified_name.clone(), t.id)
            }))
        }

        async fn update_table(
            &self,
            id: Uuid,
            update: TableUpdate,
        ) -> Result<Option<Table>, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let Some(table) = inserted.iter_mut().find(|table| table.id == id) else {
                return Ok(None);
            };
            if let Some(name) = update.name {
                table.name = name;
            }
            if let Some(description) = update.description {
                table.description = Some(description);
            }
            table.updated_at = Utc::now();
            Ok(Some(table.clone()))
        }

        async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let original_len = inserted.len();
            inserted.retain(|table| table.id != id);
            Ok(inserted.len() != original_len)
        }

        async fn create_relationship(
            &self,
            relationship: Relationship,
        ) -> Result<Relationship, StorageError> {
            self.relationships
                .lock()
                .unwrap()
                .push(relationship.clone());
            Ok(relationship)
        }

        async fn list_relationships_for_entity(
            &self,
            entity_type: &str,
            entity_id: Uuid,
        ) -> Result<Vec<Relationship>, StorageError> {
            Ok(self
                .relationships
                .lock()
                .unwrap()
                .iter()
                .filter(|relationship| {
                    (relationship.from_entity_type == entity_type
                        && relationship.from_entity_id == entity_id)
                        || (relationship.to_entity_type == entity_type
                            && relationship.to_entity_id == entity_id)
                })
                .cloned()
                .collect())
        }

        async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError> {
            let relationships = self.relationships.lock().unwrap();
            Ok(relationships.iter().find(|r| r.id == id).cloned())
        }

        async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut relationships = self.relationships.lock().unwrap();
            let original_len = relationships.len();
            relationships.retain(|relationship| relationship.id != id);
            Ok(relationships.len() != original_len)
        }
    }

    fn mock_create_table_request() -> CreateTable {
        CreateTable {
            name: "customers".to_string(),
            fully_qualified_name: "warehouse.public.customers".to_string(),
            description: None,
        }
    }

    #[tokio::test]
    async fn creating_a_table_assigns_matching_created_and_updated_timestamps() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let table = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_eq!(table.name, "customers");
        assert_eq!(table.fully_qualified_name, "warehouse.public.customers");
        assert_eq!(table.created_at, table.updated_at);
    }

    #[tokio::test]
    async fn creating_two_tables_assigns_different_ids() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let first = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_ne!(first.id, second.id);
    }

    #[tokio::test]
    async fn getting_a_table_by_id_returns_the_stored_table() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");

        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn getting_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let found = catalog
            .get_table(Uuid::new_v4())
            .await
            .expect("get_table should succeed");

        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn listing_tables_with_none_created_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        assert_eq!(page.data, Vec::new());
        assert_eq!(page.paging.after, None);
    }

    #[tokio::test]
    async fn listing_tables_returns_all_created_tables() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let first = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(
                &Principal::system(),
                CreateTable {
                    fully_qualified_name: "warehouse.public.orders".to_string(),
                    ..mock_create_table_request()
                },
            )
            .await
            .expect("create_table should succeed");

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        // Sorted by FQN, so the order is the contract's, not insertion order.
        let mut expected = vec![first, second];
        expected.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        assert_eq!(page.data, expected);
        assert_eq!(page.paging.after, None, "both rows fit in one page");
    }

    #[tokio::test]
    async fn updating_a_table_changes_only_the_provided_fields() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let updated = catalog
            .update_table(
                &Principal::system(),
                created.id,
                TableUpdate {
                    name: None,
                    description: Some("a new description".to_string()),
                },
            )
            .await
            .expect("update_table should succeed")
            .expect("table should exist");

        assert_eq!(updated.name, created.name);
        assert_eq!(updated.description, Some("a new description".to_string()));
        assert_eq!(updated.created_at, created.created_at);
    }

    #[tokio::test]
    async fn updating_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .update_table(&Principal::system(), Uuid::new_v4(), TableUpdate::default())
            .await
            .expect("update_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_table_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let deleted = catalog
            .delete_table(&Principal::system(), created.id)
            .await
            .expect("delete_table should succeed");

        assert!(deleted);
        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_table_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_table(&Principal::system(), Uuid::new_v4())
            .await
            .expect("delete_table should succeed");

        assert!(!deleted);
    }

    #[tokio::test]
    async fn creating_a_relationship_between_two_existing_tables_succeeds() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationship = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        assert_eq!(relationship.from_entity_type, "table");
        assert_eq!(relationship.from_entity_id, from.id);
        assert_eq!(relationship.to_entity_type, "table");
        assert_eq!(relationship.to_entity_id, to.id);
        assert_eq!(relationship.relationship_type, "derivedFrom");
    }

    #[tokio::test]
    async fn creating_a_relationship_from_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                Uuid::new_v4(),
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn creating_a_relationship_to_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: Uuid::new_v4(),
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn creating_a_relationship_with_an_empty_type_is_a_field_validation_error() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: String::new(),
                },
            )
            .await;

        assert!(
            matches!(result, Err(CatalogError::Validation(ref errors))
                if errors.iter().any(|e| e.field == "relationshipType")),
            "an empty type is now an unknown vocabulary member, reported per field"
        );
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_with_none_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let table = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationships = catalog
            .list_relationships_for_table(table.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships, Vec::new());
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_returns_relationships_from_either_side() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let customers = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let archive = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        catalog
            .create_relationship(
                &Principal::system(),
                orders.id,
                CreateRelationship {
                    to_table_id: customers.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");
        catalog
            .create_relationship(
                &Principal::system(),
                archive.id,
                CreateRelationship {
                    to_table_id: orders.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let relationships = catalog
            .list_relationships_for_table(orders.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships.len(), 2);
    }

    #[tokio::test]
    async fn listing_relationships_for_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .list_relationships_for_table(Uuid::new_v4())
            .await
            .expect("list_relationships_for_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_relationship_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(&Principal::system(), mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let relationship = catalog
            .create_relationship(
                &Principal::system(),
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derivedFrom".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let deleted = catalog
            .delete_relationship(&Principal::system(), relationship.id)
            .await
            .expect("delete_relationship should succeed");

        assert!(deleted);
        let remaining = catalog
            .list_relationships_for_table(from.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");
        assert_eq!(remaining, Vec::new());
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_relationship_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_relationship(&Principal::system(), Uuid::new_v4())
            .await
            .expect("delete_relationship should succeed");

        assert!(!deleted);
    }
}

#[cfg(test)]
mod scope_facts_tests {
    use super::{SparqlBudget, scope_facts};
    use graph_owl_core::flake::{Flake, FlakeValue, Sid};
    use std::collections::HashSet;

    fn visible(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn about(subject: &str, predicate: &str, value: FlakeValue) -> Flake {
        Flake::assert(Sid::dsc(subject), Sid::dsc(predicate), value, 1)
    }

    fn edge(id: &str, from: &str, to: &str) -> Vec<Flake> {
        vec![
            about(id, "fromEntity", FlakeValue::Ref(Sid::dsc(from))),
            about(id, "toEntity", FlakeValue::Ref(Sid::dsc(to))),
            about(id, "relType", FlakeValue::String("feeds".into())),
        ]
    }

    #[test]
    fn a_fact_about_a_visible_asset_is_kept() {
        let (kept, truncated) = scope_facts(
            &[about("a", "name", FlakeValue::String("visible".into()))],
            &visible(&["a"]),
            100,
        );
        assert_eq!(kept.len(), 1);
        assert!(!truncated);
    }

    #[test]
    fn a_fact_about_a_hidden_asset_is_dropped() {
        let (kept, _) = scope_facts(
            &[about("secret", "name", FlakeValue::String("pan".into()))],
            &visible(&["a"]),
            100,
        );
        assert!(kept.is_empty());
    }

    /// **The property this function exists for.** An edge is not an asset, so
    /// its visibility is not its own — an edge whose far end is hidden would
    /// disclose that the far end *exists*, which is exactly what the policy
    /// concealed.
    #[test]
    fn an_edge_to_a_hidden_asset_is_dropped_entirely() {
        let mut flakes = edge("r1", "a", "secret");
        flakes.push(about("a", "name", FlakeValue::String("visible".into())));

        let (kept, _) = scope_facts(&flakes, &visible(&["a"]), 100);

        assert_eq!(kept.len(), 1, "only the visible asset's own fact: {kept:?}");
        assert!(
            !kept.iter().any(|f| f.s.id == "r1"),
            "no part of the edge may survive — not even its relType, which \
             would still prove an edge exists"
        );
    }

    #[test]
    fn an_edge_between_two_visible_assets_is_kept_whole() {
        let (kept, _) = scope_facts(&edge("r1", "a", "b"), &visible(&["a", "b"]), 100);
        assert_eq!(kept.len(), 3, "every flake of the edge: {kept:?}");
    }

    /// Direction must not matter. Checking only one endpoint would leak in one
    /// direction and not the other, which is worse than leaking in both because
    /// nobody would find it.
    #[test]
    fn an_edge_from_a_hidden_asset_is_dropped_too() {
        let (kept, _) = scope_facts(&edge("r1", "secret", "b"), &visible(&["b"]), 100);
        assert!(kept.is_empty(), "{kept:?}");
    }

    /// A relationship node with no endpoint flakes at all — a half-written
    /// projection — must not be assumed visible. Absence of evidence is not
    /// permission.
    #[test]
    fn an_edge_with_no_endpoints_is_not_assumed_visible() {
        let orphan = vec![about("r1", "relType", FlakeValue::String("feeds".into()))];
        let (kept, _) = scope_facts(&orphan, &visible(&["a", "b"]), 100);
        assert!(kept.is_empty());
    }

    #[test]
    fn exceeding_the_budget_truncates_and_says_so() {
        let flakes: Vec<Flake> = (0..10)
            .map(|i| about("a", &format!("p{i}"), FlakeValue::Int(i)))
            .collect();
        let (kept, truncated) = scope_facts(&flakes, &visible(&["a"]), 4);
        assert_eq!(kept.len(), 4);
        assert!(truncated, "a truncated answer must never look complete");
    }

    /// Landing exactly on the budget is not truncation. Reporting it as such
    /// would make every full-budget answer look unreliable.
    #[test]
    fn landing_exactly_on_the_budget_is_not_truncation() {
        let flakes: Vec<Flake> = (0..4)
            .map(|i| about("a", &format!("p{i}"), FlakeValue::Int(i)))
            .collect();
        let (kept, truncated) = scope_facts(&flakes, &visible(&["a"]), 4);
        assert_eq!(kept.len(), 4);
        assert!(!truncated);
    }

    #[test]
    fn the_default_budget_is_bounded_and_useful() {
        let budget = SparqlBudget::default();
        assert!(
            budget.max_facts >= 10_000,
            "too small to answer real questions"
        );
        assert!(budget.max_facts <= 1_000_000, "not a budget");
    }
}

#[cfg(test)]
mod projection_isolation_tests {
    use super::*;
    use async_trait::async_trait;
    use graph_owl_core::flake::{Flake, TriplePattern};
    use graph_owl_engine::EngineError;
    use std::sync::Mutex;
    use tests::InMemoryStorage;

    /// A graph that records what it was asked to do, and can be told to fail.
    struct RecordingGraph {
        fail: bool,
        asserted: Mutex<Vec<Flake>>,
        retracted: Mutex<Vec<Flake>>,
        /// Every pattern this store was asked to answer. Some obligations —
        /// narrowing a scan to one subject — change the *question* and not the
        /// answer, and are unobservable from the result alone.
        queried: Mutex<Vec<TriplePattern>>,
        clock: std::sync::atomic::AtomicI64,
        at_resolves_to: std::sync::atomic::AtomicI64,
    }

    impl RecordingGraph {
        fn with(fail: bool) -> Arc<Self> {
            Arc::new(Self {
                fail,
                asserted: Mutex::new(Vec::new()),
                retracted: Mutex::new(Vec::new()),
                queried: Mutex::new(Vec::new()),
                clock: std::sync::atomic::AtomicI64::new(0),
                at_resolves_to: std::sync::atomic::AtomicI64::new(i64::MAX),
            })
        }

        fn working() -> Arc<Self> {
            Self::with(false)
        }

        fn broken() -> Arc<Self> {
            Self::with(true)
        }

        fn refuse<T>() -> Result<T, EngineError> {
            Err(EngineError::Backend("the graph is down".to_string()))
        }

        /// The port's contract, in memory: bindings narrow, `as_of` bounds, and
        /// each fact identity resolves to its newest row — which is dropped if
        /// that row is a retraction.
        ///
        /// A double looser than the port lets production code that ignores the
        /// contract pass, which is precisely how the `s` and `as_of` mutants on
        /// `get_asset_as_of` survived: neither field changed what came back.
        fn resolve(&self, pattern: &TriplePattern) -> Vec<Flake> {
            let assertions = self.asserted.lock().expect("lock");
            let retractions = self.retracted.lock().expect("lock");
            let mut latest: std::collections::HashMap<String, &Flake> =
                std::collections::HashMap::new();

            for flake in assertions
                .iter()
                .chain(retractions.iter())
                .filter(|f| pattern.as_of.is_none_or(|t| f.t <= t))
                .filter(|f| pattern.s.as_ref().is_none_or(|s| &f.s == s))
                .filter(|f| pattern.p.as_ref().is_none_or(|p| &f.p == p))
                .filter(|f| pattern.o.as_ref().is_none_or(|o| &f.o == o))
                .filter(|f| pattern.cx.as_ref().is_none_or(|cx| &f.cx == cx))
            {
                // Everything but `t` and `op` — the same fact identity the
                // relational store groups by.
                let identity = format!("{:?}|{:?}|{:?}|{:?}", flake.s, flake.p, flake.o, flake.cx);
                match latest.get(&identity) {
                    Some(seen) if seen.t >= flake.t => {}
                    _ => {
                        latest.insert(identity, flake);
                    }
                }
            }

            latest
                .into_values()
                .filter(|f| f.op)
                .cloned()
                .collect::<Vec<_>>()
        }
    }

    #[async_trait]
    impl TripleStore for RecordingGraph {
        async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
            if self.fail {
                return Self::refuse();
            }
            self.asserted
                .lock()
                .expect("lock")
                .extend_from_slice(flakes);
            Ok(())
        }

        async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
            if self.fail {
                return Self::refuse();
            }
            self.retracted
                .lock()
                .expect("lock")
                .extend_from_slice(flakes);
            Ok(())
        }

        /// Serves back the resolved current state at the pattern's instant.
        ///
        /// A double that always returned nothing would make `Catalog::sparql`
        /// untestable here — every query would answer zero rows whether the
        /// code worked or not, which is the shape of a test that cannot fail.
        async fn query_pattern(&self, pattern: &TriplePattern) -> Result<Vec<Flake>, EngineError> {
            self.queried.lock().expect("lock").push(pattern.clone());
            Ok(self.resolve(pattern))
        }

        /// Counted through the same resolution as the rows.
        ///
        /// A count computed by a separate path is a count that can disagree
        /// with the rows, and the disagreement always surfaces far away from
        /// here — the same reason the Postgres adapter shares one builder.
        async fn count(&self, pattern: &TriplePattern) -> Result<u64, EngineError> {
            self.queried.lock().expect("lock").push(pattern.clone());
            Ok(self.resolve(pattern).len() as u64)
        }

        /// A real advancing clock, so successive writes land at different `t`.
        /// A double that returned a constant would make `as_of` unobservable —
        /// every fact would sit at the same instant and no historical query
        /// could differ from a present one.
        async fn next_time(&self) -> Result<i64, EngineError> {
            if self.fail {
                return Self::refuse();
            }
            Ok(self.clock.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1)
        }

        /// Resolves any instant to whatever `at_resolves_to` says, so a test
        /// can ask for a specific point in history without inventing wall
        /// clocks.
        async fn time_at(
            &self,
            _: chrono::DateTime<chrono::Utc>,
        ) -> Result<Option<i64>, EngineError> {
            Ok(Some(
                self.at_resolves_to
                    .load(std::sync::atomic::Ordering::SeqCst),
            ))
        }
    }

    fn service(name: &str) -> UpsertAsset {
        UpsertAsset {
            kind: AssetKind::Service,
            name: name.to_string(),
            parent_id: None,
            description: None,
            properties: None,
        }
    }

    /// A projection that failed leaves the entity intact and the graph behind.
    /// The reconciler is what makes that recoverable rather than permanent.
    #[tokio::test]
    async fn the_reconciler_repairs_what_a_failed_projection_left_behind() {
        let storage = Arc::new(InMemoryStorage::default());
        let broken = Catalog::new(storage.clone()).with_graph(RecordingGraph::broken());

        let created = broken
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("the entity survives a graph that is down");

        // The same storage, now with a working graph — what a restart after an
        // outage looks like.
        let graph = RecordingGraph::working();
        let repaired = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let drifted = repaired.projection_drift().await.expect("drift");
        assert_eq!(drifted.len(), 1, "the unprojected asset is drift");
        assert_eq!(drifted[0].id, created.id);

        let count = repaired.reconcile_projection().await.expect("reconcile");
        assert_eq!(count, 1);
        assert!(
            graph
                .asserted
                .lock()
                .expect("lock")
                .iter()
                .any(|f| f.p.id == "fqn"),
            "the repaired asset must actually reach the graph"
        );
    }

    /// **Decision 1's invariant.** The fake panics on any relational write
    /// while the guard is armed, so a reconciler that wrote back could not
    /// pass this test.
    ///
    /// Asserted structurally rather than by reading the reconciler and
    /// believing it: reconciliation re-projects *from* relational and must
    /// never write *to* it. If it could, the graph view — which lags by design
    /// — would overwrite the source of truth and the two stores would fight.
    #[tokio::test]
    async fn reconciliation_never_writes_to_relational_storage() {
        let storage = Arc::new(InMemoryStorage::default());
        let seeding = Catalog::new(storage.clone()).with_graph(RecordingGraph::broken());
        seeding
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("seed");

        // From here, any relational write is a bug.
        storage.forbid_writes();

        let catalog =
            Catalog::new(storage).with_graph(RecordingGraph::working() as Arc<dyn TripleStore>);
        let repaired = catalog
            .reconcile_projection()
            .await
            .expect("reconciliation must not need to write relational");
        assert_eq!(repaired, 1);
    }

    /// Running it twice must converge, not duplicate — which is what makes it
    /// safe to schedule.
    #[tokio::test]
    async fn reconciliation_is_idempotent() {
        let storage = Arc::new(InMemoryStorage::default());
        let broken = Catalog::new(storage.clone()).with_graph(RecordingGraph::broken());
        broken
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        assert_eq!(catalog.reconcile_projection().await.expect("first"), 1);
        // The double records every assertion, so the second pass finding
        // nothing to repair is what "converged" means here.
        assert_eq!(
            catalog.reconcile_projection().await.expect("second"),
            0,
            "a second pass must find nothing left to repair"
        );
    }

    /// **Regression test.** `list_assets_under_fqn("")` used to match nothing —
    /// `fqn LIKE '.%'` is false for every real FQN — so drift detection scanned
    /// an empty set and reported no drift. A detector that always says "all
    /// clear" is worse than none, because it is believed.
    ///
    /// The original test passed because its only asset was a *root*, which a
    /// separate `list_children(None)` call happened to cover. A nested asset
    /// was invisible.
    #[tokio::test]
    async fn drift_detection_sees_nested_assets_not_only_roots() {
        let storage = Arc::new(InMemoryStorage::default());
        let broken = Catalog::new(storage.clone()).with_graph(RecordingGraph::broken());

        let root = broken
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("root");
        broken
            .upsert_asset(
                &Principal::system(),
                UpsertAsset {
                    kind: AssetKind::Database,
                    name: "postgres".to_string(),
                    parent_id: Some(root.id),
                    description: None,
                    properties: None,
                },
            )
            .await
            .expect("nested");

        let repaired =
            Catalog::new(storage).with_graph(RecordingGraph::working() as Arc<dyn TripleStore>);
        let drifted = repaired.projection_drift().await.expect("drift");

        assert_eq!(
            drifted.len(),
            2,
            "both the root and the nested asset are unprojected: {:?}",
            drifted
                .iter()
                .map(|a| &a.fully_qualified_name)
                .collect::<Vec<_>>()
        );
    }

    /// Drift must report only what is *actually* missing. The per-subject
    /// pattern is what makes that possible — a scan that ignored the subject
    /// would count every `fqn` flake in the graph and conclude nothing is
    /// drifted, which is the answer a broken detector gives.
    #[tokio::test]
    async fn drift_reports_only_the_unprojected_assets() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();

        // One asset projected properly.
        let working =
            Catalog::new(storage.clone()).with_graph(graph.clone() as Arc<dyn TripleStore>);
        working
            .upsert_asset(&Principal::system(), service("projected"))
            .await
            .expect("projected");

        // One written while the graph was down.
        let broken = Catalog::new(storage.clone()).with_graph(RecordingGraph::broken());
        broken
            .upsert_asset(&Principal::system(), service("unprojected"))
            .await
            .expect("unprojected");

        let drifted = working.projection_drift().await.expect("drift");

        assert_eq!(drifted.len(), 1, "{drifted:?}");
        assert_eq!(drifted[0].name, "unprojected");
    }

    /// A **half-projected** asset is drift too. Drift asks specifically whether
    /// the `fqn` flake is present, not whether the subject has any flakes at
    /// all — an asset carrying a few fields and missing its identity is exactly
    /// the state a failed projection leaves behind, and the looser check would
    /// call it healthy.
    #[tokio::test]
    async fn an_asset_projected_without_its_fqn_is_still_drift() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog =
            Catalog::new(storage.clone()).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let asset = catalog
            .upsert_asset(&Principal::system(), service("half"))
            .await
            .expect("create");

        // Simulate a projection that wrote some fields and lost the rest.
        {
            let mut asserted = graph.asserted.lock().expect("lock");
            asserted.retain(|f| f.p.id != "fqn");
            assert!(
                asserted.iter().any(|f| f.s.id == asset.id.to_string()),
                "the subject still has other flakes, which is the point"
            );
        }

        let drifted = catalog.projection_drift().await.expect("drift");
        assert_eq!(
            drifted.len(),
            1,
            "an asset without its fqn is not projected: {drifted:?}"
        );
    }

    /// No graph configured is not drift. Reporting every asset as drifted on a
    /// deployment that never wanted a graph would make the number meaningless.
    #[tokio::test]
    async fn a_catalog_with_no_graph_reports_no_drift() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");
        assert!(catalog.projection_drift().await.expect("drift").is_empty());
    }

    /// A SPARQL answer must actually contain rows. `collect` returning an
    /// empty vec is otherwise indistinguishable from a query that matched
    /// nothing — and the HTTP tests that cover this live in another crate,
    /// which a mutation run scoped to this one never executes.
    #[tokio::test]
    async fn sparql_returns_rows_from_the_graph() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "SELECT ?n WHERE { ?s <https://graph-owl.dev/ns/catalog#name> ?n }",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(outcome.rows.len(), 1, "{:?}", outcome.rows);
        assert!(
            outcome.rows[0]["n"].contains("hdfc-core"),
            "{:?}",
            outcome.rows
        );
        assert!(outcome.facts_scanned > 0);
        assert!(!outcome.truncated);
        let _ = created;
    }

    /// `as_of` must reach the scan.
    ///
    /// Dropping the field would silently answer every historical question with
    /// present-day facts — the one failure that makes time travel worse than
    /// not having it, because the answer looks right.
    ///
    /// Observable only because the double's clock advances: the first asset
    /// lands at `t = 1`, the second at `t = 2`, and a query resolved to `t = 1`
    /// must not see the second.
    #[tokio::test]
    async fn sparql_honours_as_of_by_reaching_the_scan() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        catalog
            .upsert_asset(&Principal::system(), service("first"))
            .await
            .expect("first");
        catalog
            .upsert_asset(&Principal::system(), service("second"))
            .await
            .expect("second");

        let query = "SELECT ?n WHERE { ?s <https://graph-owl.dev/ns/catalog#name> ?n }";

        let now = catalog
            .sparql(&Principal::system(), query, None, SparqlBudget::default())
            .await
            .expect("query");
        assert_eq!(now.rows.len(), 2, "both exist now: {:?}", now.rows);

        // Resolve any instant to t = 1 — before the second write.
        graph
            .at_resolves_to
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let earlier = catalog
            .sparql(
                &Principal::system(),
                query,
                Some(Utc::now()),
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(earlier.as_of, Some(1), "the resolved t must be reported");
        assert_eq!(
            earlier.rows.len(),
            1,
            "the second asset did not exist at t=1: {:?}",
            earlier.rows
        );
    }

    /// Reconstruction reads the graph **at the instant asked for**, not the
    /// present.
    ///
    /// Two edits land at different `t`. Asking as of the later one must return
    /// the later value — which only holds if the store resolves each fact to
    /// its newest row at or before `t`, and if `get_asset_as_of` passes the `t`
    /// down at all. Drop `as_of` from the pattern and the whole history arrives
    /// at once, so the answer depends on scan order rather than on time.
    #[tokio::test]
    async fn reconstruction_at_the_present_returns_the_latest_value() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        for text in ["the first description", "the corrected description"] {
            catalog
                .update_asset(
                    &Principal::system(),
                    created.id,
                    &AssetUpdate {
                        description: Some(Some(text.to_string())),
                    },
                    None,
                )
                .await
                .expect("update");
        }

        let now = catalog
            .get_asset_as_of(created.id, Utc::now())
            .await
            .expect("reconstruct");

        assert_eq!(
            now.description.as_deref(),
            Some("the corrected description"),
            "the newest value at that instant, not the oldest one written"
        );
    }

    /// And the negative: as of an instant **before** the correction, the
    /// original stands. Without this the test above is satisfied by a store
    /// that ignores `as_of` and always answers with the present.
    #[tokio::test]
    async fn reconstruction_before_an_edit_returns_what_the_field_used_to_say() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("the first description".to_string())),
                },
                None,
            )
            .await
            .expect("first edit");

        let between = graph.clock.load(std::sync::atomic::Ordering::SeqCst);

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("the corrected description".to_string())),
                },
                None,
            )
            .await
            .expect("second edit");

        graph
            .at_resolves_to
            .store(between, std::sync::atomic::Ordering::SeqCst);

        let historical = catalog
            .get_asset_as_of(created.id, Utc::now())
            .await
            .expect("reconstruct");

        assert_eq!(
            historical.description.as_deref(),
            Some("the first description"),
            "history must be recoverable — this is the whole claim of the flake model"
        );
    }

    /// A **retracted** fact must not come back. Clearing a description writes a
    /// retraction, not a delete; a reconstruction that ignored `op` would keep
    /// serving a value the catalog no longer holds — worse than a missing
    /// field, because it looks authoritative.
    #[tokio::test]
    async fn reconstruction_does_not_resurrect_a_retracted_field() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("written then withdrawn".to_string())),
                },
                None,
            )
            .await
            .expect("set");

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(None),
                },
                None,
            )
            .await
            .expect("clear");

        let now = catalog
            .get_asset_as_of(created.id, Utc::now())
            .await
            .expect("reconstruct");

        assert_eq!(now.description, None, "the retraction must win");
    }

    /// Reconstructing one asset must **ask for one subject**.
    ///
    /// The returned value cannot catch this: `asset_from_flakes` filters by id
    /// anyway, so an unbounded scan produces exactly the right answer — after
    /// reading every fact in the graph. At 124 assets that is invisible; at
    /// 100k it is the difference between a SPOT point lookup and a full scan.
    /// The question asked is the only observable, so that is what is asserted.
    #[tokio::test]
    async fn reconstruction_asks_for_one_subject_not_the_whole_graph() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph.clone() as Arc<dyn TripleStore>);

        let mine = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");
        catalog
            .upsert_asset(&Principal::system(), service("someone-else"))
            .await
            .expect("create");

        graph.queried.lock().expect("lock").clear();
        catalog
            .get_asset_as_of(mine.id, Utc::now())
            .await
            .expect("reconstruct");

        let queried = graph.queried.lock().expect("lock");
        assert!(
            queried.iter().all(|pattern| pattern
                .s
                .as_ref()
                .is_some_and(|s| s.id == mine.id.to_string())),
            "every scan must be bound to the subject asked for: {queried:?}"
        );
        assert!(!queried.is_empty(), "it must have asked something at all");
    }

    /// **Decision 6, asserted rather than promised.** Failing an entity write
    /// because its graph projection failed would make the graph a single point
    /// of failure for the catalog — the exact coupling the split exists to
    /// avoid.
    #[tokio::test]
    async fn an_entity_write_survives_a_graph_that_is_down() {
        let catalog =
            Catalog::new(Arc::new(InMemoryStorage::default())).with_graph(RecordingGraph::broken());

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("the entity must be written even though the graph refused");

        assert_eq!(created.name, "hdfc-core");
        assert_eq!(
            catalog
                .get_asset(created.id)
                .await
                .expect("readable")
                .expect("the entity must still exist")
                .name,
            "hdfc-core",
            "and must still be there afterwards"
        );
    }

    #[tokio::test]
    async fn an_update_survives_a_graph_that_is_down() {
        let catalog =
            Catalog::new(Arc::new(InMemoryStorage::default())).with_graph(RecordingGraph::broken());
        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let updated = catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("core banking".to_string())),
                },
                None,
            )
            .await
            .expect("the update must land even though the graph refused");

        assert_eq!(updated.description.as_deref(), Some("core banking"));
    }

    /// A catalog with no graph configured must behave exactly as before —
    /// this is what makes the projection genuinely optional rather than
    /// optional-until-something-touches-it.
    #[tokio::test]
    async fn a_catalog_with_no_graph_still_writes() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("no graph configured is not an error");
    }

    #[tokio::test]
    async fn creating_an_asset_projects_its_fields() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);

        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        let asserted = graph.asserted.lock().expect("lock");
        assert!(!asserted.is_empty(), "a create must project something");
        assert!(
            asserted.iter().all(|f| f.s.id == created.id.to_string()),
            "every flake is about the asset just written"
        );
        assert!(
            asserted.iter().any(|f| f.p.id == "name"),
            "the name is the least a projection can carry: {asserted:?}"
        );
        assert!(
            graph.retracted.lock().expect("lock").is_empty(),
            "a create has nothing to retract"
        );
    }

    /// The update path must withdraw what it replaces. Asserting the new value
    /// without retracting the old leaves both current, and a single-valued
    /// predicate then has two answers.
    #[tokio::test]
    async fn updating_an_asset_retracts_the_value_it_replaces() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        let created = catalog
            .upsert_asset(&Principal::system(), service("hdfc-core"))
            .await
            .expect("create");

        catalog
            .update_asset(
                &Principal::system(),
                created.id,
                &AssetUpdate {
                    description: Some(Some("core banking".to_string())),
                },
                None,
            )
            .await
            .expect("update");

        let retracted = graph.retracted.lock().expect("lock");
        let asserted = graph.asserted.lock().expect("lock");
        assert!(
            asserted.iter().any(|f| f.p.id == "description"),
            "the new description must be asserted"
        );
        // The version and updatedAt change on every edit, so there is always
        // something to withdraw even when the edited field was previously
        // absent.
        assert!(
            retracted.iter().any(|f| f.p.id == "version"),
            "the superseded version must be withdrawn: {retracted:?}"
        );
    }
    mod authorization_decisions_are_cached {
        use super::*;
        use graph_owl_authz::{Effect, MetadataOperation, Policy, ResourceMatcher, Rule};

        fn policy(prefix: &str) -> Policy {
            Policy {
                name: "analyst".to_string(),
                rules: vec![Rule {
                    name: "read-hdfc".to_string(),
                    effect: Effect::Allow,
                    operations: vec![MetadataOperation::ViewBasic],
                    resources: ResourceMatcher::FqnPrefix(prefix.to_string()),
                }],
            }
        }

        fn analyst(roles: &[&str]) -> Principal {
            Principal {
                id: "asha".to_string(),
                name: "Asha".to_string(),
                kind: graph_owl_core::PrincipalKind::User,
                roles: roles.iter().map(ToString::to_string).collect(),
                is_admin: false,
            }
        }

        async fn catalog_with_policy() -> (Catalog, Arc<InMemoryStorage>) {
            let storage = Arc::new(InMemoryStorage::default());
            storage.policies.lock().unwrap().push(policy("hdfc-core"));
            let catalog = Catalog::new(storage.clone());
            catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("create");
            (catalog, storage)
        }

        fn page() -> PageRequest {
            PageRequest::new(None, None).expect("a default page")
        }

        fn reads(storage: &InMemoryStorage) -> usize {
            storage
                .policy_reads
                .load(std::sync::atomic::Ordering::SeqCst)
        }

        /// The point of the cache: the second identical question does not go
        /// back to storage. Invisible in the result — a cached and an uncached
        /// predicate are the same predicate — so the read count is the only
        /// observable.
        #[tokio::test]
        async fn a_repeated_question_does_not_reach_storage_twice() {
            let (catalog, storage) = catalog_with_policy().await;
            let asha = analyst(&["analyst"]);

            catalog
                .list_assets_for(&asha, None, &page())
                .await
                .expect("first");
            let after_first = reads(&storage);
            catalog
                .list_assets_for(&asha, None, &page())
                .await
                .expect("second");

            assert_eq!(
                reads(&storage),
                after_first,
                "the second read was served from cache"
            );
            assert!(after_first > 0, "the first must actually have asked");
        }

        /// **The property that makes the cache safe to have.** Two principals
        /// with different roles must never share a decision. A key that
        /// ignored roles would hand one reader the other's visibility, which is
        /// the worst failure this component can have and is silent.
        #[tokio::test]
        async fn a_different_role_set_is_a_different_decision() {
            let (catalog, _storage) = catalog_with_policy().await;

            let permitted = catalog
                .list_assets_for(&analyst(&["analyst"]), None, &page())
                .await
                .expect("permitted");
            let unpermitted = catalog
                .list_assets_for(&analyst(&["nobody"]), None, &page())
                .await
                .expect("unpermitted");

            assert_eq!(permitted.data.len(), 1, "the analyst role allows hdfc-core");
            assert!(
                unpermitted.data.is_empty(),
                "a role with no policy sees nothing, cached or not: {:?}",
                unpermitted.data
            );
        }

        /// And an admin must not be served a non-admin's entry. `compile`
        /// short-circuits on the flag, so the two differ even with identical
        /// roles.
        #[tokio::test]
        async fn an_admin_is_not_served_a_non_admins_decision() {
            let (catalog, _storage) = catalog_with_policy().await;

            let restricted = catalog
                .list_assets_for(&analyst(&["nobody"]), None, &page())
                .await
                .expect("restricted");
            assert!(restricted.data.is_empty());

            let mut admin = analyst(&["nobody"]);
            admin.is_admin = true;
            let full = catalog
                .list_assets_for(&admin, None, &page())
                .await
                .expect("admin");

            assert_eq!(full.data.len(), 1, "an admin bypasses policy");
        }

        /// Invalidation is what makes a revocation take effect. Without it the
        /// cache is a window in which a withdrawn permission still works.
        #[tokio::test]
        async fn invalidation_sends_the_next_question_back_to_storage() {
            let (catalog, storage) = catalog_with_policy().await;
            let asha = analyst(&["analyst"]);
            catalog
                .list_assets_for(&asha, None, &page())
                .await
                .expect("first");
            let after_first = reads(&storage);

            catalog.invalidate_authorization();
            catalog
                .list_assets_for(&asha, None, &page())
                .await
                .expect("second");

            assert!(
                reads(&storage) > after_first,
                "after invalidation the decision must be recomputed"
            );
        }

        /// A revoked policy actually takes effect after invalidation — the
        /// behaviour, not just the read count.
        #[tokio::test]
        async fn a_revoked_policy_stops_working_once_invalidated() {
            let (catalog, storage) = catalog_with_policy().await;
            let asha = analyst(&["analyst"]);
            assert_eq!(
                catalog
                    .list_assets_for(&asha, None, &page())
                    .await
                    .expect("before")
                    .data
                    .len(),
                1
            );

            storage.policies.lock().unwrap().clear();
            catalog.invalidate_authorization();

            assert!(
                catalog
                    .list_assets_for(&asha, None, &page())
                    .await
                    .expect("after")
                    .data
                    .is_empty(),
                "the withdrawn policy must no longer admit anything"
            );
        }
    }

    mod committed_changes_are_announced {
        use super::*;
        use graph_owl_events::{ChangeEvent, EventKind, EventSink};

        #[derive(Default)]
        struct Recording(Mutex<Vec<ChangeEvent>>);

        impl EventSink for Recording {
            fn emit(&self, event: &ChangeEvent) {
                self.0.lock().expect("lock").push(event.clone());
            }
        }

        impl Recording {
            fn kinds(&self) -> Vec<EventKind> {
                self.0
                    .lock()
                    .expect("lock")
                    .iter()
                    .map(|e| e.kind)
                    .collect()
            }
        }

        async fn catalog_with_sink() -> (Catalog, Arc<Recording>, Uuid) {
            let storage = Arc::new(InMemoryStorage::default());
            let sink = Arc::new(Recording::default());
            let catalog = Catalog::new(storage).with_events(sink.clone());
            let asset = catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("created");
            sink.0.lock().expect("lock").clear();
            (catalog, sink, asset.id)
        }

        fn describe(text: &str) -> AssetUpdate {
            AssetUpdate {
                description: Some(Some(text.to_string())),
            }
        }

        #[tokio::test]
        async fn creating_an_asset_announces_a_creation() {
            let storage = Arc::new(InMemoryStorage::default());
            let sink = Arc::new(Recording::default());
            let catalog = Catalog::new(storage).with_events(sink.clone());

            let created = catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("created");

            assert_eq!(sink.kinds(), vec![EventKind::Created]);
            let events = sink.0.lock().expect("lock");
            assert_eq!(events[0].subject.id, created.id.to_string());
            assert_eq!(events[0].subject.fqn, created.fully_qualified_name);
            assert_eq!(
                events[0].previous_version, None,
                "there was nothing before a creation"
            );
            assert_eq!(events[0].current_version, Some(created.version));
        }

        /// A connector re-run over an FQN that already exists is an **update**,
        /// not a second creation. `upsert_asset` is one method serving both, so
        /// the distinction has to be drawn from prior state rather than from
        /// which method was called — and a search index told "created" twice
        /// would hold two documents for one table.
        #[tokio::test]
        async fn a_re_ingest_that_changes_a_field_announces_an_update_not_a_creation() {
            let storage = Arc::new(InMemoryStorage::default());
            let sink = Arc::new(Recording::default());
            let catalog = Catalog::new(storage).with_events(sink.clone());

            catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("created");
            sink.0.lock().expect("lock").clear();

            let described = UpsertAsset {
                description: Some("the core banking platform".to_string()),
                ..service("hdfc-core")
            };
            catalog
                .upsert_asset(&Principal::system(), described)
                .await
                .expect("re-ingested");

            assert_eq!(sink.kinds(), vec![EventKind::Updated]);
            assert!(
                !sink.0.lock().expect("lock")[0].change.is_empty(),
                "an update must say what moved"
            );
        }

        /// **The negative that matters most.** A nightly connector re-run over
        /// an unchanged estate must announce nothing at all. Without this, every
        /// asset is republished every night and the event stream stops meaning
        /// "something changed" — which is the only thing it is for.
        #[tokio::test]
        async fn a_re_ingest_that_changes_nothing_announces_nothing() {
            let storage = Arc::new(InMemoryStorage::default());
            let sink = Arc::new(Recording::default());
            let catalog = Catalog::new(storage).with_events(sink.clone());

            catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("created");
            sink.0.lock().expect("lock").clear();

            catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("re-ingested");

            assert!(
                sink.kinds().is_empty(),
                "an unchanged re-ingest is not a change: {:?}",
                sink.kinds()
            );
        }

        /// A create refused by validation never reached storage, so there is
        /// nothing to announce. Emission sits past the early returns, which is
        /// what makes this structural rather than a check.
        #[tokio::test]
        async fn a_refused_create_announces_nothing() {
            let storage = Arc::new(InMemoryStorage::default());
            let sink = Arc::new(Recording::default());
            let catalog = Catalog::new(storage).with_events(sink.clone());

            let orphan = UpsertAsset {
                kind: AssetKind::Table,
                name: "customers".to_string(),
                parent_id: None,
                description: None,
                properties: None,
            };
            let outcome = catalog.upsert_asset(&Principal::system(), orphan).await;

            assert!(outcome.is_err(), "a table requires a parent");
            assert!(sink.kinds().is_empty());
        }

        #[tokio::test]
        async fn an_update_announces_one_updated_event_naming_the_asset() {
            let (catalog, sink, id) = catalog_with_sink().await;
            catalog
                .update_asset(&Principal::system(), id, &describe("now described"), None)
                .await
                .expect("updated");

            assert_eq!(sink.kinds(), vec![EventKind::Updated]);
            let events = sink.0.lock().expect("lock");
            assert_eq!(events[0].subject.id, id.to_string());
            assert_eq!(events[0].principal_id, Principal::system().id);
        }

        #[tokio::test]
        async fn an_update_that_changes_nothing_announces_nothing() {
            let (catalog, sink, id) = catalog_with_sink().await;
            let _ = catalog
                .update_asset(&Principal::system(), id, &AssetUpdate::default(), None)
                .await;

            assert!(sink.kinds().is_empty());
        }

        #[tokio::test]
        async fn a_failed_update_announces_nothing_because_emission_follows_the_write() {
            let (catalog, sink, _) = catalog_with_sink().await;

            let outcome = catalog
                .update_asset(&Principal::system(), Uuid::new_v4(), &describe("x"), None)
                .await;

            assert!(outcome.is_err());
            assert!(
                sink.kinds().is_empty(),
                "a change that did not commit must not be announced"
            );
        }

        #[tokio::test]
        async fn a_soft_delete_and_a_restore_are_distinct_kinds_not_updates() {
            let (catalog, sink, id) = catalog_with_sink().await;
            catalog
                .soft_delete_asset(&Principal::system(), id)
                .await
                .expect("deleted");
            catalog
                .restore_asset(&Principal::system(), id)
                .await
                .expect("restored");

            assert_eq!(
                sink.kinds(),
                vec![EventKind::SoftDeleted, EventKind::Restored]
            );
        }

        #[tokio::test]
        async fn a_failed_delete_announces_nothing() {
            let (catalog, sink, _) = catalog_with_sink().await;

            assert!(
                catalog
                    .soft_delete_asset(&Principal::system(), Uuid::new_v4())
                    .await
                    .is_err()
            );
            assert!(sink.kinds().is_empty());
        }

        #[tokio::test]
        async fn a_catalog_with_no_sink_still_mutates() {
            let storage = Arc::new(InMemoryStorage::default());
            let catalog = Catalog::new(storage);
            let asset = catalog
                .upsert_asset(&Principal::system(), service("svc"))
                .await
                .expect("created");

            catalog
                .update_asset(&Principal::system(), asset.id, &describe("x"), None)
                .await
                .expect("a missing subscriber is not an outage");
        }
    }
}
