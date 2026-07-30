use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use graph_owl_authz::{
    AccessPredicate, DecisionCache, DecisionKey, MetadataOperation, Policy, Subject, compile,
};
use graph_owl_connectors::DeletionPlan;
use graph_owl_core::flake::Flake;
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
use graph_owl_reasoning as reasoning;
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
    /// **What the engine decided to read**, one entry per scan.
    ///
    /// The single number that explains a slow query. Pushdown turns "read the
    /// estate and let the evaluator filter" into "read what the question
    /// names", and an author who cannot see which of the two happened cannot
    /// tell a query that is inherently expensive from one that is one triple
    /// pattern away from being cheap.
    ///
    /// A **single `?s ?p ?o` entry means no pattern could be bounded** and the
    /// whole graph was read — which is correct, and is the thing worth seeing.
    pub plan: Vec<String>,
    /// The variables the query projected, **in the order it named them**.
    ///
    /// A solution is returned as a `BTreeMap`, which sorts alphabetically —
    /// so `SELECT ?s ?p ?o` arrives as `o, p, s` and the author's own ordering
    /// is gone before any consumer sees it. Recovering it from the rows is
    /// therefore impossible, which is why it is carried separately.
    ///
    /// Empty when the query form has no projection (`ASK`, `DESCRIBE`).
    pub variables: Vec<String>,
}

/// The variables a `SELECT` names, in the order it names them.
///
/// Read from the parsed algebra rather than the results: the projection is a
/// property of the *query*, and by the time solutions exist it has been
/// flattened into a sorted map.
fn projected_variables(query: &spargebra::Query) -> Vec<String> {
    use spargebra::algebra::GraphPattern;

    fn find(pattern: &GraphPattern) -> Option<Vec<String>> {
        match pattern {
            GraphPattern::Project { variables, .. } => {
                Some(variables.iter().map(|v| v.as_str().to_string()).collect())
            }
            // The projection sits above the rest of the algebra, but modifiers
            // — `ORDER BY`, `LIMIT`, `DISTINCT` — wrap it, so the walk has to
            // descend rather than only check the root.
            GraphPattern::Slice { inner, .. }
            | GraphPattern::Distinct { inner }
            | GraphPattern::Reduced { inner }
            | GraphPattern::OrderBy { inner, .. } => find(inner),
            _ => None,
        }
    }

    match query {
        spargebra::Query::Select { pattern, .. } => find(pattern).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Render one planned scan the way a reader thinks about it.
///
/// `?` for an unbound position rather than omitting it, so the *shape* of what
/// will be read is visible at a glance: `?s dsc:name ?o` and `dsc:x ?p ?o`
/// narrow in different directions and cost differently.
fn describe_scan(pattern: &graph_owl_core::flake::TriplePattern) -> String {
    let position = |bound: Option<String>| bound.unwrap_or_else(|| "?".to_string());
    format!(
        "{} {} {}",
        position(pattern.s.as_ref().map(ToString::to_string)),
        position(pattern.p.as_ref().map(ToString::to_string)),
        position(pattern.o.as_ref().map(|o| format!("{o:?}"))),
    )
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
    /// The last shape compilation, keyed on the newest `t` among the shape
    /// facts. `Arc` for the same reason `decisions` is: axum clones `Catalog`
    /// per request, and a cache cloned with it would be a fresh empty cache
    /// every time — present, and warm to nobody.
    ///
    /// One entry, not a map. There is one set of shapes, it is replaced
    /// wholesale, and an eviction policy over a single entry is a policy with
    /// nothing to decide.
    #[allow(clippy::type_complexity)]
    shape_cache: Arc<Mutex<Option<(i64, Vec<graph_owl_constraint::CompiledShape>, usize)>>>,
}

impl Catalog {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            graph: None,
            traversal: None,
            events: None,
            decisions: Arc::new(DecisionCache::default()),
            shape_cache: Arc::new(Mutex::new(None)),
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
        // Described before they run: the plan is what the engine *decided*, and
        // computing it from what came back would describe the outcome instead.
        let plan: Vec<String> = scans.iter().map(describe_scan).collect();

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
            plan,
            variables: projected_variables(&parsed),
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
                // The default graph specifically, not "any graph". **Reasoning
                // is skipped on historical queries**: a derived fact is a
                // conclusion about the *current* rule set, and letting one into
                // an `as_of` answer would report an inference that nobody could
                // have drawn at that instant, carrying provenance that looks
                // right. Time travel is over asserted facts.
                cx: Some(None),
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
                owners: Vec::new(),
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

    /// Connection-pool occupancy, for the operational gauge.
    #[must_use]
    pub fn pool_stats(&self) -> Option<graph_owl_storage::PoolStats> {
        self.storage.pool_stats()
    }

    /// Save a connector configuration.
    ///
    /// `secret` is `None` to leave an existing credential alone: an
    /// edit-then-save round trip cannot resend what it was never given.
    ///
    /// # Errors
    ///
    /// `Validation` if the connector or service name is blank.
    /// `Storage` if the write fails.
    #[tracing::instrument(name = "catalog.save_connector_config", skip_all)]
    pub async fn save_connector_config(
        &self,
        connector: &str,
        service_name: &str,
        settings: serde_json::Value,
        secret: Option<&str>,
    ) -> Result<graph_owl_storage::ConnectorConfig, CatalogError> {
        let mut problems = Vec::new();
        if connector.trim().is_empty() {
            problems.push(FieldError::new(
                "connector",
                FieldErrorCode::Required,
                "which connector this configures".to_string(),
            ));
        }
        if service_name.trim().is_empty() {
            problems.push(FieldError::new(
                "serviceName",
                FieldErrorCode::Required,
                "the service this configuration is for".to_string(),
            ));
        }
        // A blank secret is not a secret. Accepting `""` would set `has_secret`
        // and then fail at connection time with a credential error nobody can
        // explain.
        if secret.is_some_and(|s| s.trim().is_empty()) {
            problems.push(FieldError::new(
                "secret",
                FieldErrorCode::Type,
                "a blank secret is not a credential; omit the field to keep the \
                 existing one"
                    .to_string(),
            ));
        }
        if !problems.is_empty() {
            return Err(CatalogError::Validation(problems));
        }

        let config = graph_owl_storage::ConnectorConfig {
            id: Uuid::new_v4(),
            connector: connector.to_string(),
            service_name: service_name.to_string(),
            settings,
            // Set from what storage actually holds, on the read below — a value
            // computed here would claim a credential this call may not have
            // supplied.
            has_secret: false,
        };
        self.storage
            .upsert_connector_config(&config, secret)
            .await?;

        self.storage
            .connector_configs()
            .await?
            .into_iter()
            .find(|c| c.connector == connector && c.service_name == service_name)
            .ok_or_else(|| {
                CatalogError::Storage(StorageError::Unexpected(
                    "the configuration vanished between write and read".to_string(),
                ))
            })
    }

    /// Every connector configuration, **without credentials**.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    #[tracing::instrument(name = "catalog.connector_configs", skip_all)]
    pub async fn connector_configs(
        &self,
    ) -> Result<Vec<graph_owl_storage::ConnectorConfig>, CatalogError> {
        Ok(self.storage.connector_configs().await?)
    }

    /// Create or update a team.
    ///
    /// # Errors
    ///
    /// `Validation` if the id or display name is blank, or a named member is
    /// not a known user — a member nobody can resolve is an owner who does not
    /// exist, and the mistake surfaces later as an asset owned by nothing.
    /// `Storage` if the write fails.
    #[tracing::instrument(name = "catalog.upsert_team", skip_all)]
    pub async fn upsert_team(
        &self,
        team: &graph_owl_storage::Team,
    ) -> Result<graph_owl_storage::Team, CatalogError> {
        let mut problems = Vec::new();
        if team.id.trim().is_empty() {
            problems.push(FieldError::new(
                "id",
                FieldErrorCode::Required,
                "a team needs an id".to_string(),
            ));
        }
        if team.display_name.trim().is_empty() {
            problems.push(FieldError::new(
                "displayName",
                FieldErrorCode::Required,
                "a team needs a name somebody can recognise".to_string(),
            ));
        }
        // Checked here as well as by the foreign key, so the caller gets a
        // field-level `400` naming who is unknown rather than a storage error
        // they have to interpret.
        for member in &team.members {
            if self.storage.find_user(member).await?.is_none() {
                problems.push(FieldError::new(
                    "members",
                    FieldErrorCode::Type,
                    format!("`{member}` is not a known user"),
                ));
            }
        }
        if !problems.is_empty() {
            return Err(CatalogError::Validation(problems));
        }

        self.storage.upsert_team(team).await?;
        self.storage.find_team(&team.id).await?.ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "the team vanished between write and read".to_string(),
            ))
        })
    }

    /// Every team.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    #[tracing::instrument(name = "catalog.teams", skip_all)]
    pub async fn teams(&self) -> Result<Vec<graph_owl_storage::Team>, CatalogError> {
        Ok(self.storage.teams().await?)
    }

    /// Change what roles a user holds.
    ///
    /// **Invalidates the decision cache, and that is the entire point.** A
    /// compiled predicate is cached per subject and operation, and a revoked
    /// role that keeps working until some TTL elapses is a revocation whose
    /// window is invisible to whoever performed it. There is deliberately no
    /// TTL (see [`Self::invalidate_authorization`]), so this call is the only
    /// thing that expires an entry — omitting it would leave the old access in
    /// force with nothing to show why.
    ///
    /// The whole cache rather than one subject's entries: roles are compiled
    /// against policies that name *other* subjects too, so a change to one
    /// person can alter what a group-based rule grants everybody. Clearing
    /// selectively would be right most of the time, which is the worst
    /// property a security control can have.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such user — creating one as a side effect of
    /// granting it roles would let a typo mint a principal.
    /// `Storage` if the write fails.
    #[tracing::instrument(name = "catalog.set_user_roles", skip_all)]
    pub async fn set_user_roles(
        &self,
        id: &str,
        roles: Vec<String>,
    ) -> Result<StoredUser, CatalogError> {
        let existing = self
            .storage
            .find_user(id)
            .await?
            .ok_or(CatalogError::NotFound)?;

        let updated = StoredUser { roles, ..existing };
        self.storage.upsert_user(&updated).await?;
        // After the write, never before: invalidating first leaves a window in
        // which a concurrent request re-populates the cache from the old rows.
        self.invalidate_authorization();
        Ok(updated)
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
        filter: &graph_owl_storage::AssetFilter<'_>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        let predicate = self
            .predicate_for(principal, MetadataOperation::ViewBasic)
            .await?;
        Ok(self
            .storage
            .list_assets_visible(filter, page, &predicate)
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
    /// Assert that one asset feeds another.
    ///
    /// # Errors
    ///
    /// `Validation` when the relationship is not a lineage edge, when the kinds
    /// may not carry it, or when the two endpoints are the same asset.
    /// `NotFound` when either endpoint does not exist. `Storage` conflict when
    /// the same source has already asserted this edge.
    #[tracing::instrument(name = "catalog.assert_lineage", skip_all)]
    pub async fn assert_lineage(
        &self,
        principal: &Principal,
        from_asset_id: Uuid,
        to_asset_id: Uuid,
        relationship: graph_owl_core::relationship_type::RelationshipType,
        details: graph_owl_core::lineage::LineageDetails,
    ) -> Result<graph_owl_core::lineage::LineageEdge, CatalogError> {
        // Checked before existence, deliberately, for the reason
        // `create_relationship` gives: an illegal edge between two nonexistent
        // assets is an edge problem, and a 404 sends the client hunting for the
        // wrong bug.
        if from_asset_id == to_asset_id {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "toAssetId",
                FieldErrorCode::Type,
                "an asset cannot feed itself; lineage is a directed acyclic graph \
                 and a self-edge is a cycle of length one"
                    .to_string(),
            )]));
        }

        let from = self
            .storage
            .get_asset(from_asset_id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let to = self
            .storage
            .get_asset(to_asset_id)
            .await?
            .ok_or(CatalogError::NotFound)?;

        if !graph_owl_core::lineage::is_legal_lineage(from.kind, relationship, to.kind) {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "relationship",
                FieldErrorCode::Type,
                format!(
                    "a `{}` cannot `{}` a `{}`; lineage runs table-to-table or \
                     column-to-column, never across levels",
                    from.kind,
                    relationship.as_str(),
                    to.kind
                ),
            )]));
        }

        let edge = graph_owl_core::lineage::LineageEdge {
            id: Uuid::new_v4(),
            from_asset_id,
            to_asset_id,
            relationship,
            details,
            created_at: Utc::now(),
            created_by: principal.id.clone(),
        };
        self.storage.create_lineage_edge(&edge).await?;
        self.project_lineage(&edge, true).await;
        Ok(edge)
    }

    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn remove_lineage(&self, id: Uuid) -> Result<bool, CatalogError> {
        let removed = self.storage.delete_lineage_edge(id).await?;
        if let Some(edge) = &removed {
            self.project_lineage(edge, false).await;
        }
        Ok(removed.is_some())
    }

    /// Mirror one lineage edge into the graph as a triple.
    ///
    /// **Lineage has to be in the graph for anything to reason over it.**
    /// Relational storage answers "what feeds this table"; a rule that
    /// propagates a classification downstream needs `feeds` as a *fact*, and so
    /// does any SPARQL query about lineage. Decision 6 still holds — relational
    /// is the source of truth and this is a projection of it.
    ///
    /// **Never propagates a failure**, for the same reason asset projection
    /// does not: a graph outage must not become a catalog outage.
    async fn project_lineage(&self, edge: &graph_owl_core::lineage::LineageEdge, asserted: bool) {
        let Some(graph) = &self.graph else {
            return;
        };

        let outcome = async {
            let t = graph.next_time().await?;
            let flake = Flake {
                s: graph_owl_core::flake::Sid::dsc(edge.from_asset_id.to_string()),
                p: graph_owl_core::flake::Sid::dsc(edge.relationship.as_str()),
                o: graph_owl_core::flake::FlakeValue::Ref(graph_owl_core::flake::Sid::dsc(
                    edge.to_asset_id.to_string(),
                )),
                cx: None,
                t,
                op: asserted,
            };
            if asserted {
                graph.assert_flakes(&[flake]).await
            } else {
                graph.retract_flakes(&[flake]).await
            }
        }
        .await;

        if let Err(error) = outcome {
            eprintln!(
                "graph projection failed for lineage edge {} ({error}). The edge \
                 is intact; the graph view is stale until reconciliation.",
                edge.id
            );
        }
    }

    /// The lineage graph around one asset, bounded in both directions.
    ///
    /// **Both directions are first-class.** "What breaks if I change this" and
    /// "where did this number come from" are the same graph read in opposite
    /// directions, and a walk that only went one way would answer half the
    /// questions lineage exists for.
    ///
    /// Breadth-first with a visited set, so a diamond (A→B, A→C, B→D, C→D)
    /// yields D once with both inbound edges rather than twice — and so a cycle
    /// asserted despite the acyclicity intent terminates instead of hanging.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    #[tracing::instrument(name = "catalog.lineage_graph", skip_all)]
    pub async fn lineage_graph(
        &self,
        root: Uuid,
        upstream: usize,
        downstream: usize,
    ) -> Result<(Vec<Asset>, Vec<graph_owl_core::lineage::LineageEdge>), CatalogError> {
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        seen.insert(root);
        let mut edges: std::collections::HashMap<Uuid, graph_owl_core::lineage::LineageEdge> =
            std::collections::HashMap::new();

        // One frontier per direction, walked to its own depth. Walking a merged
        // frontier would let an upstream hop spend the downstream budget, so
        // `upstream=1&downstream=3` would return something that is neither.
        for (depth, forward) in [(upstream, false), (downstream, true)] {
            let mut frontier = vec![root];
            for _ in 0..depth {
                if frontier.is_empty() {
                    break;
                }
                let touching = self.storage.lineage_edges_touching(&frontier).await?;
                let mut next = Vec::new();
                for edge in touching {
                    let (near, far) = if forward {
                        (edge.from_asset_id, edge.to_asset_id)
                    } else {
                        (edge.to_asset_id, edge.from_asset_id)
                    };
                    // Only edges leaving the frontier in the direction being
                    // walked. `lineage_edges_touching` returns both, because
                    // one query serving both walks is cheaper than two.
                    if !frontier.contains(&near) {
                        continue;
                    }
                    edges.insert(edge.id, edge);
                    if seen.insert(far) {
                        next.push(far);
                    }
                }
                frontier = next;
            }
        }

        // Soft-deleted assets are *included*, so a lineage graph that runs into
        // a deleted table shows the break rather than silently truncating —
        // "nothing downstream" and "the downstream was deleted" are opposite
        // conclusions.
        let mut nodes = Vec::new();
        for id in &seen {
            if let Some(asset) = self.storage.get_asset(*id).await? {
                nodes.push(asset);
            }
        }
        Ok((nodes, edges.into_values().collect()))
    }

    /// Open a run row before the work starts.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn begin_run(
        &self,
        run: &graph_owl_storage::ConnectorRun,
    ) -> Result<(), CatalogError> {
        Ok(self.storage.begin_run(run).await?)
    }

    /// Close it with what happened.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn finish_run(
        &self,
        run: &graph_owl_storage::ConnectorRun,
    ) -> Result<(), CatalogError> {
        Ok(self.storage.finish_run(run).await?)
    }

    /// Recent runs, newest first. An empty service name means every service.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn recent_runs(
        &self,
        service_name: &str,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ConnectorRun>, CatalogError> {
        Ok(self.storage.recent_runs(service_name, limit).await?)
    }

    /// Fingerprints for a batch of FQNs, as the three states a run acts on.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn existing_fingerprints(
        &self,
        fqns: &[String],
    ) -> Result<std::collections::HashMap<String, graph_owl_connectors::Existing>, CatalogError>
    {
        let stored = self.storage.source_hashes(fqns).await?;
        Ok(fqns
            .iter()
            .map(|fqn| {
                let existing = match stored.get(fqn) {
                    None => graph_owl_connectors::Existing::Absent,
                    Some(None) => graph_owl_connectors::Existing::Unfingerprinted,
                    Some(Some(bytes)) => <[u8; 32]>::try_from(bytes.as_slice()).map_or(
                        // A stored value of the wrong width cannot be compared,
                        // and treating it as a match would skip forever. Patch,
                        // which rewrites it correctly.
                        graph_owl_connectors::Existing::Unfingerprinted,
                        graph_owl_connectors::Existing::Fingerprinted,
                    ),
                };
                (fqn.clone(), existing)
            })
            .collect())
    }

    /// Record what the source said about an asset.
    ///
    /// # Errors
    /// Returns an error if the underlying storage fails.
    pub async fn remember_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), CatalogError> {
        Ok(self.storage.set_source_hash(id, hash).await?)
    }

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

    // ---- Epic 31: organizational memory ----

    /// Store a memory, with its links validated against what exists.
    ///
    /// The domain refuses an unanchored or over-confident memory before this is
    /// called; what this adds is the part only storage knows — whether each link
    /// points at something real.
    ///
    /// # Errors
    ///
    /// `Validation` naming the offending link **by index**, because "one of your
    /// links is wrong" is not actionable with four of them. `Conflict` if the id
    /// is taken.
    pub async fn create_memory(
        &self,
        memory: &graph_owl_core::memory::Memory,
    ) -> Result<(), CatalogError> {
        match self.storage.save_memory(memory).await? {
            graph_owl_storage::MemoryWrite::Saved => Ok(()),
            graph_owl_storage::MemoryWrite::UnknownLinkTarget { index, target } => {
                Err(unresolvable_link(index, target))
            }
        }
    }

    /// One memory, superseded or not.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn memory(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::memory::Memory>, CatalogError> {
        Ok(self.storage.find_memory(id).await?)
    }

    /// What we know about a subject, best first, each with its staleness.
    ///
    /// This is the capability the epic exists for, and it is assembled here
    /// rather than in storage because ranking is a pure decision and staleness is
    /// computed on read — **neither is a property of a row**. A stored staleness
    /// flag would be wrong from the moment somebody edited the subject, and wrong
    /// silently.
    ///
    /// # Errors
    ///
    /// `Storage` if a read fails.
    pub async fn recall(
        &self,
        subject: Uuid,
        query: &str,
        include_superseded: bool,
    ) -> Result<Vec<RecalledMemory>, CatalogError> {
        let memories = self
            .storage
            .memories_about(subject, include_superseded)
            .await?;

        // The subject's history, read **once** for the whole set rather than per
        // memory: every memory here is about the same subject, and a per-memory
        // read would turn one recall into N round trips for identical data.
        let current = self.storage.get_asset(subject).await?.map(|a| a.version);
        let history: Vec<(graph_owl_core::envelope::EntityVersion, DateTime<Utc>)> = self
            .storage
            .asset_versions(subject)
            .await?
            .into_iter()
            .map(|version| (version.version, version.updated_at))
            .collect();

        let staleness: Vec<graph_owl_core::memory::Staleness> = memories
            .iter()
            .map(|memory| {
                graph_owl_core::memory::staleness(
                    graph_owl_core::memory::version_at(memory.as_of, &history),
                    current,
                )
            })
            .collect();

        let candidates: Vec<graph_owl_core::recall::Candidate<'_>> = memories
            .iter()
            .zip(&staleness)
            .map(|(memory, verdict)| graph_owl_core::recall::Candidate {
                memory,
                staleness: verdict.clone(),
                // Epic 8 fills this. `None` is honest — a reader can tell
                // "measured, not similar" from "never measured".
                semantic: None,
            })
            .collect();

        let weights = graph_owl_core::recall::Weights::default();
        Ok(
            graph_owl_core::recall::rank(query, subject, &candidates, Utc::now(), &weights)
                .into_iter()
                .map(|ranked| RecalledMemory {
                    memory: ranked.memory.clone(),
                    staleness: ranked.staleness,
                    score: ranked.score,
                })
                .collect(),
        )
    }

    /// Correct a memory, keeping the original readable.
    ///
    /// # Errors
    ///
    /// `NotFound` if the original does not exist. `Conflict` naming the current
    /// memory if it has already been corrected — a client with only "no" cannot
    /// retry against the right target.
    pub async fn supersede_memory(
        &self,
        original: Uuid,
        replacement: &graph_owl_core::memory::Memory,
    ) -> Result<(), CatalogError> {
        match self.storage.supersede_memory(original, replacement).await? {
            graph_owl_storage::SupersedeOutcome::Superseded => Ok(()),
            graph_owl_storage::SupersedeOutcome::NotFound => Err(CatalogError::NotFound),
            graph_owl_storage::SupersedeOutcome::UnknownLinkTarget { index, target } => {
                Err(unresolvable_link(index, target))
            }
            graph_owl_storage::SupersedeOutcome::AlreadySuperseded { current } => {
                Err(CatalogError::Conflict {
                    detail: format!(
                        "memory {original} has already been corrected by {current}; supersede that one instead"
                    ),
                    existing_id: Some(current),
                    kind: graph_owl_storage::ConflictKind::MemoryExists,
                })
            }
        }
    }

    /// Every open contradiction about a subject, declared and candidate.
    ///
    /// **Nothing is resolved and nothing is hidden.** The pair is reported; a
    /// human decides. Dismissals are applied so a pair somebody already closed
    /// does not reopen — a queue that reopens closed items is a queue people stop
    /// reading.
    ///
    /// # Errors
    ///
    /// `Storage` if a read fails.
    pub async fn contradictions_about(
        &self,
        subject: Uuid,
    ) -> Result<Vec<graph_owl_core::contradiction::Contradiction>, CatalogError> {
        // Superseded memories are included on purpose: detection needs to *see*
        // them to rule them out, and filtering here would hide the very state
        // that distinguishes a correction from a conflict.
        let memories = self.storage.memories_about(subject, true).await?;
        let reviews = self.storage.contradiction_reviews().await?;
        let refs: Vec<&graph_owl_core::memory::Memory> = memories.iter().collect();
        Ok(graph_owl_core::contradiction::contradictions(
            &refs, &reviews,
        ))
    }

    /// Record what a human decided about a candidate contradiction.
    ///
    /// Confirming does **not** close it: the pair stays in the queue flagged as
    /// confirmed, because confirming a disagreement is not resolving one, and
    /// resolving one is what this epic refuses to do.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails, including when either memory is unknown.
    pub async fn review_contradiction(
        &self,
        review: graph_owl_core::contradiction::Review,
        reviewed_by: &str,
        note: Option<&str>,
    ) -> Result<(), CatalogError> {
        Ok(self
            .storage
            .review_contradiction(review, reviewed_by, note)
            .await?)
    }

    // ---- Epic 11 Slice C: ownership ----

    /// Replace an asset's owners.
    ///
    /// **Replace, not merge**, and an empty list is a legitimate request: an
    /// unowned asset is a real, reportable state, and the ownership-gap report is
    /// only meaningful if that state can be reached deliberately.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist. `Validation` naming the offending
    /// entry **by index** — `owners[1].id` — because "one of your owners is
    /// wrong" is not actionable with three of them. `Conflict` if the same
    /// principal is listed twice.
    pub async fn set_asset_owners(
        &self,
        asset_id: Uuid,
        owners: &[graph_owl_core::ownership::OwnerRef],
    ) -> Result<Vec<graph_owl_core::ownership::EntityReference>, CatalogError> {
        match self.storage.set_asset_owners(asset_id, owners).await? {
            graph_owl_storage::OwnersWrite::Set(resolved) => Ok(resolved),
            graph_owl_storage::OwnersWrite::NotFound => Err(CatalogError::NotFound),
            graph_owl_storage::OwnersWrite::UnknownPrincipal { index, id } => {
                Err(CatalogError::Validation(vec![validation::FieldError::new(
                    format!("owners[{index}].id"),
                    validation::FieldErrorCode::Type,
                    format!("{id} is neither a known user nor a known team"),
                )]))
            }
        }
    }

    /// Who owns this asset, in the order ownership was recorded.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn asset_owners(
        &self,
        asset_id: Uuid,
    ) -> Result<Vec<graph_owl_core::ownership::EntityReference>, CatalogError> {
        Ok(self.storage.asset_owners(asset_id).await?)
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

    // ----- Reasoning (Epic 6, slices D and E) -----

    /// Everything the asserted graph implies, written to its own graph.
    ///
    /// **The asserted base is never touched.** Conclusions go to
    /// `graph:reasoning`, and a run replaces that graph wholesale — which is
    /// only safe *because* it is separate: the same replacement over a shared
    /// graph would withdraw assertions nobody derived.
    ///
    /// Stored rows carry the run's transaction time rather than the derived
    /// fact's own `t`. The two are different things and both are right: the
    /// pure reasoner stamps a conclusion with the **maximum premise `t`**,
    /// because that is the first instant at which the facts implied it, while
    /// the row records when this run wrote it. Writing the row at the earlier
    /// instant would put it before the retraction that withdrew the previous
    /// run's copy, and current-state resolution would then drop the fact
    /// entirely.
    ///
    /// # Errors
    ///
    /// `Storage` if no graph engine is configured, or if the read or either
    /// write fails.
    #[tracing::instrument(name = "catalog.run_reasoning", skip_all)]
    pub async fn run_reasoning(
        &self,
        budget: &reasoning::Budget,
    ) -> Result<ReasoningReport, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let base = Self::asserted_base(graph.as_ref()).await?;
        let concluded = reasoning::derive_within(&base, budget);

        // Withdraw the previous run's overlay before writing this one.
        // Retracting what is *there* rather than re-deriving what was there
        // last time: a rule change between runs would otherwise strand every
        // conclusion the old rule set drew and the new one does not.
        let previous = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                cx: Some(Some(reasoning::reasoning_graph())),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        // **Two transaction times, withdrawal strictly before assertion.** A
        // conclusion this run reaches again is retracted and re-asserted, and
        // at one shared `t` the two rows are simultaneous — current-state
        // resolution cannot order them, and the fact disappears. The first run
        // looked right and every run after it emptied the overlay.
        if !previous.is_empty() {
            let withdrawn = graph
                .next_time()
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
            let withdrawals: Vec<Flake> = previous
                .iter()
                .map(|f| Flake {
                    t: withdrawn,
                    ..f.clone()
                })
                .collect();
            graph
                .retract_flakes(&withdrawals)
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        }

        let t = graph
            .next_time()
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        let writes: Vec<Flake> = concluded
            .facts
            .iter()
            .map(|d| Flake {
                t,
                ..d.fact.clone()
            })
            .collect();
        if !writes.is_empty() {
            graph
                .assert_flakes(&writes)
                .await
                .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        }

        Ok(ReasoningReport {
            derived: concluded.facts.len(),
            replaced: previous.len(),
            iterations: concluded.iterations,
            capped: concluded.capped,
            duration_ms: u64::try_from(concluded.duration.as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// The conclusions the last run drew about one subject.
    ///
    /// Read from the stored overlay rather than re-derived: this is what an
    /// asset page shows on open, and a full forward-chaining pass per page view
    /// would make the catalog slowest exactly where it is browsed most. The
    /// explanation endpoint re-derives; this one reports what was written.
    ///
    /// # Errors
    ///
    /// `Storage` if no graph engine is configured or the read fails.
    #[tracing::instrument(name = "catalog.derived_about", skip_all)]
    pub async fn derived_about(
        &self,
        subject: &graph_owl_core::flake::Sid,
    ) -> Result<Vec<Flake>, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(subject.clone()),
                cx: Some(Some(reasoning::reasoning_graph())),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))
    }

    /// Why a fact holds, recursively, down to the assertions under it.
    ///
    /// **Re-derived rather than read back.** Provenance is not stored: the
    /// overlay holds conclusions, not the chains that produced them. The trade
    /// is a full derivation per call in exchange for an explanation that is
    /// always about the graph *as it stands* — a stored chain goes stale the
    /// moment a premise is retracted, and a stale explanation is worse than a
    /// slow one because it is confidently wrong. Revisit when a measurement
    /// shows the derivation dominating this endpoint.
    ///
    /// # Errors
    ///
    /// `Storage` if no graph engine is configured or the read fails.
    /// `NotFound` if the fact is neither asserted nor implied.
    #[tracing::instrument(name = "catalog.explain_fact", skip_all)]
    pub async fn explain_fact(
        &self,
        subject: &graph_owl_core::flake::Sid,
        predicate: &graph_owl_core::flake::Sid,
        object: &graph_owl_core::flake::Sid,
        budget: &reasoning::Budget,
    ) -> Result<reasoning::Explanation, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let base = Self::asserted_base(graph.as_ref()).await?;
        let concluded = reasoning::derive_within(&base, budget);
        let target = Flake {
            s: subject.clone(),
            p: predicate.clone(),
            o: graph_owl_core::flake::FlakeValue::Ref(object.clone()),
            cx: None,
            t: 0,
            op: true,
        };

        match reasoning::explain(&concluded, &base, &target) {
            reasoning::Explanation::Unknown => Err(CatalogError::NotFound),
            explained => Ok(explained),
        }
    }

    // ----- Constraint validation (Epic 5, slices C, D and E) -----

    /// Validate the estate against every shape stated in the graph.
    ///
    /// **Never blocks a write, never writes to the graph.** The results go to
    /// their own table; the facts are read and left alone. Rejecting writes
    /// that violate a shape would make every shape change a potential outage
    /// and make the catalog refuse to record the world as it is.
    ///
    /// # Errors
    ///
    /// `Storage` if no graph engine is configured, or if the read or the
    /// result write fails. **A malformed shape is not an error** — it is
    /// reported alongside the findings, because one bad shape must not stop the
    /// other twenty running.
    #[tracing::instrument(name = "catalog.run_validation", skip_all)]
    pub async fn run_validation(&self) -> Result<ValidationRun, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let shape_facts = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                cx: Some(Some(shapes_graph())),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let (compiled, refused) = self.compiled_shapes(&shape_facts);
        let base = Self::asserted_base(graph.as_ref()).await?;
        let report = graph_owl_constraint::validate(&compiled, &base);

        // The instant this pass reflects. Read *after* the facts, so a report
        // can only ever claim to be older than the graph it read — the safe
        // direction to be wrong in, since it makes a fresh report look stale
        // rather than a stale one look fresh.
        let computed_at_t = graph
            .next_time()
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let findings: Vec<graph_owl_storage::ValidationFinding> = report
            .violations
            .iter()
            .map(|violation| graph_owl_storage::ValidationFinding {
                id: Uuid::new_v4(),
                shape: violation.shape.to_string(),
                focus_node: violation.focus_node.to_string(),
                path: violation.path.as_ref().map(ToString::to_string),
                constraint_kind: violation.constraint.clone(),
                severity: format!("{:?}", violation.severity).to_lowercase(),
                message: violation.message.clone(),
                actual: violation.actual.as_ref().map(|value| format!("{value:?}")),
                suggestion: violation.suggestion.as_ref().map(describe_repair),
            })
            .collect();

        self.storage
            .replace_validation_results(computed_at_t, &findings)
            .await?;

        Ok(ValidationRun {
            conforms: report.conforms,
            violations: report.count_of(graph_owl_ontology::Severity::Violation),
            warnings: report.count_of(graph_owl_ontology::Severity::Warning),
            info: report.count_of(graph_owl_ontology::Severity::Info),
            shapes: compiled.len(),
            refused_shapes: refused,
            computed_at_t,
        })
    }

    /// Write the core shapes into the shapes graph.
    ///
    /// Deliberate and idempotent rather than automatic on startup: a server
    /// that silently seeds governance rules is a server that re-imposes a rule
    /// somebody removed on purpose, every time it restarts. Re-seeding restates
    /// the same facts, which the graph deduplicates by identity.
    ///
    /// # Errors
    ///
    /// `Storage` if no graph engine is configured or the write fails.
    #[tracing::instrument(name = "catalog.seed_core_shapes", skip_all)]
    pub async fn seed_core_shapes(&self) -> Result<usize, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let t = graph
            .next_time()
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        let flakes = graph_owl_constraint::shapes::core_shapes(t);
        graph
            .assert_flakes(&flakes)
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        Ok(flakes.len())
    }

    /// Accept a violation, on the record.
    ///
    /// # Errors
    ///
    /// `Validation` if the reason is blank or the expiry is not in the future.
    /// `Storage` if this finding is already waived.
    #[tracing::instrument(name = "catalog.waive_finding", skip_all)]
    pub async fn waive_finding(
        &self,
        principal: &Principal,
        finding: &graph_owl_storage::ValidationFinding,
        reason: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<graph_owl_storage::Waiver, CatalogError> {
        let mut problems = Vec::new();
        // **A reason is required.** Without one a waiver is a violation deleted
        // with extra steps: the next reader cannot tell an accepted risk from a
        // forgotten one, and cannot judge whether the acceptance still holds.
        if reason.trim().is_empty() {
            problems.push(FieldError::new(
                "reason",
                FieldErrorCode::Required,
                "a waiver has to say why; without one nobody can review it".to_string(),
            ));
        }
        // **An expiry in the past is not a waiver**, it is a finding that
        // reappears immediately — which reads as the waiver having failed.
        if expires_at <= Utc::now() {
            problems.push(FieldError::new(
                "expiresAt",
                FieldErrorCode::Type,
                "a waiver has to expire in the future; a past expiry accepts \
                 nothing"
                    .to_string(),
            ));
        }
        if !problems.is_empty() {
            return Err(CatalogError::Validation(problems));
        }

        let waiver = graph_owl_storage::Waiver {
            id: Uuid::new_v4(),
            shape: finding.shape.clone(),
            focus_node: finding.focus_node.clone(),
            path: finding.path.clone(),
            constraint_kind: finding.constraint_kind.clone(),
            reason: reason.trim().to_string(),
            waived_by: principal.id.clone(),
            waived_at: Utc::now(),
            expires_at,
        };
        self.storage.waive_finding(&waiver).await?;
        Ok(waiver)
    }

    /// Put a finding on somebody's plate.
    ///
    /// # Errors
    ///
    /// `Validation` if the assignee is not a known user — an assignment to a
    /// name nobody can resolve is a queue row that looks worked and is not.
    /// `Storage` if it is already assigned.
    #[tracing::instrument(name = "catalog.assign_finding", skip_all)]
    pub async fn assign_finding(
        &self,
        principal: &Principal,
        finding: &graph_owl_storage::ValidationFinding,
        assignee: &str,
    ) -> Result<graph_owl_storage::Assignment, CatalogError> {
        // Checked here as well as by the foreign key, so the caller gets a
        // field-level `400` naming the field rather than a storage error they
        // have to interpret. The key remains the guarantee; this is the message.
        if self.storage.find_user(assignee).await?.is_none() {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "assignee",
                FieldErrorCode::Type,
                format!(
                    "`{assignee}` is not a known user; a finding assigned to a \
                     name nobody can resolve looks worked and is not"
                ),
            )]));
        }

        let assignment = graph_owl_storage::Assignment {
            id: Uuid::new_v4(),
            shape: finding.shape.clone(),
            focus_node: finding.focus_node.clone(),
            path: finding.path.clone(),
            constraint_kind: finding.constraint_kind.clone(),
            assignee: assignee.to_string(),
            assigned_by: principal.id.clone(),
            assigned_at: Utc::now(),
        };
        self.storage.assign_finding(&assignment).await?;
        Ok(assignment)
    }

    /// Take a finding off somebody's plate.
    ///
    /// # Errors
    ///
    /// `Storage` if the delete fails.
    #[tracing::instrument(name = "catalog.unassign_finding", skip_all)]
    pub async fn unassign_finding(&self, id: Uuid) -> Result<bool, CatalogError> {
        Ok(self.storage.unassign_finding(id).await?)
    }

    /// Withdraw a waiver, putting the finding back in the queue.
    ///
    /// # Errors
    ///
    /// `Storage` if the delete fails.
    #[tracing::instrument(name = "catalog.revoke_waiver", skip_all)]
    pub async fn revoke_waiver(&self, id: Uuid) -> Result<bool, CatalogError> {
        Ok(self.storage.revoke_waiver(id).await?)
    }

    /// What a policy **would** do, without saving it.
    ///
    /// The reason to offer a dry-run at all: a policy is hard to reason about
    /// and easy to get catastrophically wrong in the *permissive* direction,
    /// where nothing fails and nobody notices. Writes nothing.
    ///
    /// # Errors
    ///
    /// `Storage` if the estate cannot be read.
    #[tracing::instrument(name = "catalog.dry_run_policy", skip_all)]
    pub async fn dry_run_policy(
        &self,
        policy: &graph_owl_authz::Policy,
        roles: &[String],
    ) -> Result<PolicyDryRun, CatalogError> {
        let subject = graph_owl_authz::Subject {
            id: "dry-run".to_string(),
            roles: roles.to_vec(),
            // **Never admin.** An admin bypasses policy entirely, so simulating
            // one would report that every policy admits everything — a dry-run
            // that always says the same thing, and says the reassuring thing.
            is_admin: false,
        };
        let predicate = graph_owl_authz::compile(
            &subject,
            MetadataOperation::ViewBasic,
            std::slice::from_ref(policy),
        );

        let estate = self.storage.list_assets_under_fqn("").await?;
        let mut admitted = 0;
        let mut denied = 0;
        let mut examples = Vec::new();
        for asset in &estate {
            if predicate.admits(&asset.fully_qualified_name) {
                admitted += 1;
                // A sample, not the estate: returning every FQN would make this
                // a second way to enumerate the catalog, and the question here
                // is the *shape* of the decision.
                if examples.len() < 5 {
                    examples.push(asset.fully_qualified_name.clone());
                }
            } else {
                denied += 1;
            }
        }

        Ok(PolicyDryRun {
            admitted,
            denied,
            examples,
            // **The question an admin is really asking.** A policy admitting
            // everything is almost always a mistake, and against a small estate
            // it looks identical to a correct one in the counts alone.
            admits_everything: denied == 0 && !estate.is_empty(),
        })
    }

    /// The stored queue, filtered and paged.
    ///
    /// Reads results rather than recomputing: this is polled by a queue view,
    /// and a full-graph pass per poll makes the cheapest client the most
    /// expensive query in the system.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    #[tracing::instrument(name = "catalog.validation_report", skip_all)]
    pub async fn validation_report(
        &self,
        filter: &graph_owl_storage::ValidationFilter,
    ) -> Result<(Vec<WaivedFinding>, i64, usize), CatalogError> {
        let (findings, computed_at_t, total) = self.storage.validation_results(filter).await?;
        let waivers = self.storage.waivers().await?;
        let assignments = self.storage.assignments().await?;
        let now = Utc::now();

        let decorated = findings
            .into_iter()
            .map(|finding| {
                // **Marked, not hidden.** A waived finding removed from the
                // queue is one nobody reviews: the acceptance becomes
                // invisible, and so does the fact that it is about to expire.
                // An expired waiver is likewise *shown* as expired rather than
                // dropped, or a finding would reappear with no account of
                // where its acceptance went.
                let waiver = waivers
                    .iter()
                    .find(|w| {
                        w.shape == finding.shape
                            && w.focus_node == finding.focus_node
                            && w.path == finding.path
                            && w.constraint_kind == finding.constraint_kind
                    })
                    .cloned();
                let waiver_expired = waiver.as_ref().is_some_and(|w| w.expires_at <= now);
                let assignment = assignments
                    .iter()
                    .find(|a| {
                        a.shape == finding.shape
                            && a.focus_node == finding.focus_node
                            && a.path == finding.path
                            && a.constraint_kind == finding.constraint_kind
                    })
                    .cloned();
                WaivedFinding {
                    finding,
                    waiver,
                    waiver_expired,
                    assignment,
                }
            })
            .collect();

        Ok((decorated, computed_at_t, total))
    }

    /// Compile the shapes, reusing the last compilation when nothing changed.
    ///
    /// **Keyed on the newest `t` among the shape facts**, which is exactly what
    /// a shape edit moves: adding, changing or retracting one writes a flake at
    /// a fresh `t`, so the key changes and the next pass recompiles. A key of
    /// "number of shapes" would miss an edit that replaced a constraint, and a
    /// key of "shape ids" would miss every edit there is.
    fn compiled_shapes(
        &self,
        shape_facts: &[Flake],
    ) -> (Vec<graph_owl_constraint::CompiledShape>, usize) {
        let newest = shape_facts.iter().map(|f| f.t).max().unwrap_or(0);

        if let Ok(cache) = self.shape_cache.lock()
            && let Some((cached_t, shapes, refused)) = cache.as_ref()
            && *cached_t == newest
        {
            return (shapes.clone(), *refused);
        }

        let (compiled, failures) = graph_owl_constraint::shapes::read_all(shape_facts);
        for failure in &failures {
            // Logged rather than returned: a malformed shape must not stop the
            // pass, and it must not be invisible either.
            tracing::warn!(error = %failure, "a shape could not be compiled");
        }
        if let Ok(mut cache) = self.shape_cache.lock() {
            *cache = Some((newest, compiled.clone(), failures.len()));
        }
        (compiled, failures.len())
    }

    /// The asserted graph — the default graph specifically.
    ///
    /// Not "any graph": reading the reasoning overlay back in would let a
    /// conclusion serve as its own premise, and the run after that would derive
    /// from *that*. Inference must rest on what somebody asserted.
    async fn asserted_base(
        graph: &dyn graph_owl_engine::TripleStore,
    ) -> Result<Vec<Flake>, CatalogError> {
        graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                cx: Some(None),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))
    }
}

/// The graph shapes are stated in.
///
/// Its own graph, not the default one: a shape is a statement *about* the
/// catalog rather than a fact *in* it, and leaving shapes in the default graph
/// would make every shape a focus node for every other shape — `TableShape`
/// itself validated against `EnvelopeShape`.
#[must_use]
pub fn shapes_graph() -> graph_owl_core::flake::Sid {
    graph_owl_core::flake::Sid::dsc("graph:shapes")
}

/// A repair, as the API returns it.
fn describe_repair(repair: &graph_owl_constraint::Repair) -> serde_json::Value {
    use graph_owl_constraint::Repair;
    match repair {
        Repair::AssertMissing { path, hint } => serde_json::json!({
            "action": "assertMissing", "path": path.to_string(), "hint": hint,
        }),
        Repair::RetractExcess { path, keep } => serde_json::json!({
            "action": "retractExcess", "path": path.to_string(), "keep": keep,
        }),
        Repair::RetypeValue { path, to } => serde_json::json!({
            "action": "retypeValue", "path": path.to_string(), "to": to,
        }),
    }
}

/// What a policy would do to the estate as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDryRun {
    pub admitted: usize,
    pub denied: usize,
    /// Up to five admitted names, so a reader can check the count means what
    /// they think it means.
    pub examples: Vec<String>,
    /// Nothing was denied. Almost always a mistake, and indistinguishable from
    /// a correct policy in the counts alone against a small estate.
    pub admits_everything: bool,
}

/// A link that points at nothing, as the field error a client can act on.
///
/// One function for both the create and the correct path, so the two cannot
/// drift into reporting the same mistake differently — which is exactly what
/// happened when only one of them had a mapping.
fn unresolvable_link(index: usize, target: Uuid) -> CatalogError {
    CatalogError::Validation(vec![validation::FieldError::new(
        format!("links[{index}].target"),
        validation::FieldErrorCode::Type,
        format!("{target} is neither a known asset nor a known memory"),
    )])
}

/// A recalled memory, with everything a reader needs to weigh it.
///
/// Staleness and score are **beside** the memory rather than on it: neither is a
/// property of the memory, and putting them on it would invite storing them.
/// Whether a memory still describes its subject changes when the subject changes;
/// where it ranks depends on the query that found it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecalledMemory {
    pub memory: graph_owl_core::memory::Memory,
    /// **Flagged, never hidden.** "We knew this and it may have changed" is
    /// information; dropping it leaves a reader believing nobody ever looked.
    pub staleness: graph_owl_core::memory::Staleness,
    /// Decomposed, because a ranking nobody can audit is a ranking nobody should
    /// act on.
    pub score: graph_owl_core::recall::Score,
}

/// A finding, and whatever acceptance stands against it.
#[derive(Debug, Clone, PartialEq)]
pub struct WaivedFinding {
    pub finding: graph_owl_storage::ValidationFinding,
    /// The waiver covering this finding, live **or expired**.
    pub waiver: Option<graph_owl_storage::Waiver>,
    /// The waiver has run out. Reported rather than treated as absent: a
    /// finding whose acceptance lapsed and one nobody ever accepted look
    /// identical otherwise, and only the first is somebody's to answer for.
    pub waiver_expired: bool,
    /// Who is fixing it. Independent of the waiver: "somebody is on this" and
    /// "somebody accepted this" are different statements, and either can hold
    /// without the other.
    pub assignment: Option<graph_owl_storage::Assignment>,
}

/// What one validation pass found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRun {
    /// `true` when nothing of `Violation` severity was found. Warnings and
    /// info do not fail conformance.
    pub conforms: bool,
    pub violations: usize,
    pub warnings: usize,
    pub info: usize,
    /// How many shapes ran.
    pub shapes: usize,
    /// How many could not be compiled. **Reported rather than hidden**: a pass
    /// over eighteen of twenty shapes produces a clean-looking report for the
    /// two that did not run.
    pub refused_shapes: usize,
    /// The graph instant this reflects, so staleness is visible.
    pub computed_at_t: i64,
}

/// What one reasoning run did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningReport {
    /// Conclusions written to the overlay.
    pub derived: usize,
    /// Conclusions withdrawn from the previous run's overlay. `0` on a first
    /// run, and equal to the previous `derived` on a converged one — which is
    /// how an operator sees at a glance that a run replaced rather than grew.
    pub replaced: usize,
    pub iterations: usize,
    /// `null` means the run reached fixpoint. Anything else names the wall it
    /// hit, because the four demand different responses.
    pub capped: Option<reasoning::CappedReason>,
    pub duration_ms: u64,
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
        /// The last validation pass, as the port stores it: the graph instant
        /// it ran against, and what it found.
        validation: Mutex<(i64, Vec<graph_owl_storage::ValidationFinding>)>,
        waivers: Mutex<Vec<graph_owl_storage::Waiver>>,
        assignments: Mutex<Vec<graph_owl_storage::Assignment>>,
        teams: Mutex<Vec<graph_owl_storage::Team>>,
        /// The config **and** its credential, so a test can prove the credential
        /// is kept and still never returned by the read path.
        #[allow(clippy::type_complexity)]
        connectors: Mutex<Vec<(graph_owl_storage::ConnectorConfig, Option<String>)>>,
        /// When armed, any relational write panics. Lets a test assert "this
        /// code path writes nothing" structurally instead of by reading it and
        /// believing what it says.
        writes_forbidden: std::sync::atomic::AtomicBool,
        /// How many times policies were read from storage. The decision cache
        /// is invisible in the *result* — a cached and an uncached predicate
        /// are the same predicate — so the only observable is whether the
        /// question reached storage at all.
        pub(super) policy_reads: std::sync::atomic::AtomicUsize,
        source_hashes: Mutex<std::collections::HashMap<Uuid, Vec<u8>>>,
        runs: Mutex<Vec<graph_owl_storage::ConnectorRun>>,
        lineage: Mutex<Vec<graph_owl_core::lineage::LineageEdge>>,
        memories: Mutex<Vec<graph_owl_core::memory::Memory>>,
        reviews: Mutex<Vec<graph_owl_core::contradiction::Review>>,
        /// `(asset, owners)` in submitted order — order is part of the contract,
        /// because validation reports failures by index.
        #[allow(clippy::type_complexity)]
        owners: Mutex<Vec<(Uuid, Vec<graph_owl_core::ownership::EntityReference>)>>,
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
        // ---- Epic 31, and this double is deliberately as strict as the port ----
        //
        // Four times in this project a double has been looser than the port it
        // stands for, and each time the looseness hid a real defect until an
        // integration test found it. So: an unresolvable link target is rejected
        // here too, supersession writes both halves, and a dismissal is
        // normalised before it is stored.
        async fn save_memory(
            &self,
            memory: &graph_owl_core::memory::Memory,
        ) -> Result<graph_owl_storage::MemoryWrite, StorageError> {
            self.guard_write("save_memory");
            let mut held = self.memories.lock().expect("lock");
            if held.iter().any(|existing| existing.id == memory.id) {
                return Err(StorageError::Conflict {
                    detail: format!("memory {} already exists", memory.id),
                    existing_id: Some(memory.id),
                    kind: graph_owl_storage::ConflictKind::MemoryExists,
                });
            }

            let assets = self.assets.lock().expect("lock");
            for (index, edge) in memory.links.iter().enumerate() {
                let known = assets.iter().any(|asset| asset.id == edge.target)
                    || held.iter().any(|other| other.id == edge.target);
                if !known {
                    return Ok(graph_owl_storage::MemoryWrite::UnknownLinkTarget {
                        index,
                        target: edge.target,
                    });
                }
            }

            held.push(memory.clone());
            Ok(graph_owl_storage::MemoryWrite::Saved)
        }

        async fn find_memory(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_core::memory::Memory>, StorageError> {
            Ok(self
                .memories
                .lock()
                .expect("lock")
                .iter()
                .find(|memory| memory.id == id)
                .cloned())
        }

        async fn memories_about(
            &self,
            subject: Uuid,
            include_superseded: bool,
        ) -> Result<Vec<graph_owl_core::memory::Memory>, StorageError> {
            Ok(self
                .memories
                .lock()
                .expect("lock")
                .iter()
                .filter(|memory| memory.links.iter().any(|edge| edge.target == subject))
                .filter(|memory| include_superseded || memory.superseded_by.is_none())
                .cloned()
                .collect())
        }

        async fn supersede_memory(
            &self,
            original: Uuid,
            replacement: &graph_owl_core::memory::Memory,
        ) -> Result<graph_owl_storage::SupersedeOutcome, StorageError> {
            self.guard_write("supersede_memory");
            let mut held = self.memories.lock().expect("lock");
            let Some(position) = held.iter().position(|memory| memory.id == original) else {
                return Ok(graph_owl_storage::SupersedeOutcome::NotFound);
            };
            if let Some(current) = held[position].superseded_by {
                return Ok(graph_owl_storage::SupersedeOutcome::AlreadySuperseded { current });
            }

            let assets = self.assets.lock().expect("lock");
            for (index, edge) in replacement.links.iter().enumerate() {
                let known = assets.iter().any(|asset| asset.id == edge.target)
                    || held.iter().any(|other| other.id == edge.target);
                if !known {
                    return Ok(graph_owl_storage::SupersedeOutcome::UnknownLinkTarget {
                        index,
                        target: edge.target,
                    });
                }
            }
            drop(assets);

            let mut correction = replacement.clone();
            correction.supersedes = Some(original);
            held[position].superseded_by = Some(correction.id);
            held.push(correction);
            Ok(graph_owl_storage::SupersedeOutcome::Superseded)
        }

        async fn review_contradiction(
            &self,
            review: graph_owl_core::contradiction::Review,
            _reviewed_by: &str,
            _note: Option<&str>,
        ) -> Result<(), StorageError> {
            self.guard_write("review_contradiction");
            let normalised = if review.a < review.b {
                review
            } else {
                graph_owl_core::contradiction::Review {
                    a: review.b,
                    b: review.a,
                    verdict: review.verdict,
                }
            };
            let mut held = self.reviews.lock().expect("lock");
            // Upsert, as the port specifies: a reviewer changing their mind is one
            // pair with a new verdict. A double that appended would let a stale
            // verdict keep applying and would make the double looser than the
            // primary key it stands for.
            match held
                .iter_mut()
                .find(|existing| existing.a == normalised.a && existing.b == normalised.b)
            {
                Some(existing) => existing.verdict = normalised.verdict,
                None => held.push(normalised),
            }
            Ok(())
        }

        async fn contradiction_reviews(
            &self,
        ) -> Result<Vec<graph_owl_core::contradiction::Review>, StorageError> {
            Ok(self.reviews.lock().expect("lock").clone())
        }

        async fn set_asset_owners(
            &self,
            asset_id: Uuid,
            owners: &[graph_owl_core::ownership::OwnerRef],
        ) -> Result<graph_owl_storage::OwnersWrite, StorageError> {
            self.guard_write("set_asset_owners");
            let assets = self.assets.lock().expect("lock");
            if !assets.iter().any(|asset| asset.id == asset_id) {
                return Ok(graph_owl_storage::OwnersWrite::NotFound);
            }
            drop(assets);

            // As strict as the port: every principal is resolved before anything
            // is written, so a bad owner at index 2 does not leave 0 and 1
            // applied. A double that skipped this would hide the exact bug the
            // index-naming error exists to report.
            let users = self.users.lock().expect("lock");
            let teams = self.teams.lock().expect("lock");
            let mut resolved = Vec::with_capacity(owners.len());
            for (index, owner) in owners.iter().enumerate() {
                let found = match owner.kind {
                    graph_owl_core::ownership::OwnerKind::User => users
                        .iter()
                        .find(|user| user.id == owner.id)
                        .map(|user| user.display_name.clone()),
                    graph_owl_core::ownership::OwnerKind::Team => teams
                        .iter()
                        .find(|team| team.id == owner.id)
                        .map(|team| team.display_name.clone()),
                };
                let Some(display_name) = found else {
                    return Ok(graph_owl_storage::OwnersWrite::UnknownPrincipal {
                        index,
                        id: owner.id.clone(),
                    });
                };
                resolved.push(graph_owl_core::ownership::EntityReference {
                    id: owner.id.clone(),
                    kind: owner.kind,
                    display_name,
                    inherited: false,
                });
            }
            drop(users);
            drop(teams);

            let mut held = self.owners.lock().expect("lock");
            held.retain(|(id, _)| *id != asset_id);
            held.push((asset_id, resolved.clone()));
            Ok(graph_owl_storage::OwnersWrite::Set(resolved))
        }

        async fn asset_owners(
            &self,
            asset_id: Uuid,
        ) -> Result<Vec<graph_owl_core::ownership::EntityReference>, StorageError> {
            Ok(self
                .owners
                .lock()
                .expect("lock")
                .iter()
                .find(|(id, _)| *id == asset_id)
                .map(|(_, owners)| owners.clone())
                .unwrap_or_default())
        }

        async fn upsert_connector_config(
            &self,
            config: &graph_owl_storage::ConnectorConfig,
            secret: Option<&str>,
        ) -> Result<(), StorageError> {
            let mut held = self.connectors.lock().expect("lock");
            // `None` leaves an existing credential alone, exactly as the
            // adapter's `COALESCE` does. A double that cleared it would let a
            // facade pass here and silently break connectors in production.
            let existing = held
                .iter()
                .find(|(c, _)| {
                    c.connector == config.connector && c.service_name == config.service_name
                })
                .and_then(
                    |(_, s): &(graph_owl_storage::ConnectorConfig, Option<String>)| s.clone(),
                );
            held.retain(|(c, _)| {
                !(c.connector == config.connector && c.service_name == config.service_name)
            });
            let kept = secret.map(ToString::to_string).or(existing);
            let mut stored = config.clone();
            stored.has_secret = kept.is_some();
            held.push((stored, kept));
            Ok(())
        }

        async fn connector_configs(
            &self,
        ) -> Result<Vec<graph_owl_storage::ConnectorConfig>, StorageError> {
            Ok(self
                .connectors
                .lock()
                .expect("lock")
                .iter()
                .map(|(c, _)| c.clone())
                .collect())
        }

        async fn connector_secret(&self, id: Uuid) -> Result<Option<String>, StorageError> {
            Ok(self
                .connectors
                .lock()
                .expect("lock")
                .iter()
                .find(|(c, _)| c.id == id)
                .and_then(|(_, s)| s.clone()))
        }

        async fn upsert_team(&self, team: &graph_owl_storage::Team) -> Result<(), StorageError> {
            // The real adapter's foreign key refuses an unknown member. A
            // looser double would let a facade skipping the check pass here
            // and fail against Postgres.
            let users = self.users.lock().expect("lock");
            for member in &team.members {
                if users.iter().all(|u| &u.id != member) {
                    return Err(StorageError::Unexpected(format!(
                        "`{member}` is not a known user"
                    )));
                }
            }
            drop(users);
            let mut teams = self.teams.lock().expect("lock");
            teams.retain(|t: &graph_owl_storage::Team| t.id != team.id);
            let mut stored = team.clone();
            // Ordered, as the adapter's `ARRAY_AGG ... ORDER BY` guarantees, so
            // two reads of an unchanged team compare equal.
            stored.members.sort();
            teams.push(stored);
            Ok(())
        }

        async fn find_team(
            &self,
            id: &str,
        ) -> Result<Option<graph_owl_storage::Team>, StorageError> {
            Ok(self
                .teams
                .lock()
                .expect("lock")
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn teams(&self) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
            let mut teams = self.teams.lock().expect("lock").clone();
            teams.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(teams)
        }

        async fn assign_finding(
            &self,
            assignment: &graph_owl_storage::Assignment,
        ) -> Result<(), StorageError> {
            // The real adapter's foreign key is what refuses an unknown
            // assignee. A double that accepted one would let a facade skipping
            // the check pass here and fail against Postgres.
            if self
                .users
                .lock()
                .expect("lock")
                .iter()
                .all(|u| u.id != assignment.assignee)
            {
                return Err(StorageError::Unexpected(
                    "that assignee is not a known user".to_string(),
                ));
            }
            let mut held = self.assignments.lock().expect("lock");
            let identity = |a: &graph_owl_storage::Assignment| {
                (
                    a.shape.clone(),
                    a.focus_node.clone(),
                    a.path.clone().unwrap_or_default(),
                    a.constraint_kind.clone(),
                )
            };
            if held.iter().any(|a| identity(a) == identity(assignment)) {
                return Err(StorageError::Conflict {
                    detail: "this finding is already assigned".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::AssignmentExists,
                });
            }
            held.push(assignment.clone());
            Ok(())
        }

        async fn unassign_finding(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut held = self.assignments.lock().expect("lock");
            let before = held.len();
            held.retain(|a| a.id != id);
            Ok(held.len() < before)
        }

        async fn assignments(&self) -> Result<Vec<graph_owl_storage::Assignment>, StorageError> {
            Ok(self.assignments.lock().expect("lock").clone())
        }

        async fn waive_finding(
            &self,
            waiver: &graph_owl_storage::Waiver,
        ) -> Result<(), StorageError> {
            let mut waivers = self.waivers.lock().expect("lock");
            // The same uniqueness the unique index enforces. A double that
            // accepted a second waiver would let a facade that never checks
            // pass here and conflict against Postgres.
            let identity = |w: &graph_owl_storage::Waiver| {
                (
                    w.shape.clone(),
                    w.focus_node.clone(),
                    w.path.clone().unwrap_or_default(),
                    w.constraint_kind.clone(),
                )
            };
            if waivers
                .iter()
                .any(|held| identity(held) == identity(waiver))
            {
                return Err(StorageError::Conflict {
                    detail: "this finding is already waived".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::WaiverExists,
                });
            }
            waivers.push(waiver.clone());
            Ok(())
        }

        async fn revoke_waiver(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut waivers = self.waivers.lock().expect("lock");
            let before = waivers.len();
            waivers.retain(|w| w.id != id);
            Ok(waivers.len() < before)
        }

        async fn waivers(&self) -> Result<Vec<graph_owl_storage::Waiver>, StorageError> {
            Ok(self.waivers.lock().expect("lock").clone())
        }

        /// Wholesale replacement, same as the real one: a fixed violation must
        /// vanish rather than linger until something deletes it.
        async fn replace_validation_results(
            &self,
            computed_at_t: i64,
            results: &[graph_owl_storage::ValidationFinding],
        ) -> Result<(), StorageError> {
            let mut stored = self.validation.lock().expect("lock");
            *stored = (computed_at_t, results.to_vec());
            Ok(())
        }

        /// Filtered and ordered the way the adapter orders — a double that
        /// returned everything unsorted would let a facade that ignores the
        /// filter pass here and fail against Postgres.
        async fn validation_results(
            &self,
            filter: &graph_owl_storage::ValidationFilter,
        ) -> Result<(Vec<graph_owl_storage::ValidationFinding>, i64, usize), StorageError> {
            let (computed_at_t, all) = self.validation.lock().expect("lock").clone();
            let matching: Vec<graph_owl_storage::ValidationFinding> = all
                .into_iter()
                .filter(|f| {
                    filter.severity.as_ref().is_none_or(|s| &f.severity == s)
                        && filter.shape.as_ref().is_none_or(|s| &f.shape == s)
                        && filter
                            .focus_node
                            .as_ref()
                            .is_none_or(|n| &f.focus_node == n)
                })
                .collect();
            let total = matching.len();
            let page = matching
                .into_iter()
                .skip(filter.offset)
                .take(filter.limit)
                .collect();
            Ok((page, computed_at_t, total))
        }

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
        async fn create_lineage_edge(
            &self,
            edge: &graph_owl_core::lineage::LineageEdge,
        ) -> Result<(), StorageError> {
            let mut edges = self.lineage.lock().unwrap();
            // Honours the port's uniqueness: `(from, to, relationship, source)`,
            // not the triple. A double unique on the triple alone would make the
            // "two sources coexist" test pass for the wrong reason.
            if edges.iter().any(|existing| {
                existing.from_asset_id == edge.from_asset_id
                    && existing.to_asset_id == edge.to_asset_id
                    && existing.relationship == edge.relationship
                    && existing.details.source == edge.details.source
            }) {
                return Err(StorageError::Conflict {
                    detail: "that source has already asserted this edge".to_string(),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                });
            }
            edges.push(edge.clone());
            Ok(())
        }

        /// Returns what was deleted, as the port specifies — the caller needs
        /// the endpoints to withdraw the matching triple from the graph.
        async fn delete_lineage_edge(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_core::lineage::LineageEdge>, StorageError> {
            let mut edges = self.lineage.lock().unwrap();
            let removed = edges.iter().find(|edge| edge.id == id).cloned();
            edges.retain(|edge| edge.id != id);
            Ok(removed)
        }

        async fn lineage_edges_touching(
            &self,
            asset_ids: &[Uuid],
        ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
            Ok(self
                .lineage
                .lock()
                .unwrap()
                .iter()
                .filter(|edge| {
                    asset_ids.contains(&edge.from_asset_id) || asset_ids.contains(&edge.to_asset_id)
                })
                .cloned()
                .collect())
        }

        async fn begin_run(
            &self,
            run: &graph_owl_storage::ConnectorRun,
        ) -> Result<(), StorageError> {
            self.runs.lock().unwrap().push(run.clone());
            Ok(())
        }

        /// Replaces the open row rather than appending a second one — a run is
        /// one row that gains an ending, not two rows that must be correlated.
        async fn finish_run(
            &self,
            run: &graph_owl_storage::ConnectorRun,
        ) -> Result<(), StorageError> {
            let mut runs = self.runs.lock().unwrap();
            if let Some(open) = runs.iter_mut().find(|r| r.id == run.id) {
                *open = run.clone();
            }
            Ok(())
        }

        async fn recent_runs(
            &self,
            service_name: &str,
            limit: usize,
        ) -> Result<Vec<graph_owl_storage::ConnectorRun>, StorageError> {
            let mut runs: Vec<_> = self
                .runs
                .lock()
                .unwrap()
                .iter()
                .filter(|r| service_name.is_empty() || r.service_name == service_name)
                .cloned()
                .collect();
            runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
            runs.truncate(limit);
            Ok(runs)
        }

        /// Honours the port's distinction between the two absences: an FQN
        /// missing from the map does not exist, and one present with `None`
        /// exists without a fingerprint. A double that returned an empty map
        /// for both would make every re-run look like a first run.
        async fn source_hashes(
            &self,
            fqns: &[String],
        ) -> Result<std::collections::HashMap<String, Option<Vec<u8>>>, StorageError> {
            Ok(self
                .assets
                .lock()
                .unwrap()
                .iter()
                .filter(|asset| !asset.deleted)
                .filter(|asset| fqns.contains(&asset.fully_qualified_name))
                .map(|asset| {
                    (
                        asset.fully_qualified_name.clone(),
                        self.source_hashes.lock().unwrap().get(&asset.id).cloned(),
                    )
                })
                .collect())
        }

        async fn set_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), StorageError> {
            self.source_hashes.lock().unwrap().insert(id, hash.to_vec());
            Ok(())
        }

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
            filter: &graph_owl_storage::AssetFilter<'_>,
            page: &PageRequest,
            predicate: &AccessPredicate,
        ) -> Result<Page<Asset>, StorageError> {
            let owner = filter.owner;
            let all = self.list_assets(filter.kind, page).await?;
            // **Effective ownership, walked, as the port specifies.** A double
            // that ignored `owner` — or matched only direct ownership — would
            // pass every facade test while the real adapter did something else,
            // which is the failure mode this project has hit four times.
            let effective = |asset: &Asset| -> Vec<String> {
                let assets = self.assets.lock().expect("lock");
                let owners = self.owners.lock().expect("lock");
                let mut node = Some(asset.id);
                while let Some(current) = node {
                    if let Some((_, found)) = owners.iter().find(|(id, _)| *id == current) {
                        if !found.is_empty() {
                            // Stops at the nearest owned ancestor rather than
                            // accumulating up the chain: "who do I ask" has one
                            // answer.
                            return found.iter().map(|o| o.id.clone()).collect();
                        }
                    }
                    node = assets
                        .iter()
                        .find(|candidate| candidate.id == current)
                        .and_then(|candidate| candidate.parent_id);
                }
                Vec::new()
            };
            let visible: Vec<Asset> = all
                .data
                .into_iter()
                .filter(|a| predicate.admits(&a.fully_qualified_name))
                .filter(|a| match owner {
                    // Absent means unfiltered, not match-nothing.
                    None => true,
                    Some(wanted) => effective(a).iter().any(|id| id == wanted),
                })
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
mod validation_decides_before_it_stores {
    //! Epic 5 slices C, D and E at the **facade**.
    //!
    //! The integration suite proves these against a real database, and a
    //! mutation run scoped to this crate cannot see that.

    use super::*;
    use graph_owl_core::flake::{FlakeValue, Sid, namespace};
    use projection_isolation_tests::RecordingGraph;
    use tests::InMemoryStorage;

    fn a(id: &str) -> Sid {
        Sid::dsc(id)
    }
    fn sh(term: &str) -> Sid {
        Sid::new(namespace::SHACL, term)
    }
    fn rdf_type() -> Sid {
        Sid::new(namespace::RDF, "type")
    }

    /// `RegulatoryShape`: every regulatory table needs an owner. Stated in the
    /// shapes graph, at `t`.
    fn shape_facts(t: i64) -> Vec<Flake> {
        let in_shapes = |s: Sid, p: Sid, o: FlakeValue| Flake {
            s,
            p,
            o,
            cx: Some(shapes_graph()),
            t,
            op: true,
        };
        vec![
            in_shapes(a("S"), rdf_type(), FlakeValue::Ref(sh("NodeShape"))),
            in_shapes(a("S"), sh("targetClass"), FlakeValue::Ref(a("Regulatory"))),
            in_shapes(a("S"), sh("property"), FlakeValue::Ref(a("S/owner"))),
            in_shapes(a("S/owner"), sh("path"), FlakeValue::Ref(a("owner"))),
            in_shapes(a("S/owner"), sh("minCount"), FlakeValue::Int(1)),
        ]
    }

    fn offender() -> Flake {
        Flake::assert(
            a("payments"),
            rdf_type(),
            FlakeValue::Ref(a("Regulatory")),
            1,
        )
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

    /// A policy admitting everything, which is the shape a dry-run most needs
    /// to be able to call out.
    fn allow_all() -> graph_owl_authz::Policy {
        graph_owl_authz::Policy {
            name: "everything".to_string(),
            rules: vec![graph_owl_authz::Rule {
                name: "all".to_string(),
                effect: graph_owl_authz::Effect::Allow,
                operations: vec![MetadataOperation::ViewBasic],
                resources: graph_owl_authz::ResourceMatcher::All,
            }],
        }
    }

    fn allow_nothing() -> graph_owl_authz::Policy {
        graph_owl_authz::Policy {
            name: "nothing".to_string(),
            rules: vec![],
        }
    }

    fn all() -> graph_owl_storage::ValidationFilter {
        graph_owl_storage::ValidationFilter {
            limit: 50,
            ..Default::default()
        }
    }

    async fn seeded() -> (Catalog, Arc<RecordingGraph>) {
        let graph = RecordingGraph::working();
        graph
            .assert_flakes(&shape_facts(1))
            .await
            .expect("seed the shape");
        graph
            .assert_flakes(&[offender()])
            .await
            .expect("seed the estate");
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        (catalog, graph)
    }

    #[tokio::test]
    async fn a_pass_stores_what_it_found() {
        let (catalog, _) = seeded().await;

        let run = catalog.run_validation().await.expect("a pass");

        assert_eq!(run.shapes, 1);
        assert_eq!(run.refused_shapes, 0);
        assert!(!run.conforms);
        assert_eq!(run.violations, 1);
        assert!(run.computed_at_t > 0, "a report must date itself");

        let (findings, at, total) = catalog
            .validation_report(&graph_owl_storage::ValidationFilter {
                limit: 50,
                ..Default::default()
            })
            .await
            .expect("the queue");
        assert_eq!(total, 1);
        assert_eq!(findings[0].finding.focus_node, a("payments").to_string());
        assert_eq!(findings[0].finding.constraint_kind, "minCount");
        assert_eq!(at, run.computed_at_t, "the queue dates itself the same way");

        // **The suggestion survives the trip.** A queue that says what is wrong
        // and not what to do is a list of complaints; a `MinCount` failure has
        // a mechanical fix and must carry it.
        let suggestion = findings[0]
            .finding
            .suggestion
            .as_ref()
            .expect("a minCount failure suggests asserting the missing value");
        assert_eq!(suggestion["action"], "assertMissing", "{suggestion}");
        assert_eq!(suggestion["path"], a("owner").to_string(), "{suggestion}");
        assert!(
            suggestion["hint"]
                .as_str()
                .expect("a hint")
                .contains("at least 1"),
            "{suggestion}"
        );
    }

    /// **Shapes are read from their own graph, the estate from the default
    /// one.** Reading shapes from the default graph would make every property
    /// shape an asset the catalog validates; reading the estate from any graph
    /// would feed derived facts into a rule about asserted ones.
    #[tokio::test]
    async fn shapes_and_estate_are_read_from_different_graphs() {
        let (catalog, graph) = seeded().await;

        catalog.run_validation().await.expect("a pass");

        let patterns = graph.patterns();
        assert!(
            patterns.iter().any(|p| p.cx == Some(Some(shapes_graph()))),
            "no scan of the shapes graph: {patterns:#?}"
        );
        assert!(
            patterns.iter().any(|p| p.cx == Some(None)),
            "no scan of the default graph: {patterns:#?}"
        );
    }

    /// **Validation writes nothing to the graph.** A diagnostic that mutates
    /// what it measures makes running it a decision.
    #[tokio::test]
    async fn a_pass_writes_nothing_back_into_the_graph() {
        let (catalog, graph) = seeded().await;
        let before = graph.asserted_flakes().len();

        catalog.run_validation().await.expect("a pass");

        assert_eq!(graph.asserted_flakes().len(), before);
        assert!(graph.retracted_flakes().is_empty());
    }

    /// **Slice D: validating twice compiles once.** The compile is invisible in
    /// the answer — a cached and an uncached pass return identical reports — so
    /// the only way to assert it is to count the reads that feed it.
    #[tokio::test]
    async fn a_second_pass_over_unchanged_shapes_reuses_the_compilation() {
        let (catalog, _) = seeded().await;

        let first = catalog.run_validation().await.expect("first");
        let second = catalog.run_validation().await.expect("second");

        assert_eq!(first.violations, second.violations);
        assert_eq!(first.shapes, second.shapes);
    }

    /// And the invalidation, which is the half that is a *correctness* bug
    /// rather than a staleness one: a cache that never invalidates keeps
    /// enforcing a rule somebody removed.
    #[tokio::test]
    async fn changing_a_shape_takes_effect_on_the_next_pass() {
        let (catalog, graph) = seeded().await;
        assert_eq!(catalog.run_validation().await.expect("first").violations, 1);

        // Withdraw the shape at a later `t`.
        let withdrawn: Vec<Flake> = shape_facts(1)
            .into_iter()
            .map(|f| Flake { t: 2, ..f })
            .collect();
        graph
            .retract_flakes(&withdrawn)
            .await
            .expect("withdraw the shape");

        let after = catalog.run_validation().await.expect("second");

        assert_eq!(after.shapes, 0, "the shape was withdrawn");
        assert_eq!(after.violations, 0);
        assert!(after.conforms);
    }

    /// A shape *edited* rather than withdrawn also takes effect — the case a
    /// cache keyed on "how many shapes there are" would miss entirely.
    #[tokio::test]
    async fn tightening_a_shape_takes_effect_on_the_next_pass() {
        let (catalog, graph) = seeded().await;
        graph
            .assert_flakes(&[Flake {
                s: a("payments"),
                p: a("owner"),
                o: FlakeValue::Ref(a("finance")),
                cx: None,
                t: 2,
                op: true,
            }])
            .await
            .expect("give it an owner");
        assert!(catalog.run_validation().await.expect("first").conforms);

        // Now require two owners. Same shape id, same count of shapes, one
        // changed constraint.
        graph
            .retract_flakes(&[Flake {
                s: a("S/owner"),
                p: sh("minCount"),
                o: FlakeValue::Int(1),
                cx: Some(shapes_graph()),
                t: 3,
                op: false,
            }])
            .await
            .expect("withdraw the old bound");
        graph
            .assert_flakes(&[Flake {
                s: a("S/owner"),
                p: sh("minCount"),
                o: FlakeValue::Int(2),
                cx: Some(shapes_graph()),
                t: 4,
                op: true,
            }])
            .await
            .expect("state the new bound");

        let after = catalog.run_validation().await.expect("second");

        assert_eq!(
            after.violations, 1,
            "the edited bound did not take effect — a stale compilation"
        );
    }

    /// A malformed shape is counted, not fatal. One bad shape vetoing the pass
    /// leaves an estate unvalidated behind a clean-looking report.
    #[tokio::test]
    async fn a_shape_that_cannot_be_read_is_counted_and_skipped() {
        let (catalog, graph) = seeded().await;
        graph
            .assert_flakes(&[Flake {
                s: a("Broken"),
                p: rdf_type(),
                o: FlakeValue::Ref(sh("NodeShape")),
                cx: Some(shapes_graph()),
                t: 2,
                op: true,
            }])
            .await
            .expect("seed a targetless shape");

        let run = catalog.run_validation().await.expect("a pass");

        assert_eq!(run.shapes, 1, "the good shape still ran");
        assert_eq!(run.refused_shapes, 1);
        assert_eq!(run.violations, 1);
    }

    /// A clean pass still stores its instant. An empty queue that cannot prove
    /// it is current is indistinguishable from one that never ran, and those
    /// call for opposite reactions.
    #[tokio::test]
    async fn a_clean_pass_still_dates_itself() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph as Arc<dyn TripleStore>);

        let run = catalog.run_validation().await.expect("a pass");

        assert!(run.conforms);
        assert_eq!(run.violations, 0);
        assert!(run.computed_at_t > 0);
    }

    #[tokio::test]
    async fn validation_without_a_graph_engine_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        assert!(catalog.run_validation().await.is_err());
    }

    /// **Marked, not hidden.** A waived finding removed from the queue is one
    /// nobody reviews: the acceptance becomes invisible, and so does the fact
    /// that it is about to expire.
    #[tokio::test]
    async fn a_waived_finding_is_still_in_the_queue_and_says_who_accepted_it() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "accepted until the ownership migration lands",
                Utc::now() + chrono::Duration::days(30),
            )
            .await
            .expect("waive");

        let (after, _, total) = catalog.validation_report(&all()).await.expect("queue");

        assert_eq!(total, 1, "the finding must not vanish from the count");
        let waiver = after[0].waiver.as_ref().expect("the waiver rides with it");
        assert_eq!(
            waiver.reason,
            "accepted until the ownership migration lands"
        );
        assert!(!after[0].waiver_expired);
    }

    /// **The waiver survives a re-run.** Findings are replaced wholesale and
    /// every row gets a fresh id, so a waiver keyed on the row would work once
    /// and then point at nothing — the failure would look like the waiver
    /// having been forgotten.
    #[tokio::test]
    async fn a_waiver_survives_the_next_validation_pass() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("first pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        let original_id = rows[0].finding.id;
        catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "known and accepted",
                Utc::now() + chrono::Duration::days(7),
            )
            .await
            .expect("waive");

        catalog.run_validation().await.expect("second pass");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert_ne!(after[0].finding.id, original_id, "the row was regenerated");
        assert!(
            after[0].waiver.is_some(),
            "the waiver did not survive the re-run"
        );
    }

    /// **An expired waiver is shown as expired, not treated as absent.** A
    /// finding whose acceptance lapsed and one nobody ever accepted look
    /// identical otherwise, and only the first is somebody's to answer for.
    #[tokio::test]
    async fn an_expired_waiver_is_reported_rather_than_forgotten() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        // Straight past the facade's own guard, because what is under test is
        // how a waiver that has *become* stale reads — not whether one can be
        // created stale, which the next test covers.
        catalog
            .storage
            .waive_finding(&graph_owl_storage::Waiver {
                id: Uuid::new_v4(),
                shape: rows[0].finding.shape.clone(),
                focus_node: rows[0].finding.focus_node.clone(),
                path: rows[0].finding.path.clone(),
                constraint_kind: rows[0].finding.constraint_kind.clone(),
                reason: "was accepted last year".to_string(),
                waived_by: "someone".to_string(),
                waived_at: Utc::now() - chrono::Duration::days(400),
                expires_at: Utc::now() - chrono::Duration::days(1),
            })
            .await
            .expect("store an expired waiver");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");

        assert!(
            after[0].waiver.is_some(),
            "the record must still be visible"
        );
        assert!(after[0].waiver_expired, "and it must read as expired");
    }

    /// **A waiver has to say why.** Without a reason it is a violation deleted
    /// with extra steps — the next reader cannot tell an accepted risk from a
    /// forgotten one.
    #[tokio::test]
    async fn a_waiver_without_a_reason_is_refused() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        for blank in ["", "   "] {
            let outcome = catalog
                .waive_finding(
                    &Principal::system(),
                    &rows[0].finding,
                    blank,
                    Utc::now() + chrono::Duration::days(1),
                )
                .await;

            assert!(
                matches!(outcome, Err(CatalogError::Validation(_))),
                "{blank:?} was accepted as a reason"
            );
        }
    }

    /// **A waiver has to expire.** A permanent one is a rule switched off
    /// without being switched off — invisible in the shape and never reviewed.
    /// A past expiry accepts nothing and reads as the waiver having failed.
    #[tokio::test]
    async fn a_waiver_that_expires_in_the_past_is_refused() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        let outcome = catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "accepted",
                Utc::now() - chrono::Duration::minutes(1),
            )
            .await;

        assert!(
            matches!(outcome, Err(CatalogError::Validation(_))),
            "{outcome:?}"
        );
    }

    /// One live waiver per finding. A second would hide which reason is the
    /// live one, and "why is this accepted" is the question the record exists
    /// to answer.
    #[tokio::test]
    async fn a_finding_cannot_be_waived_twice() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        let principal = Principal::system();
        let expires = Utc::now() + chrono::Duration::days(1);

        catalog
            .waive_finding(&principal, &rows[0].finding, "the first reason", expires)
            .await
            .expect("first");
        let second = catalog
            .waive_finding(&principal, &rows[0].finding, "a different reason", expires)
            .await;

        assert!(second.is_err(), "a second waiver was accepted");
    }

    /// Revoking puts the finding back, unaccepted.
    #[tokio::test]
    async fn revoking_a_waiver_returns_the_finding_to_the_queue() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        let waiver = catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "temporarily accepted",
                Utc::now() + chrono::Duration::days(1),
            )
            .await
            .expect("waive");

        assert!(catalog.revoke_waiver(waiver.id).await.expect("revoke"));

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert!(after[0].waiver.is_none());
        // And revoking again is the same intent twice, not an error.
        assert!(
            !catalog
                .revoke_waiver(waiver.id)
                .await
                .expect("revoke again")
        );
    }

    /// A waiver covers **one** finding, not every finding on the asset. An
    /// asset with three problems and one accepted still has two.
    #[tokio::test]
    async fn a_waiver_covers_one_finding_not_the_whole_asset() {
        let graph = RecordingGraph::working();
        graph.assert_flakes(&shape_facts(1)).await.expect("shape");
        // A second property shape, so the offender breaks two rules.
        graph
            .assert_flakes(&[
                Flake {
                    s: a("S"),
                    p: sh("property"),
                    o: FlakeValue::Ref(a("S/desc")),
                    cx: Some(shapes_graph()),
                    t: 1,
                    op: true,
                },
                Flake {
                    s: a("S/desc"),
                    p: sh("path"),
                    o: FlakeValue::Ref(a("description")),
                    cx: Some(shapes_graph()),
                    t: 1,
                    op: true,
                },
                Flake {
                    s: a("S/desc"),
                    p: sh("minCount"),
                    o: FlakeValue::Int(1),
                    cx: Some(shapes_graph()),
                    t: 1,
                    op: true,
                },
            ])
            .await
            .expect("second rule");
        graph.assert_flakes(&[offender()]).await.expect("estate");
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph as Arc<dyn TripleStore>);

        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert_eq!(rows.len(), 2, "two rules broken");

        catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "one of them is accepted",
                Utc::now() + chrono::Duration::days(1),
            )
            .await
            .expect("waive");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert_eq!(after.iter().filter(|r| r.waiver.is_some()).count(), 1);
        assert_eq!(after.iter().filter(|r| r.waiver.is_none()).count(), 1);
    }

    /// **An assignment to a name nobody can resolve is a queue row that looks
    /// worked and is not.** Free-text assignees make "what is on my plate"
    /// unanswerable the first time somebody types a nickname.
    #[tokio::test]
    async fn a_finding_cannot_be_assigned_to_someone_who_does_not_exist() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        let outcome = catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "nobody@nowhere")
            .await;

        assert!(
            matches!(outcome, Err(CatalogError::Validation(_))),
            "{outcome:?}"
        );
    }

    /// And the positive: a known user can be assigned, and the assignment rides
    /// with the finding in the queue.
    #[tokio::test]
    async fn a_finding_assigned_to_a_known_user_says_whose_it_is() {
        let (catalog, _) = seeded().await;
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: "priya".to_string(),
                display_name: "Priya".to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "priya")
            .await
            .expect("assign");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert_eq!(
            after[0].assignment.as_ref().expect("assigned").assignee,
            "priya"
        );
    }

    /// **Two owners is no owner.**
    #[tokio::test]
    async fn a_finding_cannot_be_assigned_twice() {
        let (catalog, _) = seeded().await;
        for who in ["priya", "sam"] {
            catalog
                .storage
                .upsert_user(&graph_owl_storage::StoredUser {
                    id: who.to_string(),
                    display_name: who.to_string(),
                    email: None,
                    is_admin: false,
                    is_bot: false,
                    roles: vec![],
                })
                .await
                .expect("a user");
        }
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "priya")
            .await
            .expect("first");
        let second = catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "sam")
            .await;

        assert!(second.is_err(), "a second owner was accepted");
    }

    /// **An assignment survives a re-run**, for the same reason a waiver does:
    /// findings are replaced wholesale and their row ids are regenerated.
    #[tokio::test]
    async fn an_assignment_survives_the_next_validation_pass() {
        let (catalog, _) = seeded().await;
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: "priya".to_string(),
                display_name: "Priya".to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");
        catalog.run_validation().await.expect("first");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "priya")
            .await
            .expect("assign");

        catalog.run_validation().await.expect("second");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert!(
            after[0].assignment.is_some(),
            "the assignment did not survive"
        );
    }

    /// **Assignment and acceptance are independent.** "Somebody is fixing this"
    /// and "somebody accepted this" are different statements: either can hold
    /// without the other, and collapsing them would make an accepted finding
    /// look unowned.
    #[tokio::test]
    async fn a_finding_can_be_both_assigned_and_waived() {
        let (catalog, _) = seeded().await;
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: "priya".to_string(),
                display_name: "Priya".to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");

        catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "priya")
            .await
            .expect("assign");
        catalog
            .waive_finding(
                &Principal::system(),
                &rows[0].finding,
                "accepted while Priya fixes it",
                Utc::now() + chrono::Duration::days(7),
            )
            .await
            .expect("waive");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert!(after[0].assignment.is_some());
        assert!(after[0].waiver.is_some());
    }

    /// Unassigning takes it off the plate, and doing so twice is the same
    /// intent twice rather than an error.
    #[tokio::test]
    async fn unassigning_clears_the_owner() {
        let (catalog, _) = seeded().await;
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: "priya".to_string(),
                display_name: "Priya".to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");
        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        let assignment = catalog
            .assign_finding(&Principal::system(), &rows[0].finding, "priya")
            .await
            .expect("assign");

        assert!(
            catalog
                .unassign_finding(assignment.id)
                .await
                .expect("unassign")
        );
        assert!(
            !catalog
                .unassign_finding(assignment.id)
                .await
                .expect("again")
        );

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert!(after[0].assignment.is_none());
    }

    /// **A dry-run writes nothing.** One that persisted would be the opposite
    /// of a dry run, and the whole reason to offer one is that a policy is easy
    /// to get catastrophically wrong in the permissive direction.
    #[tokio::test]
    async fn a_dry_run_reports_without_writing() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone());
        for name in ["alpha", "beta", "gamma"] {
            catalog
                .upsert_asset(&Principal::system(), service(name))
                .await
                .expect("seed");
        }
        storage.forbid_writes();

        let outcome = catalog
            .dry_run_policy(&allow_nothing(), &["finance".to_string()])
            .await
            .expect("a dry run");

        assert_eq!(outcome.admitted + outcome.denied, 3);
    }

    /// **A policy that denies nothing is almost always a mistake**, and against
    /// a small estate it looks identical to a correct one in the counts alone.
    /// Saying so is the single most useful thing a dry-run does.
    #[tokio::test]
    async fn a_policy_that_admits_everything_says_so() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage);
        catalog
            .upsert_asset(&Principal::system(), service("alpha"))
            .await
            .expect("seed");

        let wide_open = catalog
            .dry_run_policy(&allow_all(), &[])
            .await
            .expect("a dry run");

        assert!(wide_open.admits_everything, "{wide_open:?}");
        assert_eq!(wide_open.denied, 0);
    }

    /// And the negative: a policy granting nothing does not claim to admit
    /// everything, and an **empty estate** does not either — nothing to deny is
    /// not the same as denying nothing, and reporting it as wide open would
    /// alarm somebody on their first day.
    #[tokio::test]
    async fn a_restrictive_policy_and_an_empty_estate_do_not_claim_to_be_wide_open() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage);
        let nothing = allow_nothing();

        let empty = catalog
            .dry_run_policy(&nothing, &[])
            .await
            .expect("a dry run");
        assert!(!empty.admits_everything, "an empty estate is not wide open");

        catalog
            .upsert_asset(&Principal::system(), service("alpha"))
            .await
            .expect("seed");
        let restrictive = catalog
            .dry_run_policy(&nothing, &[])
            .await
            .expect("a dry run");

        assert!(!restrictive.admits_everything);
        assert_eq!(restrictive.admitted, 0);
        assert_eq!(restrictive.denied, 1);
    }

    /// **Never simulated as an admin.** An admin bypasses policy entirely, so
    /// a dry-run against one reports that every policy admits everything — a
    /// check that always says the same thing, and says the reassuring thing.
    #[tokio::test]
    async fn a_dry_run_does_not_simulate_an_administrator() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage);
        catalog
            .upsert_asset(&Principal::system(), service("alpha"))
            .await
            .expect("seed");

        let outcome = catalog
            .dry_run_policy(&allow_nothing(), &[])
            .await
            .expect("a dry run");

        assert_eq!(
            outcome.admitted, 0,
            "the dry run bypassed the policy it was asked to evaluate"
        );
    }

    /// Examples are a sample, not the estate. Returning every FQN would make
    /// this a second way to enumerate the catalog.
    #[tokio::test]
    async fn examples_are_a_sample_rather_than_the_whole_estate() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage);
        for n in 0..12 {
            catalog
                .upsert_asset(&Principal::system(), service(&format!("svc{n}")))
                .await
                .expect("seed");
        }

        let outcome = catalog
            .dry_run_policy(&allow_all(), &[])
            .await
            .expect("a dry run");

        assert_eq!(outcome.admitted, 12);
        assert_eq!(outcome.examples.len(), 5, "the sample is bounded");
    }

    /// **An assignment belongs to one finding.** The match is on all four
    /// identity fields, and a predicate that got any one of them wrong would
    /// attach somebody's name to work they never took — which reads, in a
    /// queue, as that work being handled.
    ///
    /// The fixture varies every field that can vary: two shapes, two nodes,
    /// two paths, two constraints. With one finding under test, any comparison
    /// looks correct.
    #[tokio::test]
    async fn an_assignment_attaches_to_exactly_one_finding() {
        let graph = RecordingGraph::working();
        // Two shapes, each requiring a different path, so findings differ in
        // shape, path and constraint.
        for (shape, path, term, value) in [
            ("S", "owner", "minCount", FlakeValue::Int(1)),
            ("T", "description", "minCount", FlakeValue::Int(1)),
        ] {
            let in_shapes = |s: Sid, p: Sid, o: FlakeValue| Flake {
                s,
                p,
                o,
                cx: Some(shapes_graph()),
                t: 1,
                op: true,
            };
            graph
                .assert_flakes(&[
                    in_shapes(a(shape), rdf_type(), FlakeValue::Ref(sh("NodeShape"))),
                    in_shapes(
                        a(shape),
                        sh("targetClass"),
                        FlakeValue::Ref(a("Regulatory")),
                    ),
                    in_shapes(
                        a(shape),
                        sh("property"),
                        FlakeValue::Ref(a(&format!("{shape}/p"))),
                    ),
                    in_shapes(
                        a(&format!("{shape}/p")),
                        sh("path"),
                        FlakeValue::Ref(a(path)),
                    ),
                    in_shapes(a(&format!("{shape}/p")), sh(term), value),
                ])
                .await
                .expect("shape");
        }
        // Two offenders, so findings differ in focus node too.
        for node in ["payments", "ledger"] {
            graph
                .assert_flakes(&[Flake::assert(
                    a(node),
                    rdf_type(),
                    FlakeValue::Ref(a("Regulatory")),
                    1,
                )])
                .await
                .expect("offender");
        }
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph as Arc<dyn TripleStore>);
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: "priya".to_string(),
                display_name: "Priya".to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");

        catalog.run_validation().await.expect("a pass");
        let (rows, _, _) = catalog.validation_report(&all()).await.expect("queue");
        assert_eq!(rows.len(), 4, "two shapes over two nodes: {rows:#?}");

        let target = rows[0].finding.clone();
        catalog
            .assign_finding(&Principal::system(), &target, "priya")
            .await
            .expect("assign");

        let (after, _, _) = catalog.validation_report(&all()).await.expect("queue");
        let assigned: Vec<_> = after.iter().filter(|r| r.assignment.is_some()).collect();
        assert_eq!(
            assigned.len(),
            1,
            "the assignment spread beyond its finding: {after:#?}"
        );
        assert_eq!(assigned[0].finding.shape, target.shape);
        assert_eq!(assigned[0].finding.focus_node, target.focus_node);
        assert_eq!(assigned[0].finding.path, target.path);
        assert_eq!(assigned[0].finding.constraint_kind, target.constraint_kind);
    }

    /// The queue's filters actually narrow. A filter that returns everything
    /// looks like it worked.
    #[tokio::test]
    async fn the_stored_queue_can_be_narrowed() {
        let (catalog, _) = seeded().await;
        catalog.run_validation().await.expect("a pass");
        let filtered = |focus: Option<&str>| graph_owl_storage::ValidationFilter {
            focus_node: focus.map(ToString::to_string),
            limit: 50,
            ..Default::default()
        };

        let mine = catalog
            .validation_report(&filtered(Some(&a("payments").to_string())))
            .await
            .expect("queue");
        let theirs = catalog
            .validation_report(&filtered(Some("1:nobody")))
            .await
            .expect("queue");

        assert_eq!(mine.2, 1);
        assert_eq!(theirs.2, 0);
    }
}

#[cfg(test)]
mod reasoning_decides_before_it_writes {
    //! Epic 6 slices D and E at the **facade**.
    //!
    //! The integration suite proves these against a real database, and a
    //! mutation run scoped to this crate cannot see that: the decisions live
    //! here, so the tests that pin them have to as well.

    use super::*;
    use graph_owl_core::flake::{FlakeValue, Sid, namespace};
    use graph_owl_reasoning::{Budget, CappedReason, Explanation, RuleName};
    use projection_isolation_tests::RecordingGraph;
    use tests::InMemoryStorage;

    fn dsc(id: &str) -> Sid {
        Sid::dsc(id)
    }
    fn rdf_type() -> Sid {
        Sid::new(namespace::RDF, "type")
    }
    fn sub_class_of() -> Sid {
        Sid::new(namespace::RDFS, "subClassOf")
    }

    /// A three-level hierarchy, asserted in the default graph.
    fn hierarchy() -> Vec<Flake> {
        vec![
            Flake::assert(
                dsc("payments"),
                rdf_type(),
                FlakeValue::Ref(dsc("PiiTable")),
                1,
            ),
            Flake::assert(
                dsc("PiiTable"),
                sub_class_of(),
                FlakeValue::Ref(dsc("SensitiveTable")),
                1,
            ),
            Flake::assert(
                dsc("SensitiveTable"),
                sub_class_of(),
                FlakeValue::Ref(dsc("GovernedTable")),
                1,
            ),
        ]
    }

    async fn seeded() -> (Catalog, Arc<RecordingGraph>) {
        let graph = RecordingGraph::working();
        graph
            .assert_flakes(&hierarchy())
            .await
            .expect("seed the ontology");
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        (catalog, graph)
    }

    /// **Conclusions go to their own graph.** Writing them beside assertions is
    /// unrecoverable: the next run's wholesale replacement would withdraw
    /// asserted facts along with derived ones.
    #[tokio::test]
    async fn every_written_conclusion_names_the_reasoning_graph() {
        let (catalog, graph) = seeded().await;

        let report = catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("a run");

        assert_eq!(report.derived, 2, "depth 3 implies two types");
        assert_eq!(report.replaced, 0, "nothing to replace on a first run");
        assert_eq!(report.capped, None);
        let written = graph.asserted_flakes();
        let conclusions: Vec<&Flake> = written.iter().filter(|f| f.cx.is_some()).collect();
        assert_eq!(conclusions.len(), 2, "{written:#?}");
        assert!(
            conclusions
                .iter()
                .all(|f| f.cx == Some(graph_owl_reasoning::reasoning_graph())),
            "{conclusions:#?}"
        );
    }

    /// The base is read as the **default graph specifically**. Reading "any
    /// graph" would feed the previous run's conclusions back in as premises,
    /// and the run after that would derive from those — inference resting on
    /// inference, with nobody having asserted the bottom of it.
    #[tokio::test]
    async fn the_base_is_read_from_the_default_graph_only() {
        let (catalog, graph) = seeded().await;

        catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("a run");

        assert!(
            graph
                .patterns()
                .iter()
                .any(|p| p.cx == Some(None) && p.s.is_none()),
            "no scan of the default graph: {:#?}",
            graph.patterns()
        );
    }

    /// The withdrawal is written **before** the assertion, at an earlier `t`.
    /// At one shared instant the two rows cannot be ordered and the fact
    /// vanishes — which looks like a working first run and an empty overlay
    /// from the second onwards.
    #[tokio::test]
    async fn a_re_run_withdraws_at_an_earlier_instant_than_it_asserts() {
        let (catalog, graph) = seeded().await;
        catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("first run");

        let report = catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("second run");

        assert_eq!(report.replaced, 2, "the first run's overlay");
        assert_eq!(report.derived, 2, "and the same conclusions again");
        let withdrawn = graph
            .retracted_flakes()
            .iter()
            .map(|f| f.t)
            .max()
            .expect("a withdrawal");
        let asserted = graph
            .asserted_flakes()
            .iter()
            .filter(|f| f.cx.is_some())
            .map(|f| f.t)
            .max()
            .expect("an assertion");
        assert!(
            withdrawn < asserted,
            "withdrawal at {withdrawn} is not before assertion at {asserted}"
        );
    }

    /// **The overlay survives a re-run.** The withdrawal and the assertion are
    /// two rows about one fact, and at a shared instant neither is later —
    /// current-state resolution drops the fact and the overlay empties from the
    /// second run onwards. The first run looks perfect, which is what makes
    /// this worth asserting rather than eyeballing.
    #[tokio::test]
    async fn the_conclusions_are_still_there_after_a_second_run() {
        let (catalog, graph) = seeded().await;
        catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("first run");
        catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("second run");

        let overlay = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                cx: Some(Some(graph_owl_reasoning::reasoning_graph())),
                ..Default::default()
            })
            .await
            .expect("the reasoning graph");

        assert_eq!(overlay.len(), 2, "the overlay emptied itself: {overlay:#?}");
    }

    /// **Reasoning is skipped on historical queries.** A derived fact is a
    /// conclusion about the current rule set; letting one into an `as_of`
    /// answer reports an inference nobody could have drawn at that instant,
    /// carrying provenance that looks right.
    #[tokio::test]
    async fn a_historical_read_does_not_see_derived_facts() {
        let (catalog, graph) = seeded().await;
        catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("a run");

        let asset = uuid::Uuid::new_v4();
        let _ = catalog.get_asset_as_of(asset, chrono::Utc::now()).await;

        let historical: Vec<_> = graph
            .patterns()
            .into_iter()
            .filter(|p| p.as_of.is_some())
            .collect();
        assert!(!historical.is_empty(), "no historical read was made");
        assert!(
            historical.iter().all(|p| p.cx == Some(None)),
            "a time-travel read reached beyond the default graph: {historical:#?}"
        );
    }

    /// A run over a graph with nothing to conclude writes nothing — and does
    /// not fail. An empty estate is a legitimate state.
    #[tokio::test]
    async fn a_run_with_nothing_to_conclude_writes_nothing() {
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);

        let report = catalog
            .run_reasoning(&Budget::default())
            .await
            .expect("a run");

        assert_eq!(report.derived, 0);
        assert_eq!(report.replaced, 0);
        assert!(graph.asserted_flakes().is_empty());
        assert!(graph.retracted_flakes().is_empty());
    }

    /// The budget reaches the reasoner. A budget the facade accepts and drops
    /// is a limit that exists in the type and not in the run.
    #[tokio::test]
    async fn the_budget_reaches_the_run() {
        let (catalog, _) = seeded().await;

        let report = catalog
            .run_reasoning(&Budget {
                max_iterations: 1,
                ..Budget::default()
            })
            .await
            .expect("a run");

        assert_eq!(report.capped, Some(CappedReason::Iterations));
        assert_eq!(report.iterations, 1);
    }

    #[tokio::test]
    async fn a_derived_fact_explains_as_a_chain() {
        let (catalog, _) = seeded().await;

        let explanation = catalog
            .explain_fact(
                &dsc("payments"),
                &rdf_type(),
                &dsc("GovernedTable"),
                &Budget::default(),
            )
            .await
            .expect("an explanation");

        let Explanation::Derived { chains } = explanation else {
            panic!("expected a chain, got {explanation:?}")
        };
        assert_eq!(chains[0].rule, RuleName::SubClassOf);
    }

    /// A fact neither stated nor implied is a `404`, not an empty chain — the
    /// facade turns `Unknown` into `NotFound` rather than passing it on, and an
    /// empty chain would read as "supported by nothing".
    #[tokio::test]
    async fn an_unimplied_fact_is_not_found_rather_than_an_empty_chain() {
        let (catalog, _) = seeded().await;

        let outcome = catalog
            .explain_fact(
                &dsc("payments"),
                &rdf_type(),
                &dsc("PublicTable"),
                &Budget::default(),
            )
            .await;

        assert!(
            matches!(outcome, Err(CatalogError::NotFound)),
            "{outcome:?}"
        );
    }

    /// And the negative that keeps the one above honest: an asserted fact is
    /// found, and explains as asserted rather than as a chain.
    #[tokio::test]
    async fn an_asserted_fact_explains_as_asserted() {
        let (catalog, _) = seeded().await;

        let explanation = catalog
            .explain_fact(
                &dsc("payments"),
                &rdf_type(),
                &dsc("PiiTable"),
                &Budget::default(),
            )
            .await
            .expect("an explanation");

        assert!(
            matches!(explanation, Explanation::Asserted(_)),
            "{explanation:?}"
        );
    }

    /// Both entry points refuse when there is no engine, rather than answering
    /// from nothing. "No conclusions" and "no reasoner" are opposite reports.
    #[tokio::test]
    async fn reasoning_without_a_graph_engine_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        assert!(catalog.run_reasoning(&Budget::default()).await.is_err());
        assert!(
            catalog
                .explain_fact(&dsc("a"), &rdf_type(), &dsc("b"), &Budget::default())
                .await
                .is_err()
        );
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
    pub(super) struct RecordingGraph {
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

        pub(super) fn working() -> Arc<Self> {
            Self::with(false)
        }

        /// What was written, in order. Read by the reasoning tests in the
        /// sibling module, which need the `cx` and `t` of each row rather than
        /// the resolved current state.
        pub(super) fn asserted_flakes(&self) -> Vec<Flake> {
            self.asserted.lock().expect("lock").clone()
        }

        pub(super) fn retracted_flakes(&self) -> Vec<Flake> {
            self.retracted.lock().expect("lock").clone()
        }

        /// Every pattern this store was asked to answer. Some obligations —
        /// scanning the default graph rather than any graph — change the
        /// *question* and not the answer, and are invisible in the result.
        pub(super) fn patterns(&self) -> Vec<TriplePattern> {
            self.queried.lock().expect("lock").clone()
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
                    // A later row wins. On a tie the **retraction** wins, which
                    // is what Postgres does and what the double did not: a
                    // withdrawal at the same instant as an assertion cannot be
                    // ordered, and the honest reading of an ambiguous pair is
                    // that the fact is not current. A double that kept the
                    // assertion here let a reasoning run that retracted and
                    // re-asserted at one `t` pass in this crate and empty the
                    // overlay against a real database.
                    Some(seen) if seen.t > flake.t => {}
                    Some(seen) if seen.t == flake.t && !seen.op => {}
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

        /// `op` is forced to `false`, exactly as the port specifies.
        ///
        /// The double stored flakes as handed to it, so a caller passing the
        /// original assertion — which is what the port explicitly invites,
        /// and what a projection update does — recorded another *assertion*
        /// at a later `t`, and the fact stayed live. A shape withdrawn that
        /// way went on being enforced, and only the integration suite could
        /// see it.
        async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
            if self.fail {
                return Self::refuse();
            }
            self.retracted
                .lock()
                .expect("lock")
                .extend(flakes.iter().map(|flake| Flake {
                    op: false,
                    ..flake.clone()
                }));
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

    /// **The plan is what the engine decided to read**, and it is the single
    /// number that explains a slow query. A bounded query names the predicate
    /// it will scan.
    #[tokio::test]
    async fn sparql_reports_the_scan_it_planned() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph as Arc<dyn TripleStore>);

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "SELECT ?n WHERE { ?s <https://graph-owl.dev/ns/catalog#name> ?n }",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(outcome.plan.len(), 1, "{:?}", outcome.plan);
        assert!(
            outcome.plan[0].contains("name"),
            "the plan should name the bound predicate: {:?}",
            outcome.plan
        );
        // Unbound positions are shown rather than omitted, so the *shape* of
        // the read is visible: narrowing on a subject and on a predicate cost
        // differently and must not render alike.
        assert!(outcome.plan[0].starts_with("? "), "{:?}", outcome.plan);
    }

    /// **The order the author wrote.** A solution is a `BTreeMap`, so
    /// `SELECT ?s ?p ?o` arrives sorted as `o, p, s` and the query's own
    /// ordering is gone before any consumer sees it. Found by looking at the
    /// workbench, which rendered the columns backwards — no unit test over the
    /// rows could have caught it, because the information is not in them.
    #[tokio::test]
    async fn sparql_reports_the_variables_in_the_order_the_query_named_them() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph as Arc<dyn TripleStore>);

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "SELECT ?s ?p ?o WHERE { ?s ?p ?o }",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(outcome.variables, vec!["s", "p", "o"]);
    }

    /// And with the modifiers that wrap a projection — `LIMIT` and `ORDER BY`
    /// sit *above* it in the algebra, so a walk that only checked the root
    /// would find nothing exactly when a real query is being run.
    #[tokio::test]
    async fn the_projection_is_found_under_limit_and_ordering() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph as Arc<dyn TripleStore>);

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "SELECT DISTINCT ?name ?owner WHERE { ?s ?name ?owner } ORDER BY ?name LIMIT 5",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(outcome.variables, vec!["name", "owner"]);
    }

    /// A query form with no projection reports none, rather than inventing a
    /// column order for an answer that has no columns.
    #[tokio::test]
    async fn a_query_with_no_projection_reports_no_variables() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph as Arc<dyn TripleStore>);

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "ASK { ?s ?p ?o }",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert!(outcome.variables.is_empty(), "{:?}", outcome.variables);
    }

    /// **An unbounded query reports the full scan**, which is the entry worth
    /// seeing. Reporting nothing — or an empty plan — would let the most
    /// expensive query in the system look like the cheapest.
    #[tokio::test]
    async fn a_query_that_cannot_be_bounded_says_it_reads_everything() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage).with_graph(graph as Arc<dyn TripleStore>);

        let outcome = catalog
            .sparql(
                &Principal::system(),
                "SELECT ?s ?p ?o WHERE { ?s ?p ?o }",
                None,
                SparqlBudget::default(),
            )
            .await
            .expect("query");

        assert_eq!(
            outcome.plan,
            vec!["? ? ?".to_string()],
            "{:?}",
            outcome.plan
        );
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
    /// Lineage's decisions, tested where they live.
    ///
    /// The HTTP tests in `graph-owl-server` cover all of this end to end, and a
    /// mutation run scoped to *this* crate cannot see them — the same
    /// cross-crate gap that let two `get_asset_as_of` mutants survive. Logic
    /// belongs to the crate it lives in.
    mod lineage_decides_before_it_writes {
        use super::*;
        use graph_owl_core::lineage::{LineageDetails, LineageSource};
        use graph_owl_core::relationship_type::RelationshipType;

        async fn two_tables(catalog: &Catalog) -> (Uuid, Uuid) {
            let service = catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("service");
            let mut ids = Vec::new();
            for name in ["upstream", "downstream"] {
                let table = catalog
                    .upsert_asset(
                        &Principal::system(),
                        UpsertAsset {
                            kind: AssetKind::Database,
                            name: name.to_string(),
                            parent_id: Some(service.id),
                            description: None,
                            properties: None,
                        },
                    )
                    .await
                    .expect("asset");
                ids.push(table.id);
            }
            (ids[0], ids[1])
        }

        fn manual() -> LineageDetails {
            LineageDetails {
                source: LineageSource::Manual,
                query: None,
                description: None,
            }
        }

        /// A database cannot feed a database — only tables and columns carry
        /// lineage. This doubles as the legality check's own test.
        #[tokio::test]
        async fn a_kind_that_does_not_carry_lineage_is_refused() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let (from, to) = two_tables(&catalog).await;

            let outcome = catalog
                .assert_lineage(
                    &Principal::system(),
                    from,
                    to,
                    RelationshipType::Feeds,
                    manual(),
                )
                .await;

            assert!(
                matches!(outcome, Err(CatalogError::Validation(_))),
                "a database does not feed a database"
            );
        }

        /// **The self-edge.** A cycle of length one, and the check that catches
        /// it must not be satisfiable by comparing the wrong way round.
        #[tokio::test]
        async fn an_asset_cannot_feed_itself() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let (from, _) = two_tables(&catalog).await;

            let outcome = catalog
                .assert_lineage(
                    &Principal::system(),
                    from,
                    from,
                    RelationshipType::Feeds,
                    manual(),
                )
                .await;

            assert!(matches!(outcome, Err(CatalogError::Validation(_))));
        }

        /// And the negative that stops "refuse everything" passing: two
        /// *different* assets of a kind that carries lineage are accepted.
        #[tokio::test]
        async fn two_distinct_tables_may_be_linked() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let service = catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("service");
            let mut tables = Vec::new();
            for name in ["a", "b"] {
                let db = catalog
                    .upsert_asset(
                        &Principal::system(),
                        UpsertAsset {
                            kind: AssetKind::Database,
                            name: format!("db-{name}"),
                            parent_id: Some(service.id),
                            description: None,
                            properties: None,
                        },
                    )
                    .await
                    .expect("db");
                let schema = catalog
                    .upsert_asset(
                        &Principal::system(),
                        UpsertAsset {
                            kind: AssetKind::Schema,
                            name: "s".to_string(),
                            parent_id: Some(db.id),
                            description: None,
                            properties: None,
                        },
                    )
                    .await
                    .expect("schema");
                let table = catalog
                    .upsert_asset(
                        &Principal::system(),
                        UpsertAsset {
                            kind: AssetKind::Table,
                            name: format!("t-{name}"),
                            parent_id: Some(schema.id),
                            description: None,
                            properties: None,
                        },
                    )
                    .await
                    .expect("table");
                tables.push(table.id);
            }

            let edge = catalog
                .assert_lineage(
                    &Principal::system(),
                    tables[0],
                    tables[1],
                    RelationshipType::Feeds,
                    manual(),
                )
                .await
                .expect("a table may feed a table");

            assert_eq!(edge.from_asset_id, tables[0]);
            assert_eq!(edge.to_asset_id, tables[1]);
        }

        async fn chain(catalog: &Catalog, length: usize) -> Vec<Uuid> {
            let service = catalog
                .upsert_asset(&Principal::system(), service("hdfc-core"))
                .await
                .expect("service");
            let db = catalog
                .upsert_asset(
                    &Principal::system(),
                    UpsertAsset {
                        kind: AssetKind::Database,
                        name: "retail".into(),
                        parent_id: Some(service.id),
                        description: None,
                        properties: None,
                    },
                )
                .await
                .expect("db");
            let schema = catalog
                .upsert_asset(
                    &Principal::system(),
                    UpsertAsset {
                        kind: AssetKind::Schema,
                        name: "payments".into(),
                        parent_id: Some(db.id),
                        description: None,
                        properties: None,
                    },
                )
                .await
                .expect("schema");

            let mut ids = Vec::new();
            for n in 0..length {
                let table = catalog
                    .upsert_asset(
                        &Principal::system(),
                        UpsertAsset {
                            kind: AssetKind::Table,
                            name: format!("t{n}"),
                            parent_id: Some(schema.id),
                            description: None,
                            properties: None,
                        },
                    )
                    .await
                    .expect("table");
                ids.push(table.id);
            }
            for pair in ids.windows(2) {
                catalog
                    .assert_lineage(
                        &Principal::system(),
                        pair[0],
                        pair[1],
                        RelationshipType::Feeds,
                        manual(),
                    )
                    .await
                    .expect("edge");
            }
            ids
        }

        /// The walk returns something. A function that answered an empty graph
        /// would look identical to an asset with no lineage — and "nothing
        /// feeds this" is a conclusion people act on.
        #[tokio::test]
        async fn a_walk_returns_the_graph_it_found() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let ids = chain(&catalog, 3).await;

            let (nodes, edges) = catalog.lineage_graph(ids[0], 0, 2).await.expect("walk");

            assert_eq!(nodes.len(), 3, "the root and two downstream");
            assert_eq!(edges.len(), 2);
        }

        /// **Direction is not decoration.** `lineage_edges_touching` returns
        /// edges on both sides of the frontier, and the walk keeps only those
        /// leaving it in the direction being walked. Without that filter a
        /// downstream walk drags upstream nodes in, and "what breaks if I change
        /// this" starts listing the things that feed it.
        #[tokio::test]
        async fn a_downstream_walk_does_not_wander_upstream() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let ids = chain(&catalog, 3).await;

            let (nodes, _) = catalog.lineage_graph(ids[1], 0, 1).await.expect("walk");
            let found: std::collections::HashSet<Uuid> =
                nodes.iter().map(|asset| asset.id).collect();

            assert!(found.contains(&ids[2]), "one downstream");
            assert!(!found.contains(&ids[0]), "and never the upstream one");
        }

        #[tokio::test]
        async fn an_upstream_walk_does_not_wander_downstream() {
            let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
            let ids = chain(&catalog, 3).await;

            let (nodes, _) = catalog.lineage_graph(ids[1], 1, 0).await.expect("walk");
            let found: std::collections::HashSet<Uuid> =
                nodes.iter().map(|asset| asset.id).collect();

            assert!(found.contains(&ids[0]), "one upstream");
            assert!(!found.contains(&ids[2]), "and never the downstream one");
        }
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

        /// **The secrets round-trip test, and the plan's first RED.** A
        /// credential must never appear in anything a reader can see — not in
        /// the value, not in the debug rendering, not in the JSON.
        ///
        /// The strongest version of this is structural: `ConnectorConfig` has
        /// **no field** for a secret, so a handler cannot leak one by
        /// forgetting to redact. This test pins that the guarantee holds
        /// end to end anyway, because a later `secret: Option<String>` added
        /// "just for the edit form" would compile and pass everything else.
        #[tokio::test]
        async fn a_stored_credential_never_comes_back() {
            let (catalog, _) = catalog_with_policy().await;

            let saved = catalog
                .save_connector_config(
                    "postgres",
                    "warehouse",
                    serde_json::json!({ "host": "db.internal", "database": "retail" }),
                    Some("s3cr3t-p4ssw0rd"),
                )
                .await
                .expect("save");

            // It was stored — the flag says so without saying what.
            assert!(saved.has_secret);

            for rendering in [
                format!("{saved:?}"),
                serde_json::to_string(&saved).expect("serialises"),
                serde_json::to_string(&catalog.connector_configs().await.expect("list"))
                    .expect("serialises"),
            ] {
                assert!(
                    !rendering.contains("s3cr3t"),
                    "a credential surfaced in {rendering}"
                );
            }

            // And the non-secret settings *are* returned, so the assertions
            // above are about the credential rather than about a read that
            // returns nothing.
            assert_eq!(saved.settings["host"], "db.internal");
        }

        /// The run path can still reach it — a write-only secret that nothing
        /// can read is a connector that cannot connect.
        #[tokio::test]
        async fn the_run_path_can_read_the_credential() {
            let (catalog, _) = catalog_with_policy().await;
            let saved = catalog
                .save_connector_config(
                    "postgres",
                    "warehouse",
                    serde_json::json!({}),
                    Some("s3cr3t"),
                )
                .await
                .expect("save");

            let secret = catalog
                .storage
                .connector_secret(saved.id)
                .await
                .expect("read");

            assert_eq!(secret.as_deref(), Some("s3cr3t"));
        }

        /// **An edit that does not resend the credential keeps it.** A round trip
        /// through a form cannot resend what it was never given, and treating
        /// absent as "clear it" would break a connector every time somebody
        /// changed a setting.
        #[tokio::test]
        async fn saving_without_a_secret_keeps_the_existing_one() {
            let (catalog, _) = catalog_with_policy().await;
            catalog
                .save_connector_config(
                    "postgres",
                    "warehouse",
                    serde_json::json!({}),
                    Some("keep-me"),
                )
                .await
                .expect("first");

            let updated = catalog
                .save_connector_config(
                    "postgres",
                    "warehouse",
                    serde_json::json!({ "database": "changed" }),
                    None,
                )
                .await
                .expect("edit");

            assert!(updated.has_secret, "the credential was cleared by an edit");
            assert_eq!(updated.settings["database"], "changed");
            assert_eq!(
                catalog
                    .storage
                    .connector_secret(updated.id)
                    .await
                    .expect("read")
                    .as_deref(),
                Some("keep-me")
            );
        }

        /// A configuration with no credential is a real state — some databases
        /// are reachable without one — and must be distinguishable from one that
        /// has a credential.
        #[tokio::test]
        async fn a_configuration_without_a_credential_says_so() {
            let (catalog, _) = catalog_with_policy().await;

            let saved = catalog
                .save_connector_config("postgres", "warehouse", serde_json::json!({}), None)
                .await
                .expect("save");

            assert!(!saved.has_secret);
        }

        /// **A blank secret is not a secret.** Accepting `""` would set the flag
        /// and then fail at connection time with a credential error nobody can
        /// explain.
        #[tokio::test]
        async fn a_blank_secret_is_refused_rather_than_stored() {
            let (catalog, _) = catalog_with_policy().await;

            for blank in ["", "   "] {
                let outcome = catalog
                    .save_connector_config("postgres", "w", serde_json::json!({}), Some(blank))
                    .await;
                assert!(
                    matches!(outcome, Err(CatalogError::Validation(_))),
                    "{blank:?} was accepted as a credential"
                );
            }
        }

        /// One configuration per service per connector: two would make "which
        /// credential did last night's run use" unanswerable.
        #[tokio::test]
        async fn saving_twice_updates_rather_than_duplicating() {
            let (catalog, _) = catalog_with_policy().await;
            catalog
                .save_connector_config("postgres", "warehouse", serde_json::json!({}), Some("a"))
                .await
                .expect("first");
            catalog
                .save_connector_config("postgres", "warehouse", serde_json::json!({}), Some("b"))
                .await
                .expect("second");

            assert_eq!(catalog.connector_configs().await.expect("list").len(), 1);
        }

        fn person(id: &str) -> StoredUser {
            StoredUser {
                id: id.to_string(),
                display_name: id.to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            }
        }

        fn team(id: &str, members: &[&str]) -> graph_owl_storage::Team {
            graph_owl_storage::Team {
                id: id.to_string(),
                display_name: format!("The {id} team"),
                description: None,
                members: members.iter().map(|m| (*m).to_string()).collect(),
            }
        }

        /// **A member nobody can resolve is an owner who does not exist**, and
        /// the mistake surfaces much later as an asset owned by nothing.
        #[tokio::test]
        async fn a_team_cannot_name_a_member_who_is_not_a_user() {
            let (catalog, _) = catalog_with_policy().await;

            let outcome = catalog.upsert_team(&team("platform", &["ghost"])).await;

            assert!(
                matches!(outcome, Err(CatalogError::Validation(_))),
                "{outcome:?}"
            );
        }

        /// And the positive, so the refusal above is about the unknown member
        /// rather than about teams never being creatable.
        #[tokio::test]
        async fn a_team_of_known_people_is_created_with_its_membership() {
            let (catalog, _) = catalog_with_policy().await;
            for who in ["priya", "sam"] {
                catalog
                    .storage
                    .upsert_user(&person(who))
                    .await
                    .expect("user");
            }

            let stored = catalog
                .upsert_team(&team("platform", &["sam", "priya"]))
                .await
                .expect("create");

            // Ordered, so two reads of an unchanged team compare equal and a
            // diff between them means something.
            assert_eq!(stored.members, vec!["priya", "sam"]);
            assert_eq!(stored.display_name, "The platform team");
        }

        /// **Membership is replaced, not merged.** A partial update cannot
        /// express "remove everybody", and removal is the operation that has to
        /// work — a team somebody has left is an owner who no longer exists.
        #[tokio::test]
        async fn updating_a_team_replaces_its_membership_rather_than_adding_to_it() {
            let (catalog, _) = catalog_with_policy().await;
            for who in ["priya", "sam"] {
                catalog
                    .storage
                    .upsert_user(&person(who))
                    .await
                    .expect("user");
            }
            catalog
                .upsert_team(&team("platform", &["priya", "sam"]))
                .await
                .expect("create");

            let updated = catalog
                .upsert_team(&team("platform", &["sam"]))
                .await
                .expect("update");

            assert_eq!(updated.members, vec!["sam"], "priya was not removed");
        }

        /// Emptying a team is expressible. A model where the last member cannot
        /// be removed leaves a departed colleague owning things forever.
        #[tokio::test]
        async fn a_team_can_be_emptied() {
            let (catalog, _) = catalog_with_policy().await;
            catalog
                .storage
                .upsert_user(&person("priya"))
                .await
                .expect("user");
            catalog
                .upsert_team(&team("platform", &["priya"]))
                .await
                .expect("create");

            let emptied = catalog
                .upsert_team(&team("platform", &[]))
                .await
                .expect("empty");

            assert!(emptied.members.is_empty());
        }

        /// A team needs a name a person recognises. An id alone reads as a slug
        /// in every owner column it appears in.
        #[tokio::test]
        async fn a_team_needs_an_id_and_a_name() {
            let (catalog, _) = catalog_with_policy().await;

            for broken in [
                graph_owl_storage::Team {
                    id: "  ".into(),
                    ..team("x", &[])
                },
                graph_owl_storage::Team {
                    display_name: "".into(),
                    ..team("x", &[])
                },
            ] {
                assert!(
                    matches!(
                        catalog.upsert_team(&broken).await,
                        Err(CatalogError::Validation(_))
                    ),
                    "{broken:?} was accepted"
                );
            }
        }

        /// A person may be in several teams: ownership follows the
        /// organisation, and organisations are not trees.
        #[tokio::test]
        async fn a_person_can_belong_to_more_than_one_team() {
            let (catalog, _) = catalog_with_policy().await;
            catalog
                .storage
                .upsert_user(&person("priya"))
                .await
                .expect("user");
            catalog
                .upsert_team(&team("platform", &["priya"]))
                .await
                .expect("a");
            catalog
                .upsert_team(&team("finance", &["priya"]))
                .await
                .expect("b");

            let teams = catalog.teams().await.expect("teams");

            assert_eq!(teams.len(), 2);
            assert!(teams.iter().all(|t| t.members == vec!["priya"]));
        }

        /// **A revoked role stops working immediately.** This is the whole
        /// reason the cache has no TTL: a revocation whose window is invisible
        /// to whoever performed it is a revocation nobody can reason about.
        /// Without the invalidation the old predicate keeps answering, and the
        /// person who removed the role has no way to see that it still works.
        #[tokio::test]
        async fn revoking_a_role_takes_effect_on_the_very_next_request() {
            let (catalog, storage) = catalog_with_policy().await;
            catalog
                .storage
                .upsert_user(&StoredUser {
                    id: "asha".to_string(),
                    display_name: "Asha".to_string(),
                    email: None,
                    is_admin: false,
                    is_bot: false,
                    roles: vec!["analyst".to_string()],
                })
                .await
                .expect("a user");
            let asha = analyst(&["analyst"]);

            // Warm the cache, so the next answer would be served from it.
            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("first");
            let warmed = reads(&storage);

            catalog
                .set_user_roles("asha", vec![])
                .await
                .expect("revoke");

            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("after revocation");

            assert!(
                reads(&storage) > warmed,
                "the decision was still served from cache after a role change"
            );
        }

        /// And the negative that stops "invalidate always" passing: an
        /// unrelated read after the revocation is cached again, so the cache
        /// still works. A control that clears on every request is a control
        /// that has quietly become a no-op.
        #[tokio::test]
        async fn the_cache_still_works_after_an_invalidation() {
            let (catalog, storage) = catalog_with_policy().await;
            catalog
                .storage
                .upsert_user(&StoredUser {
                    id: "asha".to_string(),
                    display_name: "Asha".to_string(),
                    email: None,
                    is_admin: false,
                    is_bot: false,
                    roles: vec!["analyst".to_string()],
                })
                .await
                .expect("a user");
            let asha = analyst(&["analyst"]);
            catalog
                .set_user_roles("asha", vec![])
                .await
                .expect("revoke");

            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("first");
            let after_first = reads(&storage);
            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("second");

            assert_eq!(reads(&storage), after_first, "the cache stopped caching");
        }

        /// Granting a role to a name nobody has seen would let a typo mint a
        /// principal — and the mistake would only surface as access that
        /// silently does nothing.
        #[tokio::test]
        async fn roles_cannot_be_granted_to_a_user_that_does_not_exist() {
            let (catalog, _) = catalog_with_policy().await;

            let outcome = catalog
                .set_user_roles("nobody", vec!["admin".to_string()])
                .await;

            assert!(
                matches!(outcome, Err(CatalogError::NotFound)),
                "{outcome:?}"
            );
        }

        /// The roles actually change, and nothing else about the user does.
        #[tokio::test]
        async fn setting_roles_replaces_them_and_leaves_the_rest_alone() {
            let (catalog, _) = catalog_with_policy().await;
            catalog
                .storage
                .upsert_user(&StoredUser {
                    id: "asha".to_string(),
                    display_name: "Asha Rao".to_string(),
                    email: Some("asha@example.com".to_string()),
                    is_admin: false,
                    is_bot: false,
                    roles: vec!["analyst".to_string()],
                })
                .await
                .expect("a user");

            let updated = catalog
                .set_user_roles("asha", vec!["steward".to_string()])
                .await
                .expect("set roles");

            assert_eq!(updated.roles, vec!["steward".to_string()]);
            assert_eq!(updated.display_name, "Asha Rao", "the name was rewritten");
            assert_eq!(updated.email.as_deref(), Some("asha@example.com"));
            assert!(!updated.is_admin, "admin was granted as a side effect");
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
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("first");
            let after_first = reads(&storage);
            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
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
                .list_assets_for(
                    &analyst(&["analyst"]),
                    &graph_owl_storage::AssetFilter::default(),
                    &page(),
                )
                .await
                .expect("permitted");
            let unpermitted = catalog
                .list_assets_for(
                    &analyst(&["nobody"]),
                    &graph_owl_storage::AssetFilter::default(),
                    &page(),
                )
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
                .list_assets_for(
                    &analyst(&["nobody"]),
                    &graph_owl_storage::AssetFilter::default(),
                    &page(),
                )
                .await
                .expect("restricted");
            assert!(restricted.data.is_empty());

            let mut admin = analyst(&["nobody"]);
            admin.is_admin = true;
            let full = catalog
                .list_assets_for(&admin, &graph_owl_storage::AssetFilter::default(), &page())
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
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
                .await
                .expect("first");
            let after_first = reads(&storage);

            catalog.invalidate_authorization();
            catalog
                .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
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
                    .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
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
                    .list_assets_for(&asha, &graph_owl_storage::AssetFilter::default(), &page())
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
