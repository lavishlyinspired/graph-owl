use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use graph_owl_authz::{
    AccessPredicate, DecisionCache, DecisionKey, MetadataOperation, Policy, Subject, compile,
};
use graph_owl_connectors::DeletionPlan;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use graph_owl_core::projection;
use graph_owl_core::resolution::{Candidate, Evidence, MergeDecidedBy, MergeRecord, Resolution};
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
use graph_owl_resolution::bands::{ConfidenceBands, Decision, decide};
use graph_owl_resolution::normalize::is_deterministic_match;
use graph_owl_resolution::score::{EntityView, ScoreWeights, evidence, score};
use graph_owl_storage::{
    ConflictKind, SplitOutcome, Storage, StorageError, StoredUser, UpdateOutcome,
};
use graph_owl_traversal::{Bounds, Direction, EdgeFilter, Subgraph, TraversalEngine};
use serde::Deserialize;
use uuid::Uuid;

pub mod extraction;
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
        if let Some(kind) = value.get("kind").and_then(serde_json::Value::as_str)
            && AssetKind::parse(kind).is_err()
        {
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
    /// The caller is known and the thing they asked for is visible, but they
    /// specifically may not do this — Epic 24 Slice C's "only an assigned
    /// reviewer may approve, others `403`". Distinct from `Validation`: the
    /// request would be accepted from a different, permitted caller, so the
    /// fix is not a different value but a different actor.
    Forbidden,
    /// The caller has not proven who they claim to be — Epic 18's webhook
    /// signature verification. Distinct from `Forbidden`: a bad or missing
    /// signature is "we do not believe this is who it says it is", not "we
    /// know who this is and they may not do this" — the `401` vs `403`
    /// distinction RFC 9110 draws.
    Unauthenticated,
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
    /// Whether the `>= 0.9` band writes a merge automatically, or is
    /// downgraded to a queued `Ambiguous` for a human to confirm — Slice D's
    /// "auto-merge is disableable per deployment". **Defaults to disabled**
    /// per the plan's own pre-PR quality gate ("auto-merge is off by default
    /// in the shipped config; enabling it is a deliberate operator
    /// decision") — a deployment opts in explicitly via
    /// [`Self::with_auto_merge_enabled`], rather than every deployment
    /// silently getting automatic merges the day this field was added.
    ///
    /// Deterministic FQN matching (Slice A) is unaffected either way — it is
    /// a different, more certain mechanism than this confidence-band toggle.
    auto_merge_enabled: bool,
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
            auto_merge_enabled: false,
        }
    }

    /// Disables (or re-enables) automatic merging of `>= 0.9` matches. When
    /// disabled, a would-be auto-merge is downgraded to `Ambiguous` instead —
    /// still surfaced, never silently dropped.
    #[must_use]
    pub fn with_auto_merge_enabled(mut self, enabled: bool) -> Self {
        self.auto_merge_enabled = enabled;
        self
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
    // ---- Epic 21: the surface an out-of-process worker submits to ----

    /// Accept a worker's extraction output and apply the policy to it.
    ///
    /// The subject check is resolved here rather than passed in, because
    /// "is this a known entity" is a question only the catalog can answer and
    /// a worker that could answer it would be one that had to be given the
    /// catalog's contents.
    ///
    /// # Errors
    ///
    /// `Storage` if the run cannot be read or written.
    pub async fn submit_extraction(
        &self,
        document: &graph_owl_core::extraction::ParsedDocument,
        result: graph_owl_core::extraction::ExtractionResult,
        extractor: &str,
        version: &str,
    ) -> Result<extraction::SubmissionOutcome, CatalogError> {
        // Resolved for the **distinct** subjects this document names, before
        // the policy runs. Two things this is not: not a query per claim (a
        // document naming one table forty times would pay forty round trips to
        // answer the same question), and not a page of every asset in the
        // catalog (bounded by the catalog's size rather than the document's,
        // and silently wrong past the first page).
        let mut wanted: Vec<&str> = result
            .claims
            .iter()
            .map(|claim| claim.subject.as_str())
            .collect();
        wanted.sort_unstable();
        wanted.dedup();

        let mut subjects = std::collections::HashSet::new();
        for fqn in wanted {
            if self
                .storage
                .get_asset_by_fqn(fqn)
                .await
                .map_err(CatalogError::from)?
                .is_some()
            {
                subjects.insert(fqn.to_string());
            }
        }

        extraction::submit(
            self.storage.as_ref(),
            document,
            result,
            extractor,
            version,
            &|fqn: &str| subjects.contains(fqn),
        )
        .await
        .map_err(CatalogError::from)
    }

    /// Claims waiting for a human, with the sentence each came from.
    ///
    /// **The evidence text is resolved here, from the run's stored source.** A
    /// reviewer asked to confirm "the orders table is append-only" without
    /// seeing the sentence it came from is being asked to trust the extractor,
    /// which is the thing under review.
    ///
    /// # Errors
    ///
    /// `Storage` if the queue cannot be read.
    pub async fn extraction_queue(&self) -> Result<Vec<extraction::PendingClaim>, CatalogError> {
        let claims = self
            .storage
            .pending_extraction_claims(extraction::QUEUE_PAGE)
            .await
            .map_err(CatalogError::from)?;

        let mut pending = Vec::with_capacity(claims.len());
        for claim in claims {
            // Spans are byte offsets into the run's own `source_text`, and an
            // out-of-range one is untrusted input rather than a bug here — a
            // worker that miscounts must not be able to panic the reviewer's
            // page.
            let evidence = self
                .extraction_evidence(claim.run_id, claim.evidence_start, claim.evidence_end)
                .await;
            pending.push(extraction::PendingClaim {
                id: claim.id,
                run_id: claim.run_id,
                subject: claim.subject,
                predicate: claim.predicate,
                object: claim.object,
                confidence: claim.confidence,
                evidence,
            });
        }
        Ok(pending)
    }

    async fn extraction_evidence(&self, run_id: Uuid, start: i32, end: i32) -> String {
        const UNRESOLVED: &str = "(the evidence span does not resolve against the source)";
        let Ok(Some(run)) = self.storage.find_extraction_run_by_id(run_id).await else {
            return UNRESOLVED.to_string();
        };
        let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
            return UNRESOLVED.to_string();
        };
        run.source_text
            .get(start..end)
            .map_or_else(|| UNRESOLVED.to_string(), |text| text.trim().to_string())
    }

    /// Record a reviewer's decision on a queued claim.
    ///
    /// # Errors
    ///
    /// `NotFound` if the claim does not exist; `Storage` if the write fails.
    pub async fn decide_extraction_claim(
        &self,
        claim_id: Uuid,
        confirmed: bool,
        decided_by: &str,
    ) -> Result<graph_owl_storage::QueuedClaimRecord, CatalogError> {
        self.storage
            .decide_extraction_claim(claim_id, confirmed, decided_by)
            .await
            .map_err(CatalogError::from)?
            .ok_or(CatalogError::NotFound)
    }

    /// Delete a run and everything it produced.
    ///
    /// **This is what decision 0 buys.** Extraction is scoped so that a bad
    /// run — a mis-prompted model, a broken OCR pass — is one delete rather
    /// than a hunt through the graph for facts nothing can attribute.
    ///
    /// # Errors
    ///
    /// `NotFound` if the run does not exist; `Storage` if the delete fails.
    pub async fn delete_extraction_run(&self, run_id: Uuid) -> Result<(), CatalogError> {
        if self
            .storage
            .delete_extraction_run(run_id)
            .await
            .map_err(CatalogError::from)?
        {
            Ok(())
        } else {
            Err(CatalogError::NotFound)
        }
    }

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

    // ---- Epic 18 Slice A: webhooks ----

    /// Registers or updates a webhook endpoint. `secret` is the raw key
    /// material (an HMAC shared secret, or an Ed25519 public verifying
    /// key); `None` leaves an existing one alone.
    ///
    /// # Errors
    ///
    /// `Conflict` if `path` is already registered to a different endpoint.
    #[tracing::instrument(name = "catalog.register_webhook_endpoint", skip_all)]
    pub async fn register_webhook_endpoint(
        &self,
        endpoint: graph_owl_storage::WebhookEndpoint,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::WebhookEndpoint, CatalogError> {
        Ok(self
            .storage
            .upsert_webhook_endpoint(endpoint, secret)
            .await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn webhook_endpoint(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, CatalogError> {
        Ok(self.storage.get_webhook_endpoint(id).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn webhook_endpoint_by_path(
        &self,
        path: &str,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, CatalogError> {
        Ok(self.storage.get_webhook_endpoint_by_path(path).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn list_webhook_endpoints(
        &self,
    ) -> Result<Vec<graph_owl_storage::WebhookEndpoint>, CatalogError> {
        Ok(self.storage.list_webhook_endpoints().await?)
    }

    /// # Errors
    ///
    /// `Storage::Conflict` if `(topic, consumer_group)` is already
    /// registered to a different subscription; `Storage` if the write
    /// fails.
    pub async fn register_stream_subscription(
        &self,
        subscription: graph_owl_storage::StreamSubscription,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::StreamSubscription, CatalogError> {
        Ok(self
            .storage
            .upsert_stream_subscription(subscription, secret)
            .await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn stream_subscription(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StreamSubscription>, CatalogError> {
        Ok(self.storage.get_stream_subscription(id).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn list_stream_subscriptions(
        &self,
    ) -> Result<Vec<graph_owl_storage::StreamSubscription>, CatalogError> {
        Ok(self.storage.list_stream_subscriptions().await?)
    }

    /// Verifies an inbound delivery's signature and, if it verifies, records
    /// it. The HTTP layer is responsible for reading `raw_body` before any
    /// JSON parsing and for extracting exactly the header `endpoint`'s own
    /// scheme names — this function has no notion of an HTTP request, only
    /// of the bytes an unverified one might have lied about.
    ///
    /// # Errors
    ///
    /// `NotFound` if `endpoint` is disabled — an existence signal is
    /// unnecessary (Slice E). `Unauthenticated` if the signature is missing
    /// or does not verify. `Storage` if the write fails.
    #[tracing::instrument(name = "catalog.receive_webhook", skip_all)]
    pub async fn receive_webhook(
        &self,
        endpoint: &graph_owl_storage::WebhookEndpoint,
        signature_header_value: Option<&str>,
        raw_body: &[u8],
    ) -> Result<graph_owl_core::webhook::InboundEvent, CatalogError> {
        if !endpoint.enabled {
            return Err(CatalogError::NotFound);
        }

        let secret = self
            .storage
            .webhook_secret(endpoint.id)
            .await?
            .unwrap_or_default();
        let verified = match &endpoint.signature_scheme {
            graph_owl_storage::SignatureScheme::HmacSha256 { prefix, .. } => signature_header_value
                .is_some_and(|value| {
                    graph_owl_connectors::webhook_signature::verify_hmac_sha256(
                        &secret, raw_body, value, prefix,
                    )
                }),
            graph_owl_storage::SignatureScheme::Ed25519 { .. } => signature_header_value
                .is_some_and(|value| {
                    secret.as_slice().try_into().is_ok_and(|key: [u8; 32]| {
                        graph_owl_connectors::webhook_signature::verify_ed25519(
                            &key, raw_body, value,
                        )
                    })
                }),
        };
        if !verified {
            // Named by endpoint id only — never the secret, the header value,
            // or the body, all of which an operator's log aggregator would
            // otherwise persist indefinitely.
            tracing::warn!(endpoint = %endpoint.id, "webhook delivery did not verify");
            return Err(CatalogError::Unauthenticated);
        }

        // Malformed JSON is checked synchronously, here, rather than left
        // for the async mapping step — Epic 18 Slice E's own criterion:
        // "malformed JSON after a valid signature → 400 and DLQ, not a
        // panic" names a synchronous response, and mapping/shape checks are
        // the only reasons this pipeline is otherwise asynchronous at all.
        // A signature can verify over any bytes; only now, once verified,
        // does it become worth looking at what they contain.
        let is_valid_json = serde_json::from_slice::<serde_json::Value>(raw_body).is_ok();

        // `sender_event_id` stays `None` until Slice C's declarative mapping
        // can extract one from the payload — every delivery through this
        // pipeline is content-hash deduped today, which `dedup_key` already
        // does correctly with no sender id to prefer.
        let event = graph_owl_core::webhook::InboundEvent {
            id: Uuid::new_v4(),
            endpoint: endpoint.id,
            sender_event_id: None,
            sender_timestamp: None,
            received_at: chrono::Utc::now(),
            dedup_key: graph_owl_core::webhook::dedup_key(None, raw_body),
            raw: raw_body.to_vec(),
            state: if is_valid_json {
                graph_owl_core::webhook::EventState::Received
            } else {
                graph_owl_core::webhook::EventState::Failed
            },
            reason: if is_valid_json {
                None
            } else {
                Some("payload is not valid JSON".to_string())
            },
        };
        Ok(self.storage.create_inbound_event(event).await?)
    }

    /// Records a new version of a mapping — Epic 18 Slice C.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    #[tracing::instrument(name = "catalog.upsert_mapping", skip_all)]
    pub async fn upsert_mapping(
        &self,
        mapping: graph_owl_storage::Mapping,
    ) -> Result<graph_owl_storage::Mapping, CatalogError> {
        Ok(self.storage.upsert_mapping(mapping).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn mapping(
        &self,
        name: &str,
    ) -> Result<Option<graph_owl_storage::Mapping>, CatalogError> {
        Ok(self.storage.get_mapping(name).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn mapping_versions(
        &self,
        name: &str,
    ) -> Result<Vec<graph_owl_storage::Mapping>, CatalogError> {
        Ok(self.storage.list_mapping_versions(name).await?)
    }

    /// Applies a mapping to a sample payload **without writing anything** —
    /// Epic 18 Slice C's dry-run criterion. Every outcome here is a value,
    /// not a `CatalogError`: `Err` is reserved for infrastructure failure
    /// (the mapping name does not exist, or storage fails), because a
    /// mapping that does not fit this payload is exactly what a dry run
    /// exists to discover.
    ///
    /// **Reuses [`Catalog::validate_draft`] rather than a second
    /// implementation.** That is Epic 16 Slice D's own mechanism for
    /// checking a not-yet-persisted entity against Epic 5's shapes — a
    /// draft is projected to flakes and validated exactly as a stored one
    /// would be, so there is one shape-checking code path, not two that
    /// could drift.
    ///
    /// # Errors
    ///
    /// `NotFound` if no mapping is registered under `mapping_name`.
    /// `Storage` if a read fails.
    #[tracing::instrument(name = "catalog.dry_run_mapping", skip_all)]
    pub async fn dry_run_mapping(
        &self,
        mapping_name: &str,
        payload: &serde_json::Value,
    ) -> Result<MappingOutcome, CatalogError> {
        let mapping = self
            .storage
            .get_mapping(mapping_name)
            .await?
            .ok_or(CatalogError::NotFound)?;

        Ok(
            match self.resolve_and_validate_draft(&mapping, payload).await? {
                MappingResolution::Ready { draft, .. } => MappingOutcome::Draft(draft),
                MappingResolution::MissingField { field } => MappingOutcome::MissingField { field },
                MappingResolution::InvalidKind { kind } => MappingOutcome::InvalidKind { kind },
                MappingResolution::ShapeViolation { reason } => {
                    MappingOutcome::ShapeViolation { reason }
                }
            },
        )
    }

    /// Applies a mapping and checks the result against shapes — the one
    /// place that answers "does this payload fit this mapping", shared by
    /// [`Catalog::dry_run_mapping`] (which stops here) and
    /// [`Catalog::process_inbound_event`] (which goes on to apply what this
    /// resolves). `kind` and `parent_id` are returned alongside the draft
    /// so a caller that proceeds to write does not have to re-derive them.
    async fn resolve_and_validate_draft(
        &self,
        mapping: &graph_owl_storage::Mapping,
        payload: &serde_json::Value,
    ) -> Result<MappingResolution, CatalogError> {
        let draft = match graph_owl_connectors::webhook_mapping::apply_mapping(mapping, payload) {
            Ok(draft) => draft,
            Err(graph_owl_connectors::webhook_mapping::MappingError { field, .. }) => {
                return Ok(MappingResolution::MissingField { field });
            }
        };

        let Ok(kind) = AssetKind::parse(&draft.kind) else {
            return Ok(MappingResolution::InvalidKind { kind: draft.kind });
        };

        let parent_id = match &draft.parent_fqn {
            None => None,
            Some(parent_fqn) => self
                .storage
                .get_asset_by_fqn(parent_fqn)
                .await?
                .map(|parent| parent.id),
        };
        let fully_qualified_name = match &draft.parent_fqn {
            Some(parent) => format!("{parent}.{}", draft.name),
            None => draft.name.clone(),
        };
        let now = chrono::Utc::now();
        let candidate = Asset {
            id: Uuid::nil(),
            kind,
            name: draft.name.clone(),
            fully_qualified_name: fully_qualified_name.clone(),
            parent_id,
            description: draft.description.clone(),
            properties: draft.properties.clone(),
            owners: Vec::new(),
            version: graph_owl_core::envelope::EntityVersion::initial(),
            // Attributed to `system`, the same principal every other
            // machine-originated write uses — this candidate is never
            // persisted, so the value only matters if a shape happens to
            // constrain it.
            updated_by: "system".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        if let Some(reason) = self.validate_draft(&candidate).await? {
            return Ok(MappingResolution::ShapeViolation { reason });
        }

        Ok(MappingResolution::Ready {
            draft,
            kind,
            parent_id,
            fully_qualified_name,
        })
    }

    /// Maps and applies one inbound event — Epic 18 Slice D's processing
    /// pipeline. Looks up the event's endpoint and its mapping, parses the
    /// raw payload, resolves and shape-checks the draft (the same check
    /// [`Catalog::dry_run_mapping`] runs), and upserts it — moving the
    /// event through `Mapped` to `Applied`, or to `Failed` with a reason at
    /// whichever step rejected it.
    ///
    /// **Idempotent by state.** Only `Received` and `Failed` events are
    /// (re)processed; every other state is left alone — this is what makes
    /// replaying a window (which may include already-`Applied` events)
    /// safe: dedup holds because there is nothing left for it to do.
    ///
    /// # Errors
    ///
    /// `NotFound` if no such event exists. `Storage` if a write fails. A
    /// mapping, JSON, or shape problem is **not** an error here — it is
    /// recorded as `Failed` and this returns `Ok(())`, because a rejected
    /// draft is this pipeline's normal, expected output for a bad payload.
    #[tracing::instrument(name = "catalog.process_inbound_event", skip_all)]
    pub async fn process_inbound_event(&self, id: Uuid) -> Result<(), CatalogError> {
        let event = self
            .storage
            .get_inbound_event(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        if !matches!(
            event.state,
            graph_owl_core::webhook::EventState::Received
                | graph_owl_core::webhook::EventState::Failed
        ) {
            return Ok(());
        }

        let Some(endpoint) = self.storage.get_webhook_endpoint(event.endpoint).await? else {
            return self
                .fail_inbound_event(
                    id,
                    "the endpoint this event was received on no longer exists".to_string(),
                )
                .await;
        };

        let payload: serde_json::Value = match serde_json::from_slice(&event.raw) {
            Ok(value) => value,
            Err(error) => {
                return self
                    .fail_inbound_event(id, format!("payload is not valid JSON: {error}"))
                    .await;
            }
        };

        let Some(mapping) = self.storage.get_mapping(&endpoint.mapping).await? else {
            return self
                .fail_inbound_event(
                    id,
                    format!("no mapping named `{}` is registered", endpoint.mapping),
                )
                .await;
        };

        let (draft, kind, parent_id, fully_qualified_name) =
            match self.resolve_and_validate_draft(&mapping, &payload).await? {
                MappingResolution::Ready {
                    draft,
                    kind,
                    parent_id,
                    fully_qualified_name,
                } => (draft, kind, parent_id, fully_qualified_name),
                MappingResolution::MissingField { field } => {
                    return self
                        .fail_inbound_event(
                            id,
                            format!(
                                "mapping `{}` field `{field}`: nothing at the path",
                                mapping.name
                            ),
                        )
                        .await;
                }
                MappingResolution::InvalidKind { kind } => {
                    return self
                        .fail_inbound_event(id, format!("`{kind}` is not a known asset kind"))
                        .await;
                }
                MappingResolution::ShapeViolation { reason } => {
                    return self.fail_inbound_event(id, reason).await;
                }
            };

        self.storage
            .update_inbound_event_state(id, graph_owl_core::webhook::EventState::Mapped, None)
            .await?;

        // Out-of-order protection: a candidate whose `sender_timestamp` is
        // older than what is already applied for this entity is recognized
        // and deliberately not applied — a late-arriving stale update must
        // never revert a newer one. Nothing to compare against (no prior
        // applied timestamp) always proceeds; no `sender_timestamp` on the
        // candidate falls back to arrival order, same as
        // `Freshness::Ambiguous` documents, and is logged rather than
        // silently assumed safe.
        if let Some(current) = self
            .storage
            .last_applied_timestamp(&fully_qualified_name)
            .await?
        {
            match graph_owl_core::webhook::compare_timestamps(event.sender_timestamp, current) {
                graph_owl_core::webhook::Freshness::Older => {
                    self.storage
                        .update_inbound_event_state(
                            id,
                            graph_owl_core::webhook::EventState::Superseded,
                            Some(&format!(
                                "sender_timestamp is older than the currently applied state for `{fully_qualified_name}`"
                            )),
                        )
                        .await?;
                    return Ok(());
                }
                graph_owl_core::webhook::Freshness::Ambiguous => {
                    tracing::warn!(
                        event = %id,
                        entity = %fully_qualified_name,
                        "applying an event with no sender_timestamp against entity state that has one; falling back to arrival order"
                    );
                }
                graph_owl_core::webhook::Freshness::Newer => {}
            }
        }

        let principal = self
            .resolve_principal(&endpoint.source, &endpoint.source)
            .await?;
        let sender_timestamp = event.sender_timestamp;
        let request = UpsertAsset {
            kind,
            name: draft.name,
            parent_id,
            description: draft.description,
            properties: draft.properties,
        };
        // `Validation`/`Conflict` are the write refusing *this draft* —
        // exactly what "mapping or validation failure moves the event to
        // DLQ" means, so they dead-letter rather than propagate. Anything
        // else (a storage failure) is not about this event and must not be
        // swallowed as though it were.
        match self.upsert_asset(&principal, request).await {
            Ok(_) => {
                if let Some(sender_timestamp) = sender_timestamp {
                    self.storage
                        .record_applied_timestamp(&fully_qualified_name, sender_timestamp)
                        .await?;
                }
                self.storage
                    .update_inbound_event_state(
                        id,
                        graph_owl_core::webhook::EventState::Applied,
                        None,
                    )
                    .await?;
                Ok(())
            }
            Err(CatalogError::Validation(errors)) => {
                let detail = errors
                    .iter()
                    .map(|e| format!("{}: {}", e.field, e.detail))
                    .collect::<Vec<_>>()
                    .join("; ");
                self.fail_inbound_event(id, detail).await
            }
            Err(CatalogError::Conflict { detail, .. }) => self.fail_inbound_event(id, detail).await,
            Err(other) => Err(other),
        }
    }

    async fn fail_inbound_event(&self, id: Uuid, reason: String) -> Result<(), CatalogError> {
        self.storage
            .update_inbound_event_state(
                id,
                graph_owl_core::webhook::EventState::Failed,
                Some(&reason),
            )
            .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn inbound_event(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::webhook::InboundEvent>, CatalogError> {
        Ok(self.storage.get_inbound_event(id).await?)
    }

    /// Records a poisoned streamed message — Epic 19 Slice D.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn record_stream_dead_letter(
        &self,
        letter: graph_owl_storage::StreamDeadLetter,
    ) -> Result<graph_owl_storage::StreamDeadLetter, CatalogError> {
        Ok(self.storage.create_stream_dead_letter(letter).await?)
    }

    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn stream_dead_letters(
        &self,
        subscription: Option<Uuid>,
    ) -> Result<Vec<graph_owl_storage::StreamDeadLetter>, CatalogError> {
        Ok(self.storage.list_stream_dead_letters(subscription).await?)
    }

    /// Replays one dead letter through the same mapping-and-apply path a
    /// live message takes — Epic 19 Slice D's "the DLQ is replayable after
    /// a mapping fix". On success the letter is removed; on failure it
    /// stays, with the error propagated so the caller sees exactly what a
    /// fixed-then-still-broken mapping produced.
    ///
    /// # Errors
    ///
    /// `NotFound` if no such letter, or its subscription no longer exists.
    /// `Validation` if the payload still does not apply. `Storage` on
    /// read/write failure.
    #[tracing::instrument(name = "catalog.replay_stream_dead_letter", skip_all)]
    pub async fn replay_stream_dead_letter(&self, id: Uuid) -> Result<(), CatalogError> {
        let letter = self
            .storage
            .get_stream_dead_letter(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let subscription = self
            .storage
            .get_stream_subscription(letter.subscription_id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let payload: serde_json::Value =
            serde_json::from_slice(&letter.payload).map_err(|error| {
                CatalogError::Validation(vec![FieldError::new(
                    "payload",
                    FieldErrorCode::Type,
                    format!("the dead-lettered payload is not valid JSON: {error}"),
                )])
            })?;
        self.apply_streamed_message(&subscription.mapping, &payload)
            .await?;
        self.storage.delete_stream_dead_letter(id).await?;
        Ok(())
    }

    /// Maps and applies one streamed message — Epic 19 Slice A. Reuses
    /// Epic 18's mapping and resolution machinery wholesale
    /// (`resolve_and_validate_draft`), the same helper
    /// [`Catalog::process_inbound_event`] calls — the payload shapes and
    /// duplication problem are identical to a webhook's, only the transport
    /// differs.
    ///
    /// **Unlike `process_inbound_event`, there is no persisted event row to
    /// move through states here** — a streamed message has no `InboundEvent`
    /// equivalent yet; Slice D designs streaming's own poison-message
    /// handling separately, since Kafka's redelivery semantics (a consumer
    /// simply re-reads an uncommitted offset) are not a webhook's. A
    /// rejection is therefore a `CatalogError::Validation` here, for the
    /// caller (the consume loop) to decide what to do with, rather than a
    /// silent `Failed` state written somewhere.
    ///
    /// **`Catalog::resolve_asset` runs automatically after every successful
    /// upsert** — `19-streaming.md` decision 7: streaming has no caller
    /// waiting on a response the way a webhook's sender or a batch push's
    /// client does, so nothing else is in a position to ask for it, and
    /// leaving it manual would let streamed data duplicate silently.
    ///
    /// # Errors
    ///
    /// `NotFound` if no mapping is registered under `mapping_name`.
    /// `Validation` if the payload does not resolve to a valid draft (a
    /// missing field, an unknown kind, or a shape violation) or the write
    /// itself is refused. `Storage` if a read or write fails.
    #[tracing::instrument(name = "catalog.apply_streamed_message", skip_all)]
    pub async fn apply_streamed_message(
        &self,
        mapping_name: &str,
        payload: &serde_json::Value,
    ) -> Result<(), CatalogError> {
        let mapping = self
            .storage
            .get_mapping(mapping_name)
            .await?
            .ok_or(CatalogError::NotFound)?;

        let (draft, kind, parent_id) =
            match self.resolve_and_validate_draft(&mapping, payload).await? {
                MappingResolution::Ready {
                    draft,
                    kind,
                    parent_id,
                    ..
                } => (draft, kind, parent_id),
                MappingResolution::MissingField { field } => {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        field,
                        FieldErrorCode::Required,
                        format!("mapping `{mapping_name}` field `{field}`: nothing at the path"),
                    )]));
                }
                MappingResolution::InvalidKind { kind } => {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "kind",
                        FieldErrorCode::Type,
                        format!("`{kind}` is not a known asset kind"),
                    )]));
                }
                MappingResolution::ShapeViolation { reason } => {
                    return Err(CatalogError::Validation(vec![FieldError::new(
                        "shape",
                        FieldErrorCode::Type,
                        reason,
                    )]));
                }
            };

        let principal = Principal::system();
        let request = UpsertAsset {
            kind,
            name: draft.name,
            parent_id,
            description: draft.description,
            properties: draft.properties,
        };
        let asset = self.upsert_asset(&principal, request).await?;
        self.resolve_asset(&principal, asset.id).await?;
        Ok(())
    }

    /// The dead-letter queue, filtered — Epic 18 Slice D.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn dead_letter_queue(
        &self,
        filter: &graph_owl_storage::DeadLetterFilter,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, CatalogError> {
        Ok(self.storage.list_dead_letters(filter).await?)
    }

    /// Replays every event for `endpoint` received between `since` and
    /// `until`, in `sender_timestamp` order (falling back to arrival order
    /// for events without one) — Epic 18 Slice D.
    ///
    /// An already-`Applied` or `Duplicate` event in the window is skipped,
    /// not reprocessed: [`Catalog::process_inbound_event`]'s own
    /// state-gating is what makes "replay of an already-applied event is a
    /// no-op" true, so replay does not need a second idempotency check of
    /// its own — it would only be restating the same rule in a second
    /// place, which is exactly how the two drift.
    ///
    /// # Errors
    ///
    /// `Storage` if a read or write fails.
    #[tracing::instrument(name = "catalog.replay_window", skip_all)]
    pub async fn replay_window(
        &self,
        endpoint: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<ReplaySummary, CatalogError> {
        let events = self
            .storage
            .list_inbound_events_in_window(endpoint, since, until)
            .await?;

        let mut summary = ReplaySummary::default();
        for event in events {
            if matches!(
                event.state,
                graph_owl_core::webhook::EventState::Applied
                    | graph_owl_core::webhook::EventState::Duplicate
                    | graph_owl_core::webhook::EventState::Superseded
            ) {
                summary.skipped += 1;
                continue;
            }
            summary.attempted += 1;
            self.process_inbound_event(event.id).await?;
            match self
                .storage
                .get_inbound_event(event.id)
                .await?
                .map(|e| e.state)
            {
                Some(graph_owl_core::webhook::EventState::Applied) => summary.applied += 1,
                _ => summary.still_failed += 1,
            }
        }
        Ok(summary)
    }

    /// Deletes dead-lettered events older than `older_than` — Slice D's
    /// bounded-retention criterion. The bound is the caller's to configure;
    /// this is the mechanism, not a schedule this crate decides on its own.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn purge_dead_letters(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, CatalogError> {
        Ok(self.storage.purge_dead_letters(older_than).await?)
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
        // **Cycle detection, at any depth** — Epic 11 Slice B. Checked here rather
        // than only in the schema because the database can cheaply refuse
        // self-parenting and nothing more: `A → B → C → A` needs an ancestor walk,
        // and the walk has to happen before the write or the graph it walks is
        // already broken.
        if let Some(parent) = &team.parent_team_id {
            if self.storage.find_team(parent).await?.is_none() {
                problems.push(FieldError::new(
                    "parentTeamId",
                    FieldErrorCode::Type,
                    format!("`{parent}` is not a known team"),
                ));
            } else if self.storage.would_cycle(&team.id, parent).await? {
                problems.push(FieldError::new(
                    "parentTeamId",
                    FieldErrorCode::Type,
                    format!(
                        "making `{parent}` the parent of `{}` would close a cycle in the team hierarchy",
                        team.id
                    ),
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

    /// Teams reporting directly into this one.
    ///
    /// # Errors
    ///
    /// `NotFound` if the team does not exist. `Storage` if the read fails.
    pub async fn child_teams(
        &self,
        id: &str,
    ) -> Result<Vec<graph_owl_storage::Team>, CatalogError> {
        if self.storage.find_team(id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.child_teams(id).await?)
    }

    /// Record that a user follows an asset — Epic 11 Slice F.
    ///
    /// **Idempotent**: following what you already follow is the state you asked
    /// for, not a conflict.
    ///
    /// # Errors
    ///
    /// `NotFound` if the asset does not exist. `Validation` if the asset is
    /// soft-deleted — following a tombstone records interest in something nobody
    /// can read.
    pub async fn follow_asset(
        &self,
        asset_id: Uuid,
        user_id: &str,
    ) -> Result<graph_owl_storage::FollowOutcome, CatalogError> {
        let Some(asset) = self.storage.get_asset(asset_id).await? else {
            return Err(CatalogError::NotFound);
        };
        if asset.deleted {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "assetId",
                FieldErrorCode::Type,
                "this asset is deleted; following it would record interest in \
                 something nobody can read"
                    .to_string(),
            )]));
        }
        Ok(self.storage.follow_asset(asset_id, user_id).await?)
    }

    /// Stop following. Also idempotent.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn unfollow_asset(&self, asset_id: Uuid, user_id: &str) -> Result<(), CatalogError> {
        Ok(self.storage.unfollow_asset(asset_id, user_id).await?)
    }

    /// What this user follows.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn assets_followed_by(
        &self,
        user_id: &str,
        page: &PageRequest,
    ) -> Result<Page<Asset>, CatalogError> {
        Ok(self.storage.assets_followed_by(user_id, page).await?)
    }

    /// How many people follow this asset.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn follower_count(&self, asset_id: Uuid) -> Result<i64, CatalogError> {
        Ok(self.storage.follower_count(asset_id).await?)
    }

    /// Delete a principal, refusing to strand what it holds — Epic 11 Slice G.
    ///
    /// # Errors
    ///
    /// `NotFound` if the principal does not exist. `Conflict` carrying the counts
    /// when it still owns assets or parents teams and no `reassign_to` was given —
    /// "ownership stays truthful when somebody leaves" is the slice's value, and a
    /// silent cascade to unowned is exactly what that forbids. `Validation` if the
    /// reassignment target does not exist.
    pub async fn delete_principal(
        &self,
        principal: &graph_owl_core::ownership::OwnerRef,
        reassign_to: Option<&graph_owl_core::ownership::OwnerRef>,
    ) -> Result<i64, CatalogError> {
        match self
            .storage
            .delete_principal(principal, reassign_to)
            .await?
        {
            graph_owl_storage::PrincipalDeletion::Deleted { reassigned } => Ok(reassigned),
            graph_owl_storage::PrincipalDeletion::NotFound => Err(CatalogError::NotFound),
            graph_owl_storage::PrincipalDeletion::UnknownTarget => {
                Err(CatalogError::Validation(vec![FieldError::new(
                    "reassignTo",
                    FieldErrorCode::Type,
                    "the principal to reassign ownership to does not exist".to_string(),
                )]))
            }
            graph_owl_storage::PrincipalDeletion::StillHolds(holdings) => {
                // The detail names counts *by kind*, because "you still own 400
                // things" is not actionable while "1 service, 3 schemas, 396
                // columns" says reassign the service and let inheritance do the
                // rest.
                let owned = holdings
                    .owned_by_kind
                    .iter()
                    .map(|(kind, n)| format!("{n} {}", kind.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut detail = format!(
                    "`{}` still owns {} asset(s): {owned}",
                    principal.id,
                    holdings.owned_total()
                );
                if !holdings.child_teams.is_empty() {
                    detail.push_str(&format!(
                        "; and parents {} team(s): {}",
                        holdings.child_teams.len(),
                        holdings.child_teams.join(", ")
                    ));
                }
                detail.push_str(". Pass reassignTo to transfer, or remove the ownership first.");
                Err(CatalogError::Conflict {
                    detail,
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::PrincipalStillHolds,
                })
            }
        }
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

    // ---- Epic 24 Slice A: glossary and terms ----

    /// Create a glossary.
    ///
    /// # Errors
    ///
    /// `Validation` if the name is blank. `Conflict` if the derived FQN
    /// collides with an existing one.
    #[tracing::instrument(name = "catalog.create_glossary", skip_all)]
    pub async fn create_glossary(
        &self,
        name: &str,
        description: Option<String>,
    ) -> Result<graph_owl_storage::Glossary, CatalogError> {
        if name.trim().is_empty() {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Empty,
                "a glossary needs a name".to_string(),
            )]));
        }
        let fully_qualified_name = graph_owl_core::fqn::derive(&[name]).map_err(|e| {
            CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Type,
                e.to_string(),
            )])
        })?;
        let now = Utc::now();
        let glossary = graph_owl_storage::Glossary {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description,
            fully_qualified_name,
            created_at: now,
            updated_at: now,
        };
        Ok(self.storage.insert_glossary(glossary).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_glossary(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::Glossary>, CatalogError> {
        Ok(self.storage.get_glossary(id).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_glossaries(&self) -> Result<Vec<graph_owl_storage::Glossary>, CatalogError> {
        Ok(self.storage.list_glossaries().await?)
    }

    /// Delete a glossary. `recursive` deletes its terms first rather than
    /// refusing.
    ///
    /// # Errors
    ///
    /// `NotFound` if it does not exist. `Conflict` (`kind:
    /// GlossaryHasTerms`) if it still has terms and `recursive` was not
    /// asked for.
    #[tracing::instrument(name = "catalog.delete_glossary", skip_all)]
    pub async fn delete_glossary(&self, id: Uuid, recursive: bool) -> Result<(), CatalogError> {
        match self.storage.delete_glossary(id, recursive).await? {
            graph_owl_storage::GlossaryDeletion::Deleted => Ok(()),
            graph_owl_storage::GlossaryDeletion::NotFound => Err(CatalogError::NotFound),
            graph_owl_storage::GlossaryDeletion::HasTerms { term_count } => {
                Err(CatalogError::Conflict {
                    detail: format!(
                        "this glossary still has {term_count} term(s); pass \
                         `recursive=true` to delete them with it"
                    ),
                    existing_id: Some(id),
                    kind: ConflictKind::GlossaryHasTerms,
                })
            }
        }
    }

    /// Create a term under a glossary.
    ///
    /// # Errors
    ///
    /// `NotFound` if the glossary does not exist. `Validation` if the name
    /// is blank. `Conflict` if the name is already used within this
    /// glossary — checked by FQN, so a term with the same name in a
    /// *different* glossary succeeds (decision 1).
    #[tracing::instrument(name = "catalog.create_term", skip_all)]
    pub async fn create_term(
        &self,
        glossary_id: Uuid,
        name: &str,
        definition: String,
        synonyms: Vec<String>,
        abbreviations: Vec<String>,
    ) -> Result<graph_owl_storage::GlossaryTermRecord, CatalogError> {
        let Some(glossary) = self.storage.get_glossary(glossary_id).await? else {
            return Err(CatalogError::NotFound);
        };
        if name.trim().is_empty() {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Empty,
                "a term needs a name".to_string(),
            )]));
        }
        let fully_qualified_name =
            graph_owl_core::fqn::child_of(&glossary.fully_qualified_name, name).map_err(|e| {
                CatalogError::Validation(vec![FieldError::new(
                    "name",
                    FieldErrorCode::Type,
                    e.to_string(),
                )])
            })?;
        let now = Utc::now();
        let term = graph_owl_storage::GlossaryTermRecord {
            id: Uuid::new_v4(),
            glossary_id,
            name: name.to_string(),
            fully_qualified_name,
            definition,
            // Every term is born `Draft`; only Slice C's workflow moves it.
            status: graph_owl_core::glossary::TermStatus::Draft,
            synonyms,
            abbreviations,
            // The migration's own default (`1.0`) — a workflow move, not a
            // field edit, so it does not share Epic 3's asset envelope's
            // `0.1` starting point.
            version: EntityVersion { major: 1, minor: 0 },
            created_at: now,
            updated_at: now,
        };
        Ok(self.storage.insert_term(term).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_term(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, CatalogError> {
        Ok(self.storage.get_term(id).await?)
    }

    /// Every term in one glossary.
    ///
    /// # Errors
    ///
    /// `NotFound` if the glossary does not exist.
    pub async fn list_terms(
        &self,
        glossary_id: Uuid,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, CatalogError> {
        if self.storage.get_glossary(glossary_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.list_terms(glossary_id).await?)
    }

    /// # Errors
    ///
    /// `NotFound` if the term does not exist.
    pub async fn update_term(
        &self,
        id: Uuid,
        update: graph_owl_storage::GlossaryTermUpdate,
    ) -> Result<graph_owl_storage::GlossaryTermRecord, CatalogError> {
        self.storage
            .update_term(id, update)
            .await?
            .ok_or(CatalogError::NotFound)
    }

    /// # Errors
    ///
    /// `NotFound` if the term does not exist.
    pub async fn delete_term(&self, id: Uuid) -> Result<(), CatalogError> {
        if self.storage.delete_term(id).await? {
            Ok(())
        } else {
            Err(CatalogError::NotFound)
        }
    }

    /// Search terms by name, synonym, abbreviation, or definition.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn search_terms(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, CatalogError> {
        Ok(self.storage.search_terms(query).await?)
    }

    // ---- Epic 24 Slice B: SKOS relations ----

    /// Assert a SKOS relation, owned by `term_id`.
    ///
    /// # Errors
    ///
    /// `NotFound` if `term_id` does not exist. `Validation` if: the relation
    /// is `narrower` (derived only, never stored directly — decision:
    /// storing both directions gives two rows that can disagree); an
    /// internal relation's target is not a known term; or a `broader`
    /// assertion would close a cycle at any depth.
    #[tracing::instrument(name = "catalog.add_term_relation", skip_all)]
    pub async fn add_term_relation(
        &self,
        term_id: Uuid,
        relation: graph_owl_core::glossary::SkosRelation,
    ) -> Result<(), CatalogError> {
        use graph_owl_core::glossary::SkosRelation;

        if self.storage.get_term(term_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }

        if matches!(relation, SkosRelation::Narrower(_)) {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "kind",
                FieldErrorCode::Type,
                "`narrower` is derived from the other term's `broader` and cannot be \
                 asserted directly; assert `broader` from that term instead"
                    .to_string(),
            )]));
        }

        // Internal relations point at another term in this catalog; a match
        // relation points at an external IRI and is deliberately **not**
        // checked for reachability (decision 2's whole point — a vocabulary
        // that must be online for a term to be valid is a vocabulary whose
        // terms fail when somebody else's server does).
        if relation.is_internal() {
            let target_id = Uuid::parse_str(relation.target()).map_err(|_| {
                CatalogError::Validation(vec![FieldError::new(
                    "target",
                    FieldErrorCode::Type,
                    format!("`{}` is not a valid term id", relation.target()),
                )])
            })?;
            if self.storage.get_term(target_id).await?.is_none() {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "target",
                    FieldErrorCode::Type,
                    format!("`{}` is not a known term", relation.target()),
                )]));
            }
        }

        // **Cycle detection, at any depth** — Slice B, reusing Epic 11's
        // detector. `would_cycle` is a pure walk over every stored `broader`
        // edge; poly-hierarchy is legitimate SKOS, so it follows every
        // parent rather than assuming one.
        if let SkosRelation::Broader(target) = &relation {
            let edges = self.storage.broader_edges().await?;
            if graph_owl_core::glossary::would_cycle(&term_id.to_string(), target, &edges) {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "target",
                    FieldErrorCode::Type,
                    format!(
                        "making `{target}` broader than `{term_id}` would close a cycle \
                         in the term hierarchy"
                    ),
                )]));
            }
        }

        self.storage.insert_term_relation(term_id, relation).await?;
        Ok(())
    }

    /// Every relation visible on a term — what it declared, and what points
    /// at it, derived inverses included — without a second stored edge for
    /// the derived half.
    ///
    /// # Errors
    ///
    /// `NotFound` if `term_id` does not exist.
    pub async fn term_relations(
        &self,
        term_id: Uuid,
    ) -> Result<Vec<graph_owl_core::glossary::SkosRelation>, CatalogError> {
        if self.storage.get_term(term_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        let stored = self.storage.term_relations_touching(term_id).await?;
        Ok(graph_owl_core::glossary::visible_relations(
            &term_id.to_string(),
            &stored,
        ))
    }

    /// Retract a relation `term_id` owns.
    ///
    /// # Errors
    ///
    /// `NotFound` if no such stored row exists — including when `relation`
    /// is only visible on `term_id` as a derived inverse, which was never a
    /// row of its own to delete.
    pub async fn remove_term_relation(
        &self,
        term_id: Uuid,
        relation: &graph_owl_core::glossary::SkosRelation,
    ) -> Result<(), CatalogError> {
        if self.storage.delete_term_relation(term_id, relation).await? {
            Ok(())
        } else {
            Err(CatalogError::NotFound)
        }
    }

    // ---- Epic 24 Slice C: review workflow ----

    /// Replace a term's assigned reviewers.
    ///
    /// # Errors
    ///
    /// `NotFound` if the term does not exist. `Validation` if a named
    /// reviewer is not a known user.
    pub async fn set_term_reviewers(
        &self,
        term_id: Uuid,
        reviewers: Vec<String>,
    ) -> Result<(), CatalogError> {
        if self.storage.get_term(term_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        for reviewer in &reviewers {
            if self.storage.find_user(reviewer).await?.is_none() {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "reviewers",
                    FieldErrorCode::Type,
                    format!("`{reviewer}` is not a known user"),
                )]));
            }
        }
        self.storage.set_term_reviewers(term_id, &reviewers).await?;
        Ok(())
    }

    /// # Errors
    ///
    /// `NotFound` if the term does not exist.
    pub async fn term_reviewers(&self, term_id: Uuid) -> Result<Vec<String>, CatalogError> {
        if self.storage.get_term(term_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.term_reviewers(term_id).await?)
    }

    /// Move a term to `to`.
    ///
    /// # Errors
    ///
    /// `NotFound` if the term does not exist. `Validation` if the move is
    /// not in the transition matrix, naming both ends; or if approval is
    /// attempted with no reviewer assigned; or if `successor_term_id` is
    /// given and is not a known term. `Forbidden` if `actor` is not an
    /// assigned reviewer and the move is an approval — the request would be
    /// accepted from a different, permitted caller.
    #[tracing::instrument(name = "catalog.transition_term", skip_all)]
    pub async fn transition_term(
        &self,
        term_id: Uuid,
        to: graph_owl_core::glossary::TermStatus,
        actor: &str,
        reason: Option<String>,
        successor_term_id: Option<Uuid>,
    ) -> Result<graph_owl_storage::GlossaryTermRecord, CatalogError> {
        use graph_owl_core::glossary::TransitionError;

        let Some(term) = self.storage.get_term(term_id).await? else {
            return Err(CatalogError::NotFound);
        };

        let reviewers = self.storage.term_reviewers(term_id).await?;
        if let Err(error) = graph_owl_core::glossary::transition(term.status, to, actor, &reviewers)
        {
            return Err(match &error {
                // The request would be accepted from a different, permitted
                // caller — a `Forbidden`, not a `Validation` a retry with a
                // different value could fix.
                TransitionError::NotAReviewer => CatalogError::Forbidden,
                TransitionError::NotPermitted { .. } => {
                    CatalogError::Validation(vec![FieldError::new(
                        "status",
                        FieldErrorCode::Type,
                        error.to_string(),
                    )])
                }
                TransitionError::NoReviewer => CatalogError::Validation(vec![FieldError::new(
                    "reviewers",
                    FieldErrorCode::Required,
                    error.to_string(),
                )]),
            });
        }

        if let Some(successor) = successor_term_id {
            if self.storage.get_term(successor).await?.is_none() {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "successorTermId",
                    FieldErrorCode::Type,
                    format!("`{successor}` is not a known term"),
                )]));
            }
        }

        self.storage
            .transition_term(term_id, term.status, to, actor, reason, successor_term_id)
            .await?
            .ok_or(CatalogError::NotFound)
    }

    // ---- Epic 24 Slice D: terms attach to assets and columns ----

    /// Attach a term to an asset or column, addressed by FQN.
    ///
    /// # Errors
    ///
    /// `NotFound` if the term does not exist. `Validation` naming the
    /// term's status if it is not `Approved` — a draft definition attached
    /// to a thousand columns becomes the de facto definition regardless of
    /// what its status says (decision 4).
    pub async fn attach_term(
        &self,
        term_id: Uuid,
        target_fqn: &str,
        attached_by: &str,
    ) -> Result<(), CatalogError> {
        let Some(term) = self.storage.get_term(term_id).await? else {
            return Err(CatalogError::NotFound);
        };
        if !graph_owl_core::glossary::is_attachable(term.status) {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "status",
                FieldErrorCode::Type,
                format!(
                    "only an approved term may be attached; this term is {}",
                    term.status.as_str()
                ),
            )]));
        }
        self.storage
            .attach_term(term_id, target_fqn, attached_by)
            .await?;
        Ok(())
    }

    /// Detach a term from an asset or column.
    ///
    /// # Errors
    ///
    /// `NotFound` if no such attachment exists.
    pub async fn detach_term(&self, term_id: Uuid, target_fqn: &str) -> Result<(), CatalogError> {
        if self.storage.detach_term(term_id, target_fqn).await? {
            Ok(())
        } else {
            Err(CatalogError::NotFound)
        }
    }

    /// Every asset or column a term is attached to, paginated.
    ///
    /// # Errors
    ///
    /// `NotFound` if the term does not exist.
    pub async fn term_usage(
        &self,
        term_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<String>, CatalogError> {
        if self.storage.get_term(term_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        Ok(self.storage.term_usage(term_id, page).await?)
    }

    // ---- Epic 24 Slice E: Metric as a first-class entity ----

    /// Create a metric.
    ///
    /// # Errors
    ///
    /// `Validation` if the name or definition is blank; if `defined_by`
    /// does not reference a known, `Approved` term; or if a named source is
    /// not a known asset. `Conflict` if the derived FQN collides.
    ///
    /// A metric with **no** `source_assets` is permitted — it is the
    /// commonest metric there is, and the gap is reported on the response
    /// rather than refused (`graph_owl_core::metric::gaps`).
    #[tracing::instrument(name = "catalog.create_metric", skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub async fn create_metric(
        &self,
        name: &str,
        definition: String,
        formula: Option<String>,
        unit: Option<String>,
        granularity: Option<String>,
        calculation_type: graph_owl_core::metric::CalculationType,
        source_assets: Vec<String>,
        defined_by: Option<Uuid>,
    ) -> Result<graph_owl_storage::MetricRecord, CatalogError> {
        if name.trim().is_empty() {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Empty,
                "a metric needs a name".to_string(),
            )]));
        }
        if definition.trim().is_empty() {
            return Err(CatalogError::Validation(vec![FieldError::new(
                "definition",
                FieldErrorCode::Empty,
                "a metric whose definition is blank is a name, and a name is what the \
                 ambiguity was in the first place"
                    .to_string(),
            )]));
        }
        // Namespaced away from tables (decision — Slice E): `metric.revenue`
        // and a table called `revenue` are different things, and a shared
        // FQN space would make one of them unaddressable.
        let fully_qualified_name = graph_owl_core::fqn::derive(&["metric", name]).map_err(|e| {
            CatalogError::Validation(vec![FieldError::new(
                "name",
                FieldErrorCode::Type,
                e.to_string(),
            )])
        })?;

        if let Some(term_id) = defined_by {
            let Some(term) = self.storage.get_term(term_id).await? else {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "definedBy",
                    FieldErrorCode::Type,
                    format!("`{term_id}` is not a known term"),
                )]));
            };
            if !graph_owl_core::glossary::is_attachable(term.status) {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "definedBy",
                    FieldErrorCode::Type,
                    format!(
                        "a metric may only be defined by an approved term; `{term_id}` is {}",
                        term.status.as_str()
                    ),
                )]));
            }
        }

        for source in &source_assets {
            if self.storage.get_asset_by_fqn(source).await?.is_none() {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "sourceAssets",
                    FieldErrorCode::Type,
                    format!("`{source}` is not a known asset"),
                )]));
            }
        }

        let now = Utc::now();
        let metric = graph_owl_storage::MetricRecord {
            id: Uuid::new_v4(),
            name: name.to_string(),
            fully_qualified_name,
            definition,
            formula,
            unit,
            granularity,
            calculation_type,
            defined_by,
            source_assets,
            created_at: now,
            updated_at: now,
        };
        Ok(self.storage.insert_metric(metric).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_metric(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::MetricRecord>, CatalogError> {
        Ok(self.storage.get_metric(id).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_metrics(
        &self,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_storage::MetricRecord>, CatalogError> {
        Ok(self.storage.list_metrics(page).await?)
    }

    /// # Errors
    ///
    /// `NotFound` if the metric does not exist.
    pub async fn update_metric(
        &self,
        id: Uuid,
        update: graph_owl_storage::MetricUpdate,
    ) -> Result<graph_owl_storage::MetricRecord, CatalogError> {
        self.storage
            .update_metric(id, update)
            .await?
            .ok_or(CatalogError::NotFound)
    }

    /// # Errors
    ///
    /// `NotFound` if the metric does not exist.
    pub async fn delete_metric(&self, id: Uuid) -> Result<(), CatalogError> {
        if self.storage.delete_metric(id).await? {
            Ok(())
        } else {
            Err(CatalogError::NotFound)
        }
    }

    /// Search metrics by name, definition, or defining term.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn search_metrics(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::MetricRecord>, CatalogError> {
        Ok(self.storage.search_metrics(query).await?)
    }

    // ---- Epic 24 Slice F: metric lineage reconciliation ----

    /// Replace what a metric declares as its sources.
    ///
    /// Runs the declared list through
    /// [`graph_owl_core::metric::reconcile_lineage`] before storing —
    /// deduplicated, and a metric naming itself excluded, the same rule the
    /// core function applies everywhere it is used. **Scoped to
    /// `metric_sources`**, not `lineage_edges` (see
    /// [`graph_owl_storage::Storage::update_metric_sources`]'s doc): a
    /// metric is not an asset, so this is not yet reachable by Epic 29
    /// traversal.
    ///
    /// # Errors
    ///
    /// `NotFound` if the metric does not exist. `Validation` if a named
    /// source is not a known asset.
    #[tracing::instrument(name = "catalog.set_metric_sources", skip_all)]
    pub async fn set_metric_sources(
        &self,
        metric_id: Uuid,
        sources: Vec<String>,
    ) -> Result<graph_owl_storage::MetricRecord, CatalogError> {
        if self.storage.get_metric(metric_id).await?.is_none() {
            return Err(CatalogError::NotFound);
        }
        for source in &sources {
            if self.storage.get_asset_by_fqn(source).await?.is_none() {
                return Err(CatalogError::Validation(vec![FieldError::new(
                    "sourceAssets",
                    FieldErrorCode::Type,
                    format!("`{source}` is not a known asset"),
                )]));
            }
        }

        let metric_id_str = metric_id.to_string();
        let plan = graph_owl_core::metric::reconcile_lineage(&metric_id_str, &sources, &[]);
        let mut resolved: Vec<String> = plan.to_add.into_iter().map(|edge| edge.from).collect();
        resolved.sort();

        self.storage
            .update_metric_sources(metric_id, &resolved)
            .await?
            .ok_or(CatalogError::NotFound)
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
    /// Create or update a user's record — Epic 11 Slice A's missing half.
    ///
    /// **Roles are deliberately not settable here.** `set_user_roles` is the one
    /// path that grants them, and it invalidates the authorization cache; a second
    /// path that also wrote roles would have to remember to do the same, and the
    /// one that forgot would leave stale permissions in force. Creating a user
    /// grants nothing, which is also the right default for onboarding.
    ///
    /// # Errors
    ///
    /// `Validation` if the id or display name is blank — an unnamed principal is
    /// one nobody can pick from a list.
    pub async fn upsert_user_record(
        &self,
        id: &str,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<StoredUser, CatalogError> {
        let mut problems = Vec::new();
        if id.trim().is_empty() {
            problems.push(FieldError::new(
                "id",
                FieldErrorCode::Required,
                "a user needs an id — the identity provider's subject".to_string(),
            ));
        }
        if display_name.trim().is_empty() {
            problems.push(FieldError::new(
                "displayName",
                FieldErrorCode::Required,
                "a user needs a name somebody can recognise in an owner list".to_string(),
            ));
        }
        if !problems.is_empty() {
            return Err(CatalogError::Validation(problems));
        }

        // Existing roles and flags are preserved: this endpoint renames and
        // records, it does not re-provision. Overwriting `is_admin` from a body
        // would make a rename a privilege change.
        let existing = self.storage.find_user(id).await?;
        let user = StoredUser {
            id: id.to_string(),
            display_name: display_name.to_string(),
            email: email.map(ToString::to_string),
            is_admin: existing.as_ref().is_some_and(|u| u.is_admin),
            is_bot: existing.as_ref().is_some_and(|u| u.is_bot),
            roles: existing.map(|u| u.roles).unwrap_or_default(),
        };
        self.storage.upsert_user(&user).await?;
        Ok(user)
    }

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

    // ---- Epic 17: entity resolution ----

    /// The column names of a `Table` asset, for the scorer's structural-
    /// overlap term. Empty for every other kind — a column has no children
    /// of its own, and asking would just be an empty query every time.
    async fn column_names(&self, asset: &Asset) -> Result<Vec<String>, CatalogError> {
        if asset.kind != AssetKind::Table {
            return Ok(Vec::new());
        }
        Ok(self
            .storage
            .list_children(Some(asset.id))
            .await?
            .into_iter()
            .filter(|child| child.kind == AssetKind::Column)
            .map(|child| child.name)
            .collect())
    }

    fn entity_view<'a>(asset: &'a Asset, columns: &'a [String]) -> EntityView<'a> {
        EntityView {
            name: &asset.name,
            parent_fqn: fqn::parent(&asset.fully_qualified_name),
            // Not yet tracked on `Asset` (Epic 17 Slice B's design note) — the
            // term never fires until a source-system field exists to compare.
            source_system: None,
            column_names: columns,
        }
    }

    /// Resolves `asset_id` against its blocking-key candidates: a
    /// deterministic FQN match short-circuits (Slice A); otherwise the
    /// probabilistic scorer (Slice C) picks the best-scoring candidate and
    /// the confidence bands (Slice D) decide what happens to it.
    ///
    /// # Errors
    ///
    /// `NotFound` if `asset_id` does not exist. `Storage` if the decision
    /// would auto-merge and no graph engine is configured — a merge without
    /// a graph to retract/assert against cannot honour Slice D's contract.
    ///
    /// # Panics
    ///
    /// Never in practice: the one `expect` below is reached only when
    /// `candidates` is non-empty, which is checked immediately above it.
    pub async fn resolve_asset(
        &self,
        principal: &Principal,
        asset_id: Uuid,
    ) -> Result<Resolution, CatalogError> {
        let _ = principal;

        let target = self
            .storage
            .get_asset(asset_id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        let all_candidates = self.storage.resolution_candidates(asset_id).await?;

        // A recently-split pair is excluded before *any* decision is made —
        // deterministic or scored — or a case-different FQN would re-merge
        // through the short-circuit on the very next write, ignoring the
        // cooldown entirely (Slice E's "does not immediately re-merge").
        let mut candidates = Vec::with_capacity(all_candidates.len());
        for candidate in all_candidates {
            let cooled_down = self
                .storage
                .most_recent_split_between(target.id, candidate.id)
                .await?
                .is_some();
            if !cooled_down {
                candidates.push(candidate);
            }
        }
        if candidates.is_empty() {
            return Ok(Resolution::New);
        }

        // Deterministic match short-circuits scoring entirely (Slice A's
        // contract: a scorer bug must never affect an exact match).
        for candidate in &candidates {
            if is_deterministic_match(
                &target.fully_qualified_name,
                &candidate.fully_qualified_name,
            ) {
                return self
                    .merge(
                        candidate.id,
                        target.id,
                        1.0,
                        vec![Evidence::NormalizedFqn],
                        MergeDecidedBy::Auto,
                    )
                    .await;
            }
        }

        let weights = ScoreWeights::default();
        let target_columns = self.column_names(&target).await?;
        let target_view = Self::entity_view(&target, &target_columns);

        // Each candidate's own columns are kept alongside it — needed again
        // below for its evidence, and an `EntityView` borrows them, so they
        // cannot be dropped between scoring and reporting.
        let mut scored: Vec<(Asset, Vec<String>, f64)> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let columns = self.column_names(&candidate).await?;
            let candidate_view = Self::entity_view(&candidate, &columns);
            let candidate_score = score(&target_view, &candidate_view, &weights);
            scored.push((candidate, columns, candidate_score));
        }

        let best_index = scored
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.2.total_cmp(&b.2))
            .map(|(index, _)| index)
            .expect("scored is non-empty because candidates was non-empty");
        let (best, best_columns, best_score) = &scored[best_index];
        let bands = ConfidenceBands::default();
        let decision = decide(*best_score, &bands);

        // **`Decision::AutoMerge` is unreachable through this branch given
        // the current wiring, and that is provable rather than a missing
        // test.** `same_source_system` is always `0` here (`entity_view`
        // hard-codes `source_system: None` — `Asset` does not track one
        // yet), so with the weights `(0.5, 0.3, 0.1, 0.1)` the maximum score
        // without an exact (case-insensitive) name match is
        // `0.5·sim + 0.3 + 0.1 = 0.4 + 0.5·sim` — reaching `0.9` needs
        // `sim = 1.0` exactly, and a name that similar under a shared parent
        // is already a normalized-FQN match, which returns via the
        // deterministic short-circuit above and never reaches here. `cargo
        // mutants` reports `==`/`&&` mutations on this line as MISSED, and
        // that is expected for the same reason `column_overlap`'s `&&`/`||`
        // in `graph-owl-resolution` is: no input can currently distinguish
        // the two. Wiring a real `source_system` onto `Asset` would make
        // this branch reachable and give a mutant here real teeth again.
        if decision == Decision::AutoMerge && self.auto_merge_enabled {
            let best_view = Self::entity_view(best, best_columns);
            let merge_evidence = evidence(&target_view, &best_view);
            return self
                .merge(
                    best.id,
                    target.id,
                    *best_score,
                    merge_evidence,
                    MergeDecidedBy::Auto,
                )
                .await;
        }

        if decision == Decision::New {
            return Ok(Resolution::New);
        }

        // `AutoMerge` while disabled, and `Review`, both surface every
        // scored candidate for a human to decide — creating no merge and no
        // new entity, per Slice D's strictest acceptance criterion. Queuing
        // (Slice F) is the one side effect that *is* expected here: it is
        // neither, and `queue_for_review`'s idempotency is what keeps a
        // repeated resolution from re-litigating an already-decided pair.
        let mut reported = Vec::with_capacity(scored.len());
        for (candidate, columns, candidate_score) in &scored {
            let candidate_evidence = evidence(&target_view, &Self::entity_view(candidate, columns));
            self.storage
                .queue_for_review(graph_owl_core::resolution::ReviewQueueEntry {
                    id: Uuid::new_v4(),
                    target: target.id,
                    candidate: candidate.id,
                    score: *candidate_score,
                    evidence: candidate_evidence.clone(),
                    status: graph_owl_core::resolution::ReviewStatus::Pending,
                    decided_by: None,
                    decided_at: None,
                    created_at: chrono::Utc::now(),
                })
                .await?;
            reported.push(Candidate {
                entity: candidate.id,
                fqn: candidate.fully_qualified_name.clone(),
                score: *candidate_score,
                evidence: candidate_evidence,
            });
        }
        Ok(Resolution::Ambiguous {
            candidates: reported,
        })
    }

    /// Writes a merge: retracts `merged`'s flakes, asserts `sameAs` toward
    /// `canonical`, and records the decision. `merged`'s relational row is
    /// untouched — only its graph projection changes — which is what makes
    /// a split (below) a matter of reversing two flake operations rather
    /// than restoring relationships, tags or owners that were never moved.
    async fn merge(
        &self,
        canonical: Uuid,
        merged: Uuid,
        confidence: f64,
        merge_evidence: Vec<Evidence>,
        decided_by: MergeDecidedBy,
    ) -> Result<Resolution, CatalogError> {
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let merged_sid = Sid::new(namespace::DSC, merged.to_string());
        let canonical_sid = Sid::new(namespace::DSC, canonical.to_string());
        let same_as = Sid::new(namespace::OWL, "sameAs");

        let current = graph
            .query_pattern(&TriplePattern {
                s: Some(merged_sid.clone()),
                cx: Some(None),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let t = graph
            .next_time()
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let retractions: Vec<Flake> = current.iter().map(|f| f.retracted_at(t)).collect();
        graph
            .retract_flakes(&retractions)
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        graph
            .assert_flakes(&[Flake::assert(
                merged_sid,
                same_as,
                FlakeValue::Ref(canonical_sid),
                t,
            )])
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let record = self
            .storage
            .create_merge_record(MergeRecord {
                id: Uuid::new_v4(),
                canonical,
                merged,
                evidence: merge_evidence,
                confidence,
                decided_by,
                decided_at: chrono::Utc::now(),
                merged_at_t: t,
                split_at: None,
            })
            .await?;

        Ok(Resolution::Existing {
            entity: record.canonical,
            confidence: record.confidence,
        })
    }

    /// Reverses a merge (Epic 17 Slice E): retracts the `sameAs` and
    /// re-asserts exactly the flakes the merge retracted, read via
    /// time-travel at `merged_at_t - 1` — the instant before the merge's own
    /// write — rather than reconstructed by any other means. That is what
    /// makes the round trip exact rather than approximate.
    ///
    /// # Errors
    ///
    /// `NotFound` if the merge record does not exist. `Conflict` if it was
    /// already split. `Storage` if no graph engine is configured.
    pub async fn split_merge(
        &self,
        principal: &Principal,
        merge_id: Uuid,
    ) -> Result<MergeRecord, CatalogError> {
        let _ = principal;
        let graph = self.graph.as_ref().ok_or_else(|| {
            CatalogError::Storage(StorageError::Unexpected(
                "this server has no graph engine configured".to_string(),
            ))
        })?;

        let existing = self
            .storage
            .get_merge_record(merge_id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        if existing.split_at.is_some() {
            return Err(CatalogError::Conflict {
                detail: "this merge has already been split".to_string(),
                existing_id: Some(merge_id),
                kind: ConflictKind::MergeAlreadySplit,
            });
        }

        let merged_sid = Sid::new(namespace::DSC, existing.merged.to_string());
        let same_as = Sid::new(namespace::OWL, "sameAs");

        // The pre-merge state, exactly: everything true of `merged` the
        // instant before its merge transaction wrote.
        let before_merge = graph
            .query_pattern(&TriplePattern {
                s: Some(merged_sid.clone()),
                cx: Some(None),
                as_of: Some(existing.merged_at_t - 1),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let t = graph
            .next_time()
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let retract_same_as = Flake::assert(
            merged_sid,
            same_as,
            FlakeValue::Ref(Sid::new(namespace::DSC, existing.canonical.to_string())),
            existing.merged_at_t,
        )
        .retracted_at(t);
        graph
            .retract_flakes(&[retract_same_as])
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        let restored: Vec<Flake> = before_merge
            .into_iter()
            .map(|f| Flake::assert(f.s, f.p, f.o, t))
            .collect();
        graph
            .assert_flakes(&restored)
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;

        match self
            .storage
            .split_merge_record(merge_id, chrono::Utc::now())
            .await?
        {
            SplitOutcome::Split(record) => Ok(*record),
            SplitOutcome::AlreadySplit { split_at } => Err(CatalogError::Conflict {
                detail: format!("this merge was already split at {split_at}"),
                existing_id: Some(merge_id),
                kind: ConflictKind::MergeAlreadySplit,
            }),
            SplitOutcome::NotFound => Err(CatalogError::NotFound),
        }
    }

    /// The review queue (Epic 17 Slice F) — pending by default; `filter`
    /// narrows to a specific status, entity kind, or score range.
    ///
    /// # Errors
    ///
    /// [`CatalogError::Storage`] if the query fails.
    pub async fn review_queue(
        &self,
        principal: &Principal,
        filter: &graph_owl_storage::ReviewQueueFilter,
    ) -> Result<(Vec<graph_owl_core::resolution::ReviewQueueEntry>, i64), CatalogError> {
        let _ = principal;
        Ok(self.storage.list_review_queue(filter).await?)
    }

    /// Confirms a queued pair: writes the merge, `decided_by: Human`.
    ///
    /// # Errors
    ///
    /// `NotFound` if the entry does not exist. `Storage` if no graph engine
    /// is configured.
    ///
    /// # Errors
    ///
    /// `NotFound` if the entry does not exist. `Conflict` if it was already
    /// confirmed or rejected — checked *before* deciding, so a second confirm
    /// can never write a second `MergeRecord` for the same pair. `Storage`
    /// if no graph engine is configured.
    pub async fn confirm_review(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Resolution, CatalogError> {
        let entry = self
            .storage
            .get_review_queue_entry(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        if entry.status != graph_owl_core::resolution::ReviewStatus::Pending {
            return Err(CatalogError::Conflict {
                detail: "this review entry has already been decided".to_string(),
                existing_id: Some(id),
                kind: ConflictKind::ReviewAlreadyDecided,
            });
        }
        let decided_by = MergeDecidedBy::Human {
            user_id: principal.id.clone(),
        };
        self.storage
            .decide_review_queue_entry(
                id,
                graph_owl_core::resolution::ReviewStatus::Confirmed,
                decided_by.clone(),
                chrono::Utc::now(),
            )
            .await?;
        self.merge(
            entry.candidate,
            entry.target,
            entry.score,
            entry.evidence,
            decided_by,
        )
        .await
    }

    /// Rejects a queued pair. The decision persists — Slice F's "rejection
    /// is not re-queued on the next re-ingestion of the same draft" is
    /// exactly [`graph_owl_storage::Storage::queue_for_review`]'s
    /// idempotency, applied to a row this leaves in a non-pending state.
    ///
    /// # Errors
    ///
    /// `NotFound` if the entry does not exist. `Conflict` if it was already
    /// confirmed — a merged pair cannot be un-merged by rejecting the queue
    /// entry; that is what splitting the merge is for.
    pub async fn reject_review(&self, principal: &Principal, id: Uuid) -> Result<(), CatalogError> {
        let entry = self
            .storage
            .get_review_queue_entry(id)
            .await?
            .ok_or(CatalogError::NotFound)?;
        if entry.status == graph_owl_core::resolution::ReviewStatus::Confirmed {
            return Err(CatalogError::Conflict {
                detail: "this review entry has already been confirmed".to_string(),
                existing_id: Some(id),
                kind: ConflictKind::ReviewAlreadyDecided,
            });
        }
        let decided_by = MergeDecidedBy::Human {
            user_id: principal.id.clone(),
        };
        self.storage
            .decide_review_queue_entry(
                id,
                graph_owl_core::resolution::ReviewStatus::Rejected,
                decided_by,
                chrono::Utc::now(),
            )
            .await?;
        Ok(())
    }

    // ---- Epic 17 Slice G: mention resolution ----

    /// Resolves a textual mention against the catalog — **never a merge**.
    /// `source` is what the mention was found in (a memory, most commonly);
    /// `Ok(None)` means no candidate cleared
    /// [`graph_owl_resolution::mention::MENTION_THRESHOLD`], which is a
    /// normal outcome, not an error.
    ///
    /// # Errors
    ///
    /// [`CatalogError::Storage`] if a query or the write fails.
    pub async fn resolve_mention(
        &self,
        principal: &Principal,
        source: Uuid,
        mention: graph_owl_core::resolution::TextMention,
    ) -> Result<Option<graph_owl_core::resolution::MentionResolution>, CatalogError> {
        let _ = principal;
        let candidates = self
            .storage
            .search_assets(
                &mention.text,
                mention.expected_type,
                &graph_owl_core::page::PageRequest {
                    limit: 50,
                    after: None,
                },
            )
            .await?;

        let mut best: Option<(Asset, f64)> = None;
        for candidate in candidates.data {
            let ancestor_names: Vec<String> = self
                .storage
                .ancestors_of(candidate.id)
                .await?
                .into_iter()
                .filter(|a| a.id != candidate.id)
                .map(|a| a.name)
                .collect();
            let score = graph_owl_resolution::mention::score_mention(
                &mention.text,
                &mention.context,
                &candidate.name,
                &ancestor_names,
            );
            // `>` rather than `>=`: on an exact tie, the earlier-found
            // candidate wins. Untested at the exact tie boundary on
            // purpose — `search_assets`'s ordering is not part of this
            // module's contract, so a test pinning "candidate A beats
            // candidate B on a tie" would be asserting an ordering this
            // code does not promise, not a real requirement. Which
            // candidate wins a genuine tie is not specified by the plan;
            // that any winner is chosen deterministically, and that a
            // clear winner (this loop's normal case) is picked, are.
            if best
                .as_ref()
                .is_none_or(|(_, best_score)| score > *best_score)
            {
                best = Some((candidate, score));
            }
        }

        let Some((candidate, score)) = best else {
            return Ok(None);
        };
        if !graph_owl_resolution::mention::clears_threshold(score) {
            return Ok(None);
        }

        let resolution = graph_owl_core::resolution::MentionResolution {
            id: Uuid::new_v4(),
            source,
            text: mention.text,
            entity: candidate.id,
            confidence: score,
            resolved_at: chrono::Utc::now(),
        };
        Ok(Some(
            self.storage.record_mention_resolution(resolution).await?,
        ))
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

    /// Run Epic 5's shapes against a draft **before** it is persisted — Epic 16
    /// Slice D.
    ///
    /// The draft is projected to flakes and validated exactly as a stored entity
    /// would be. That is the whole trick: `constraint::validate` takes facts, not
    /// a database, so a not-yet-written entity can be checked with the same code
    /// that checks a written one — no second implementation to drift.
    ///
    /// **Only `Violation` rejects.** A `Warning` lands and is recorded: warnings
    /// exist to be visible, and refusing a push over one would make every shape
    /// author's judgement call a hard gate.
    ///
    /// Returns `None` when the entity is acceptable, or the reason it is not.
    async fn validate_draft(&self, draft: &Asset) -> Result<Option<String>, CatalogError> {
        let Some(graph) = self.graph.as_ref() else {
            // No engine configured is not a silent pass: it is reported by the
            // caller as unvalidated rather than as valid. Here it simply means
            // there are no shapes to run.
            return Ok(None);
        };
        let shape_facts = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                cx: Some(Some(shapes_graph())),
                ..Default::default()
            })
            .await
            .map_err(|e| CatalogError::Storage(StorageError::Unexpected(e.to_string())))?;
        if shape_facts.is_empty() {
            return Ok(None);
        }

        // `read_all` compiles as it reads and hands back the shapes that failed
        // separately. The failures are ignored here on purpose: a shape this
        // server cannot compile is a *validation* problem, reported by
        // `run_validation`'s `refusedShapes`, and letting it block an unrelated
        // push would make one bad shape an outage.
        let (shapes, _refused) = graph_owl_constraint::shapes::read_all(&shape_facts);
        if shapes.is_empty() {
            return Ok(None);
        }

        // `t` is irrelevant to shape evaluation — constraints read the values, not
        // when they were asserted — so a fixed instant keeps this pure and avoids
        // making two identical drafts validate differently.
        let facts = graph_owl_core::projection::asset_to_flakes(draft, 0);
        let report = graph_owl_constraint::validate(&shapes, &facts);

        Ok(report
            .violations
            .iter()
            .find(|v| v.severity == graph_owl_ontology::Severity::Violation)
            .map(|v| {
                // The shape and the constraint are named, per Slice D: "a
                // `Violation` rejects that entity with the shape and constraint
                // named". "Invalid" alone tells a pusher nothing about what to fix.
                format!(
                    "shape `{}` constraint `{}`: {}",
                    v.shape, v.constraint, v.message
                )
            }))
    }

    /// Claim an idempotency key, or learn what it already answered.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn claim_idempotency(
        &self,
        key: &str,
        request_hash: &str,
    ) -> Result<graph_owl_storage::IdempotencyClaim, CatalogError> {
        Ok(self
            .storage
            .claim_idempotency_key(key, request_hash)
            .await?)
    }

    /// Record what a claimed key produced, so a replay returns it.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn record_idempotent_response(
        &self,
        key: &str,
        status: u16,
        body: &serde_json::Value,
    ) -> Result<(), CatalogError> {
        Ok(self
            .storage
            .record_idempotent_response(key, status, body)
            .await?)
    }

    // ---- Epic 16 Slice C: batch file ingestion ----

    /// Register a job for a file that is about to be read.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn create_ingest_job(
        &self,
        id: Uuid,
        format: &str,
        submitted_by: &str,
    ) -> Result<(), CatalogError> {
        self.storage
            .create_ingest_job(&graph_owl_storage::IngestJob {
                id,
                format: format.to_string(),
                state: graph_owl_connectors::job::JobState::Queued.to_string(),
                rows_read: 0,
                accepted: 0,
                rejected: 0,
                failures: Vec::new(),
                halt_reason: None,
                cancel_requested: false,
                submitted_by: submitted_by.to_string(),
                started_at: chrono::Utc::now(),
                heartbeat_at: chrono::Utc::now(),
                finished_at: None,
            })
            .await?;
        Ok(())
    }

    /// A job as it stands, having first failed anything that stopped reporting.
    ///
    /// **The reaper runs on read, not on a timer.** This project refuses a
    /// scheduler (Epic 15 decision 5), and the only moment a stuck job matters is
    /// when somebody asks about it — so the poll that would otherwise wait
    /// forever is exactly the right place to notice.
    ///
    /// # Errors
    ///
    /// `Storage` if the read fails.
    pub async fn ingest_job(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::IngestJob>, CatalogError> {
        self.storage
            .reap_abandoned_ingest_jobs(ABANDONED_AFTER_SECONDS)
            .await?;
        Ok(self.storage.ingest_job(id).await?)
    }

    /// Ask a running job to stop. `false` if it had already finished.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn cancel_ingest_job(&self, id: Uuid) -> Result<bool, CatalogError> {
        Ok(self.storage.cancel_ingest_job(id).await?)
    }

    /// Close a job out as failed before it read anything.
    ///
    /// Separate from the worker's own verdict because there is a window — the
    /// upload spooled, the job registered, the file gone — where nothing was read
    /// and so no `Progress` exists to judge. Leaving it `queued` would be the
    /// worst of the options: a client polling a job that will never move.
    ///
    /// # Errors
    ///
    /// `Storage` if the write fails.
    pub async fn fail_ingest_job(&self, id: Uuid, reason: &str) -> Result<(), CatalogError> {
        self.storage
            .finish_ingest_job(
                id,
                &graph_owl_connectors::job::JobState::Failed.to_string(),
                Some(reason),
            )
            .await?;
        Ok(())
    }

    /// Read a batch file to the end, applying it in chunks.
    ///
    /// Runs after the response has already gone back with `202`, so nothing here
    /// can be reported to the caller directly — every outcome lands in the job
    /// row instead, which is what makes decision 2's "batch is a job" real rather
    /// than a naming choice.
    ///
    /// # Errors
    ///
    /// `Storage` if the job row cannot be updated. A file that cannot be read, or
    /// rows that cannot be applied, are recorded on the job rather than returned:
    /// there is nobody left to return them to.
    pub async fn run_batch_ingest(
        &self,
        id: Uuid,
        source: impl std::io::BufRead + Send + 'static,
        format: graph_owl_connectors::rows::Format,
        principal: Principal,
        error_cap: usize,
    ) -> Result<(), CatalogError> {
        use graph_owl_connectors::job::{Halt, Progress, should_halt, verdict};
        use graph_owl_connectors::rows::{Row, RowError};

        // A bounded channel **is** the memory bound at this layer, the same way
        // the iterator is at the parser's. The reader thread blocks once the
        // channel is full, so a fast file and a slow catalog cannot conspire to
        // buffer a 500k-row backlog in the middle.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Row, RowError>>(BATCH_CHUNK_ROWS);
        // A dedicated thread rather than `spawn_blocking`: the parser is a
        // synchronous iterator that has to live for the whole file, and holding a
        // blocking-pool slot open for the duration of a 500k-row read would
        // starve every other blocking task in the process.
        std::thread::spawn(move || {
            for row in graph_owl_connectors::rows::rows(source, format) {
                if tx.blocking_send(row).is_err() {
                    // The consumer stopped — cancelled, capped, or gone. Nothing
                    // left to read for.
                    break;
                }
            }
        });

        let mut progress = Progress::default();
        let mut halt: Option<Halt> = None;
        let mut pending: Vec<(u64, IngestItem)> = Vec::new();
        let mut failures: Vec<graph_owl_storage::RowFailure> = Vec::new();

        while let Some(next) = rx.recv().await {
            progress.rows_read += 1;
            match read_batch_row(next) {
                Ok((number, item)) => pending.push((number, item)),
                Err(failure) => {
                    progress.rejected += 1;
                    failures.push(failure);
                }
            }

            // Checked every row, not every chunk: the cap exists to stop a file
            // that is wrong in its entirety, and reading another 499 rows to
            // notice would defeat the point.
            if let Some(reached) = should_halt(progress, error_cap, false) {
                halt = Some(reached);
                break;
            }

            if pending.len() + failures.len() >= BATCH_CHUNK_ROWS
                && let Some(cancelled) = self
                    .flush_batch_chunk(id, &principal, &mut progress, &mut pending, &mut failures)
                    .await?
            {
                halt = Some(cancelled);
                break;
            }
        }

        if halt.is_none() {
            self.flush_batch_chunk(id, &principal, &mut progress, &mut pending, &mut failures)
                .await?;
        } else {
            // Still reported, so the counts a client polls describe everything
            // that was actually read — a halt must not silently discard the work
            // done in the chunk it happened in.
            self.storage
                .report_ingest_progress(id, storage_progress(progress), &failures)
                .await?;
        }

        let state = verdict(progress, halt.as_ref());
        self.storage
            .finish_ingest_job(
                id,
                &state.to_string(),
                halt.as_ref().map(halt_reason).as_deref(),
            )
            .await?;
        Ok(())
    }

    /// Apply what has accumulated and report it, returning a halt if the job was
    /// cancelled while the chunk was in flight.
    async fn flush_batch_chunk(
        &self,
        id: Uuid,
        principal: &Principal,
        progress: &mut graph_owl_connectors::job::Progress,
        pending: &mut Vec<(u64, IngestItem)>,
        failures: &mut Vec<graph_owl_storage::RowFailure>,
    ) -> Result<Option<graph_owl_connectors::job::Halt>, CatalogError> {
        if !pending.is_empty() {
            let numbers: Vec<u64> = pending.iter().map(|(number, _)| *number).collect();
            let items: Vec<IngestItem> = pending.drain(..).map(|(_, item)| item).collect();

            match self.ingest(principal, items.clone(), Vec::new()).await {
                Ok(outcomes) => {
                    for outcome in outcomes {
                        let row = numbers.get(outcome.index).copied().unwrap_or_default();
                        if outcome.status >= 400 {
                            progress.rejected += 1;
                            failures.push(graph_owl_storage::RowFailure {
                                row,
                                detail: outcome.problem.unwrap_or_else(|| "rejected".to_string()),
                            });
                        } else {
                            progress.accepted += 1;
                        }
                    }
                }
                Err(chunk_error) => {
                    // A chunk-level failure is a duplicate FQN or a containment
                    // cycle *within these 500 rows* — a property of the chunk, not
                    // of any one row, so `ingest` has nothing per-item to report.
                    //
                    // Retried one row at a time rather than written off. A file
                    // with one repeated FQN would otherwise cost 499 innocent rows
                    // per occurrence, and a client would see a rejection list that
                    // named rows with nothing wrong with them. The extra round
                    // trips are paid only on the failure path.
                    let detail = batch_detail(&chunk_error);
                    for (number, item) in numbers.iter().zip(items) {
                        match self.ingest(principal, vec![item], Vec::new()).await {
                            Ok(outcomes) if outcomes.iter().all(|outcome| outcome.status < 400) => {
                                progress.accepted += 1;
                            }
                            Ok(outcomes) => {
                                progress.rejected += 1;
                                failures.push(graph_owl_storage::RowFailure {
                                    row: *number,
                                    detail: outcomes
                                        .into_iter()
                                        .find_map(|outcome| outcome.problem)
                                        .unwrap_or_else(|| detail.clone()),
                                });
                            }
                            Err(row_error) => {
                                progress.rejected += 1;
                                failures.push(graph_owl_storage::RowFailure {
                                    row: *number,
                                    detail: batch_detail(&row_error),
                                });
                            }
                        }
                    }
                }
            }
        }

        let cancelled = self
            .storage
            .report_ingest_progress(id, storage_progress(*progress), failures)
            .await?;
        // Cleared **after** the report, because failures are appended in storage:
        // sending them again would double every entry in the job's report.
        failures.clear();

        Ok(cancelled.then_some(graph_owl_connectors::job::Halt::Cancelled))
    }

    // ---- Epic 16 Slice A: synchronous push ----

    /// Push a batch, applying what is valid and reporting what is not.
    ///
    /// **Partial success, per item.** A 1000-item push with one bad row must land
    /// 999 — an all-or-nothing batch makes a pusher's retry loop re-send
    /// everything to fix one typo, and at that size somebody stops retrying.
    ///
    /// Parents are applied before children **within the batch**, because a pusher
    /// walking a source emits what it finds when it finds it; requiring a
    /// topological submission would push the catalog's model onto every adapter
    /// author, which `16-ingestion-apis.md` decision 1 refuses.
    ///
    /// # Errors
    ///
    /// `Validation` when the batch cannot be ordered at all — a duplicate FQN or a
    /// containment cycle is a property of the *batch*, not of one item, and there
    /// is no partial success to report.
    pub async fn ingest(
        &self,
        principal: &Principal,
        items: Vec<IngestItem>,
        edges: Vec<IngestEdge>,
    ) -> Result<Vec<IngestOutcome>, CatalogError> {
        // FQNs are derived here, not taken from the client — `Asset` documents them
        // as "derived from the parent chain, never client-set", and a batch that
        // could name its own FQN could place an entity outside its parent.
        let drafts: Vec<graph_owl_connectors::ingest::Draft> = items
            .iter()
            .enumerate()
            .map(|(index, item)| graph_owl_connectors::ingest::Draft {
                index,
                fully_qualified_name: match &item.parent_fqn {
                    Some(parent) => format!("{parent}.{}", item.name),
                    None => item.name.clone(),
                },
                parent_fqn: item.parent_fqn.clone(),
            })
            .collect();

        let order = graph_owl_connectors::ingest::apply_order(&drafts).map_err(|e| {
            CatalogError::Validation(vec![validation::FieldError::new(
                "items",
                validation::FieldErrorCode::Type,
                e.to_string(),
            )])
        })?;

        // FQN → id for parents applied earlier in this same batch. Seeded empty:
        // a parent already in the catalog is resolved by a lookup instead.
        let mut landed: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();
        let mut outcomes: Vec<IngestOutcome> = Vec::with_capacity(items.len());

        for index in order {
            let item = &items[index];
            let draft = &drafts[index];

            let parent_id = match &item.parent_fqn {
                None => None,
                Some(parent_fqn) => match landed.get(parent_fqn) {
                    Some(id) => Some(*id),
                    None => match self.storage.get_asset_by_fqn(parent_fqn).await? {
                        Some(parent) => Some(parent.id),
                        None => {
                            outcomes.push(IngestOutcome {
                                index,
                                status: 400,
                                id: None,
                                problem: Some(format!(
                                    "parent `{parent_fqn}` is neither in this batch nor in the catalog"
                                )),
                            });
                            continue;
                        }
                    },
                },
            };

            // **Validation runs before the write, and therefore before the FQN
            // uniqueness check.** Slice D: a draft that is both shape-invalid and
            // FQN-conflicting must report the shape violation, because that is the
            // actionable one — a conflict tells a pusher to rename, which is the
            // wrong fix for a malformed entity.
            let candidate = Asset {
                id: Uuid::nil(),
                kind: item.kind,
                name: item.name.clone(),
                fully_qualified_name: draft.fully_qualified_name.clone(),
                parent_id,
                description: item.description.clone(),
                properties: item.properties.clone(),
                owners: Vec::new(),
                version: graph_owl_core::envelope::EntityVersion::initial(),
                updated_by: principal.id.clone(),
                change_description: None,
                deleted: false,
                deleted_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            if let Some(reason) = self.validate_draft(&candidate).await? {
                outcomes.push(IngestOutcome {
                    index,
                    status: 400,
                    id: None,
                    problem: Some(reason),
                });
                continue;
            }

            let request = UpsertAsset {
                kind: item.kind,
                name: item.name.clone(),
                parent_id,
                description: item.description.clone(),
                properties: item.properties.clone(),
            };
            match self.upsert_asset(principal, request).await {
                Ok(asset) => {
                    landed.insert(draft.fully_qualified_name.clone(), asset.id);
                    outcomes.push(IngestOutcome {
                        index,
                        status: 200,
                        id: Some(asset.id),
                        problem: None,
                    });
                }
                // **One item's failure is reported, not raised.** Raising would
                // abandon the rest of the batch, which is the all-or-nothing
                // behaviour this slice exists to avoid.
                Err(error) => outcomes.push(IngestOutcome {
                    index,
                    status: 400,
                    id: None,
                    problem: Some(format!("{error:?}")),
                }),
            }
        }

        // **Edges after every entity**, because an edge's endpoints may be
        // anywhere in the batch — including an item submitted after it. Ordering
        // entities among themselves is a containment problem with one answer;
        // ordering an edge against them is not, so edges simply go last.
        //
        // Indexes continue from the entity range so a `207` addresses the whole
        // request with one numbering, rather than making a client know which of
        // two lists an index refers to.
        for (offset, edge) in edges.iter().enumerate() {
            let index = items.len() + offset;
            let resolve = |fqn: &String| -> Option<Uuid> { landed.get(fqn).copied() };

            let from = match resolve(&edge.from_fqn) {
                Some(id) => Some(id),
                None => self
                    .storage
                    .get_asset_by_fqn(&edge.from_fqn)
                    .await?
                    .map(|a| a.id),
            };
            let to = match resolve(&edge.to_fqn) {
                Some(id) => Some(id),
                None => self
                    .storage
                    .get_asset_by_fqn(&edge.to_fqn)
                    .await?
                    .map(|a| a.id),
            };

            let (Some(from), Some(to)) = (from, to) else {
                let missing = if from.is_none() {
                    &edge.from_fqn
                } else {
                    &edge.to_fqn
                };
                outcomes.push(IngestOutcome {
                    index,
                    status: 400,
                    id: None,
                    problem: Some(format!(
                        "`{missing}` is neither in this batch nor in the catalog"
                    )),
                });
                continue;
            };

            let result = match graph_owl_core::relationship_type::RelationshipType::parse(
                &edge.relationship,
            ) {
                Ok(relationship) => self
                    .assert_lineage(
                        principal,
                        from,
                        to,
                        relationship,
                        graph_owl_core::lineage::LineageDetails {
                            // A push is a connector speaking, not a person:
                            // `Manual` would claim somebody vouched for this.
                            source: graph_owl_core::lineage::LineageSource::Connector,
                            query: edge.query.clone(),
                            description: edge.description.clone(),
                        },
                    )
                    .await
                    .map(|edge| edge.id),
                Err(unknown) => Err(CatalogError::Validation(vec![validation::FieldError::new(
                    "relationship",
                    validation::FieldErrorCode::Type,
                    format!("`{}` is not a relationship type", unknown.got),
                )])),
            };

            match result {
                Ok(id) => outcomes.push(IngestOutcome {
                    index,
                    status: 200,
                    id: Some(id),
                    problem: None,
                }),
                Err(error) => outcomes.push(IngestOutcome {
                    index,
                    status: 400,
                    id: None,
                    problem: Some(format!("{error:?}")),
                }),
            }
        }

        // Back into submitted order. The caller reads results against the batch it
        // sent, and application order is an implementation detail of this method.
        outcomes.sort_by_key(|outcome| outcome.index);
        Ok(outcomes)
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

/// One entity in a push — Epic 16 Slice A.
///
/// The parent is named by **FQN, not id**: a pusher walking an external source
/// knows the path it is at, not the UUIDs this catalog assigned.
#[derive(Debug, Clone)]
pub struct IngestItem {
    pub kind: AssetKind,
    pub name: String,
    pub parent_fqn: Option<String>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

/// How many rows a batch job applies in one round trip.
///
/// **Half the synchronous ceiling**, so a chunk is never a load the synchronous
/// path would itself refuse — the batch path gets its size from the same
/// argument rather than inventing a second one, and the margin leaves room for
/// the edges a future slice will apply alongside.
const BATCH_CHUNK_ROWS: usize = 500;

/// How long a job may go without a heartbeat before it is presumed dead.
///
/// A chunk is at most [`BATCH_CHUNK_ROWS`] rows and heartbeats when it lands, so
/// a healthy job reports on the order of seconds. Five minutes is two orders of
/// magnitude above that, and the asymmetry is deliberate: declaring a live job
/// abandoned corrupts a real result, while waiting too long only delays the
/// moment a crashed job stops reading `running`.
const ABANDONED_AFTER_SECONDS: i64 = 300;

/// The default number of rejected rows a job tolerates before giving up.
///
/// From the plan: "errors accumulate to a bounded cap (default 1000) after which
/// the job fails with 'too many errors' rather than producing an unreadable
/// report". The cap is about legibility, not about correctness — a file that
/// produces a thousand rejections is wrong in a way no per-row list will explain.
pub const BATCH_ERROR_CAP: usize = 1000;

fn storage_progress(
    progress: graph_owl_connectors::job::Progress,
) -> graph_owl_storage::IngestProgress {
    graph_owl_storage::IngestProgress {
        rows_read: i64::try_from(progress.rows_read).unwrap_or(i64::MAX),
        accepted: i64::try_from(progress.accepted).unwrap_or(i64::MAX),
        rejected: i64::try_from(progress.rejected).unwrap_or(i64::MAX),
    }
}

/// Why a job stopped, in words a client can act on.
fn halt_reason(halt: &graph_owl_connectors::job::Halt) -> String {
    match halt {
        graph_owl_connectors::job::Halt::ErrorCap { cap } => format!(
            "too many errors: {cap} rows were rejected, so reading stopped. \
             A file failing this often is usually wrong in one way — check the \
             first few rejections rather than all of them"
        ),
        graph_owl_connectors::job::Halt::Cancelled => {
            "cancelled; the counts describe what had landed when it stopped".to_string()
        }
        graph_owl_connectors::job::Halt::Abandoned => {
            "abandoned: the worker stopped reporting".to_string()
        }
    }
}

/// One row, as far as it can be understood without touching the catalog.
///
/// A free function rather than a method: it needs nothing from the catalog, and
/// a `&self` it never reads would suggest otherwise to the next reader.
fn read_batch_row(
    next: Result<graph_owl_connectors::rows::Row, graph_owl_connectors::rows::RowError>,
) -> Result<(u64, IngestItem), graph_owl_storage::RowFailure> {
    let row = next.map_err(
        |graph_owl_connectors::rows::RowError::Malformed { number, detail }| {
            graph_owl_storage::RowFailure {
                row: number,
                detail,
            }
        },
    )?;
    let draft = graph_owl_connectors::batch::draft_from_row(&row).map_err(
        |graph_owl_connectors::rows::RowError::Malformed { number, detail }| {
            graph_owl_storage::RowFailure {
                row: number,
                detail,
            }
        },
    )?;
    let kind = AssetKind::parse(&draft.kind).map_err(|_| graph_owl_storage::RowFailure {
        row: row.number,
        detail: format!(
            "`{}` is not an asset kind; expected one of: {}",
            draft.kind,
            AssetKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })?;
    Ok((
        row.number,
        IngestItem {
            kind,
            name: draft.name,
            parent_fqn: draft.parent_fqn,
            description: draft.description,
            properties: draft.properties,
        },
    ))
}

/// A catalog failure rendered for a job report.
///
/// `CatalogError` has no `Display` on purpose — HTTP handlers *map* it to a
/// status and a problem body rather than printing it — but a batch row has no
/// handler behind it, and "something went wrong on row 41,203" is not a report.
fn batch_detail(error: &CatalogError) -> String {
    match error {
        CatalogError::NotFound => "no such entity".to_string(),
        CatalogError::Conflict { detail, .. } => detail.clone(),
        CatalogError::Validation(fields) => fields
            .iter()
            .map(|field| format!("{}: {}", field.field, field.detail))
            .collect::<Vec<_>>()
            .join("; "),
        CatalogError::IllegalRelationship {
            from,
            relationship,
            to,
        } => format!("{relationship:?} is not legal from {from:?} to {to:?}"),
        CatalogError::PreconditionFailed { .. } => {
            "the entity changed while this batch was in flight".to_string()
        }
        CatalogError::Forbidden => "not permitted".to_string(),
        CatalogError::Unauthenticated => "signature verification failed".to_string(),
        CatalogError::Storage(inner) => inner.to_string(),
    }
}

/// One edge in a push — Epic 16 Slice A.
///
/// Endpoints are named by **FQN**, resolved against this batch first and the
/// catalog second: "a relationship whose endpoints are in the same batch resolves"
/// is a stated criterion, because a pusher cannot pre-create in dependency order.
///
/// **Every pushed edge is a lineage edge, and there is deliberately no option for
/// anything else.** Epic 1's `Relationship` operates on the `tables` relation,
/// which is not `assets` — a push creates assets, so a plain relationship between
/// two of them can never resolve. Offering the choice and failing every time is
/// worse than not offering it: the option would look like a capability, and the
/// `NotFound` it produced would look like a missing entity rather than a
/// mismatched model. Found by a test doing exactly that.
#[derive(Debug, Clone)]
pub struct IngestEdge {
    pub from_fqn: String,
    pub to_fqn: String,
    /// A lineage relationship — `feeds`, `derivedFrom`, and so on.
    pub relationship: String,
    pub query: Option<String>,
    pub description: Option<String>,
}

/// What applying a mapping to a payload produced — Epic 18 Slice C.
///
/// Every variant is a legitimate result of a dry run, not an error: a
/// mapping that does not fit a sample payload is exactly what dry-running it
/// is for, and collapsing these into a single `CatalogError` would lose the
/// distinction a client needs to know *what* to fix — the mapping's field
/// paths, or the shapes the resulting entity would violate.
#[derive(Debug, Clone, PartialEq)]
pub enum MappingOutcome {
    /// Every required field resolved and the draft passed shape validation.
    Draft(graph_owl_connectors::batch::RowDraft),
    /// A required field's path resolved to nothing.
    MissingField { field: &'static str },
    /// `kind` resolved to a string that names no known asset kind.
    InvalidKind { kind: String },
    /// The draft resolved but a shape rejected it — names the shape and
    /// constraint, from [`Catalog::validate_draft`].
    ShapeViolation { reason: String },
}

/// The internal twin of [`MappingOutcome`] — `resolve_and_validate_draft`'s
/// own return type, carrying `kind`/`parent_id` alongside a successful
/// draft so [`Catalog::process_inbound_event`] does not have to re-derive
/// them. Not `pub`: `MappingOutcome` is the shape a caller outside this
/// function ever sees.
enum MappingResolution {
    Ready {
        draft: graph_owl_connectors::batch::RowDraft,
        kind: AssetKind,
        parent_id: Option<Uuid>,
        fully_qualified_name: String,
    },
    MissingField {
        field: &'static str,
    },
    InvalidKind {
        kind: String,
    },
    ShapeViolation {
        reason: String,
    },
}

/// What a replay over a window did — Epic 18 Slice D.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaySummary {
    /// Events actually (re)processed — excludes `skipped`.
    pub attempted: usize,
    /// Reached `Applied` this time.
    pub applied: usize,
    /// Reprocessed but still not `Applied` (mapping/shape still rejects it,
    /// or the endpoint or mapping is gone).
    pub still_failed: usize,
    /// Already `Applied` or `Duplicate` — left untouched, not reprocessed.
    pub skipped: usize,
}

/// What happened to one pushed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestOutcome {
    /// Position in the **submitted** batch, so a client can match this to what it
    /// sent without re-deriving anything.
    pub index: usize,
    pub status: u16,
    pub id: Option<Uuid>,
    pub problem: Option<String>,
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

    // `Forbidden` cannot currently arise from anything `batch_detail` reports
    // on (no ingest path returns it), so it needs its own direct test rather
    // than one reached through a batch run — the only way to prove the arm
    // is not `todo!()`.
    #[test]
    fn batch_detail_reports_forbidden() {
        assert_eq!(batch_detail(&CatalogError::Forbidden), "not permitted");
    }

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
    /// The fake's blocking-key computation (Epic 17 Slice B) — derived
    /// on demand from the current `assets` list rather than a second store
    /// kept in sync, since the fake already rebuilds every read from that
    /// one list. Must derive identically to `PostgresStorage`'s stored keys,
    /// so this calls the exact same pure functions rather than a
    /// reimplementation that could quietly disagree.
    fn asset_blocking_key_values(asset: &Asset, all: &[Asset]) -> [String; 4] {
        let column_hash = if asset.kind == AssetKind::Table {
            let columns: Vec<String> = all
                .iter()
                .filter(|a| {
                    a.parent_id == Some(asset.id) && a.kind == AssetKind::Column && !a.deleted
                })
                .map(|a| a.name.clone())
                .collect();
            graph_owl_core::blocking::column_hash_key(&columns)
        } else {
            graph_owl_core::blocking::column_hash_key(&[])
        };
        [
            graph_owl_core::blocking::normalized_fqn_key(&asset.fully_qualified_name),
            graph_owl_core::blocking::name_parent_key(
                &asset.name,
                graph_owl_core::fqn::parent(&asset.fully_qualified_name),
            ),
            graph_owl_core::blocking::soundex(&asset.name),
            column_hash,
        ]
    }

    /// The fake must derive the same candidates Postgres would (Epic 17
    /// Slice B), since `graph_owl_resolution` will eventually run against
    /// either backend interchangeably — proven directly against
    /// `InMemoryStorage`, mirroring `graph-owl-storage-postgres`'s own
    /// `entity_blocking_keys` repository tests rather than only against
    /// `Catalog`, which has no resolution-facing method yet.
    #[tokio::test]
    async fn fake_storage_computes_resolution_candidates_via_blocking_keys() {
        let storage = InMemoryStorage::default();
        let now = chrono::Utc::now();
        let make = |name: &str, fqn: &str| Asset {
            id: Uuid::new_v4(),
            kind: AssetKind::Service,
            name: name.to_string(),
            fully_qualified_name: fqn.to_string(),
            parent_id: None,
            description: None,
            properties: None,
            owners: Vec::new(),
            version: graph_owl_core::envelope::EntityVersion::initial(),
            updated_by: "system".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };

        let lower = storage
            .upsert_asset(make("orders", "svc.orders"))
            .await
            .expect("lower");
        let upper = storage
            .upsert_asset(make("ORDERS", "SVC.ORDERS"))
            .await
            .expect("upper");
        let unrelated = storage
            .upsert_asset(make("zzqxw", "svc.zzqxw"))
            .await
            .expect("unrelated");

        let candidates = storage
            .resolution_candidates(lower.id)
            .await
            .expect("candidates");

        assert!(
            candidates.iter().any(|a| a.id == upper.id),
            "a case-variant FQN should be a candidate via the normalized-FQN key"
        );
        assert!(
            !candidates.iter().any(|a| a.id == unrelated.id),
            "an unrelated asset must not appear as a candidate"
        );
    }

    use graph_owl_storage::{ReviewQueueFilter, SplitOutcome, Storage};
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
        followers: Mutex<Vec<(Uuid, String)>>,
        #[allow(clippy::type_complexity)]
        idempotency: Mutex<Vec<(String, String, u16, serde_json::Value)>>,
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
        jobs: Mutex<Vec<graph_owl_storage::IngestJob>>,
        glossaries: Mutex<Vec<graph_owl_storage::Glossary>>,
        glossary_terms: Mutex<Vec<graph_owl_storage::GlossaryTermRecord>>,
        /// `(owner, relation)`, exactly the shape the port carries.
        #[allow(clippy::type_complexity)]
        term_relations: Mutex<Vec<(Uuid, graph_owl_core::glossary::SkosRelation)>>,
        term_reviewers: Mutex<Vec<(Uuid, Vec<String>)>>,
        /// `(term_id, target_fqn)`.
        term_attachments: Mutex<Vec<(Uuid, String)>>,
        metrics: Mutex<Vec<graph_owl_storage::MetricRecord>>,
        pub(super) merge_records: Mutex<Vec<graph_owl_core::resolution::MergeRecord>>,
        pub(super) resolution_queue: Mutex<Vec<graph_owl_core::resolution::ReviewQueueEntry>>,
        pub(super) mention_resolutions: Mutex<Vec<graph_owl_core::resolution::MentionResolution>>,
        /// `(endpoint, secret)` — the secret kept beside the public record so
        /// a test can prove it is kept and still never returned by the read
        /// path, matching the `connectors` field's own precedent.
        #[allow(clippy::type_complexity)]
        webhook_endpoints: Mutex<Vec<(graph_owl_storage::WebhookEndpoint, Option<Vec<u8>>)>>,
        inbound_events: Mutex<Vec<graph_owl_core::webhook::InboundEvent>>,
        /// `(subscription, secret)` — same reasoning as `webhook_endpoints`.
        #[allow(clippy::type_complexity)]
        stream_subscriptions: Mutex<Vec<(graph_owl_storage::StreamSubscription, Option<Vec<u8>>)>>,
        stream_dead_letters: Mutex<Vec<graph_owl_storage::StreamDeadLetter>>,
        mapping_versions: Mutex<Vec<graph_owl_storage::Mapping>>,
        entity_last_applied:
            Mutex<std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>>,
        extraction_runs: Mutex<Vec<graph_owl_storage::ExtractionRunRecord>>,
        extraction_claims: Mutex<Vec<graph_owl_storage::QueuedClaimRecord>>,
        extraction_discards: Mutex<Vec<graph_owl_storage::DiscardedClaimRecord>>,
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

        async fn claim_idempotency_key(
            &self,
            key: &str,
            request_hash: &str,
        ) -> Result<graph_owl_storage::IdempotencyClaim, StorageError> {
            let mut held = self.idempotency.lock().expect("lock");
            // The lock is this double's atomicity, standing in for the port's
            // `ON CONFLICT DO NOTHING`. A double that read, released, and wrote
            // would pass every single-threaded test and hide the exact race the
            // criterion is about.
            match held.iter().find(|(k, _, _, _)| k == key) {
                None => {
                    held.push((
                        key.to_string(),
                        request_hash.to_string(),
                        0,
                        serde_json::Value::Null,
                    ));
                    Ok(graph_owl_storage::IdempotencyClaim::Claimed)
                }
                Some((_, stored, _, _)) if stored != request_hash => {
                    Ok(graph_owl_storage::IdempotencyClaim::Mismatch)
                }
                Some((_, _, 0, _)) => Ok(graph_owl_storage::IdempotencyClaim::InFlight),
                Some((_, _, status, body)) => Ok(graph_owl_storage::IdempotencyClaim::Replay {
                    status: *status,
                    body: body.clone(),
                }),
            }
        }

        async fn record_idempotent_response(
            &self,
            key: &str,
            status: u16,
            body: &serde_json::Value,
        ) -> Result<(), StorageError> {
            let mut held = self.idempotency.lock().expect("lock");
            if let Some(entry) = held.iter_mut().find(|(k, _, _, _)| k == key) {
                entry.2 = status;
                entry.3 = body.clone();
            }
            Ok(())
        }

        // ---- Epic 16 Slice C, and as strict as the port ----

        async fn create_ingest_job(
            &self,
            job: &graph_owl_storage::IngestJob,
        ) -> Result<(), StorageError> {
            self.guard_write("create_ingest_job");
            self.jobs.lock().expect("lock").push(job.clone());
            Ok(())
        }

        async fn ingest_job(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::IngestJob>, StorageError> {
            Ok(self
                .jobs
                .lock()
                .expect("lock")
                .iter()
                .find(|job| job.id == id)
                .cloned())
        }

        async fn report_ingest_progress(
            &self,
            id: Uuid,
            progress: graph_owl_storage::IngestProgress,
            new_failures: &[graph_owl_storage::RowFailure],
        ) -> Result<bool, StorageError> {
            self.guard_write("report_ingest_progress");
            let mut held = self.jobs.lock().expect("lock");
            let Some(job) = held.iter_mut().find(|job| job.id == id) else {
                // The port stops a worker whose row has gone, and so does this:
                // a double that kept going would hide the case where it matters.
                return Ok(true);
            };
            job.rows_read = progress.rows_read;
            job.accepted = progress.accepted;
            job.rejected = progress.rejected;
            // Appended, exactly as the SQL does — a double that replaced would
            // pass a test that the real adapter would double-count.
            job.failures.extend_from_slice(new_failures);
            job.heartbeat_at = chrono::Utc::now();
            job.state = "running".to_string();
            Ok(job.cancel_requested)
        }

        async fn finish_ingest_job(
            &self,
            id: Uuid,
            state: &str,
            halt_reason: Option<&str>,
        ) -> Result<(), StorageError> {
            self.guard_write("finish_ingest_job");
            let mut held = self.jobs.lock().expect("lock");
            if let Some(job) = held.iter_mut().find(|job| job.id == id) {
                job.state = state.to_string();
                job.halt_reason = halt_reason.map(ToString::to_string);
                job.finished_at = Some(chrono::Utc::now());
            }
            Ok(())
        }

        async fn cancel_ingest_job(&self, id: Uuid) -> Result<bool, StorageError> {
            self.guard_write("cancel_ingest_job");
            let mut held = self.jobs.lock().expect("lock");
            let Some(job) = held
                .iter_mut()
                .find(|job| job.id == id && job.finished_at.is_none())
            else {
                return Ok(false);
            };
            job.cancel_requested = true;
            Ok(true)
        }

        async fn reap_abandoned_ingest_jobs(
            &self,
            stale_after_seconds: i64,
        ) -> Result<u64, StorageError> {
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(stale_after_seconds);
            let mut held = self.jobs.lock().expect("lock");
            let mut reaped = 0;
            for job in held
                .iter_mut()
                .filter(|job| job.finished_at.is_none() && job.heartbeat_at < cutoff)
            {
                job.state = "failed".to_string();
                job.halt_reason = Some("abandoned: the worker stopped reporting".to_string());
                job.finished_at = Some(chrono::Utc::now());
                reaped += 1;
            }
            Ok(reaped)
        }

        async fn child_teams(
            &self,
            id: &str,
        ) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
            Ok(self
                .teams
                .lock()
                .expect("lock")
                .iter()
                .filter(|team| team.parent_team_id.as_deref() == Some(id))
                .cloned()
                .collect())
        }

        async fn would_cycle(&self, team: &str, parent: &str) -> Result<bool, StorageError> {
            if team == parent {
                return Ok(true);
            }
            // Walks ancestry, as the port specifies. A double that compared only
            // the immediate parent would pass every depth-1 test and let
            // `A → B → C → A` through — which is Slice B's named mutator watch.
            let teams = self.teams.lock().expect("lock");
            let mut node = Some(parent.to_string());
            let mut seen = 0;
            while let Some(current) = node {
                if current == team {
                    return Ok(true);
                }
                // Guard against an already-corrupt chain rather than looping
                // forever while deciding whether a loop would be created.
                seen += 1;
                if seen > teams.len() {
                    return Ok(true);
                }
                node = teams
                    .iter()
                    .find(|candidate| candidate.id == current)
                    .and_then(|candidate| candidate.parent_team_id.clone());
            }
            Ok(false)
        }

        async fn follow_asset(
            &self,
            asset_id: Uuid,
            user_id: &str,
        ) -> Result<graph_owl_storage::FollowOutcome, StorageError> {
            self.guard_write("follow_asset");
            let mut held = self.followers.lock().expect("lock");
            let edge = (asset_id, user_id.to_string());
            if held.contains(&edge) {
                return Ok(graph_owl_storage::FollowOutcome::AlreadyFollowing);
            }
            held.push(edge);
            Ok(graph_owl_storage::FollowOutcome::Followed)
        }

        async fn unfollow_asset(&self, asset_id: Uuid, user_id: &str) -> Result<(), StorageError> {
            self.guard_write("unfollow_asset");
            self.followers
                .lock()
                .expect("lock")
                .retain(|(id, who)| !(*id == asset_id && who == user_id));
            Ok(())
        }

        async fn assets_followed_by(
            &self,
            user_id: &str,
            page: &PageRequest,
        ) -> Result<Page<Asset>, StorageError> {
            let followed: Vec<Uuid> = self
                .followers
                .lock()
                .expect("lock")
                .iter()
                .filter(|(_, who)| who == user_id)
                .map(|(id, _)| *id)
                .collect();
            let assets: Vec<Asset> = self
                .assets
                .lock()
                .expect("lock")
                .iter()
                .filter(|asset| followed.contains(&asset.id) && !asset.deleted)
                .cloned()
                .collect();
            Ok(Page::from_overfetch(assets, page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }))
        }

        async fn follower_count(&self, asset_id: Uuid) -> Result<i64, StorageError> {
            Ok(i64::try_from(
                self.followers
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|(id, _)| *id == asset_id)
                    .count(),
            )
            .unwrap_or(i64::MAX))
        }

        async fn principal_holdings(
            &self,
            principal: &graph_owl_core::ownership::OwnerRef,
        ) -> Result<graph_owl_storage::Holdings, StorageError> {
            let owners = self.owners.lock().expect("lock");
            let assets = self.assets.lock().expect("lock");
            // Keyed on the wire string because `AssetKind` is deliberately not
            // `Ord` — deriving an ordering on a domain enum to satisfy a test
            // double would put an arbitrary rank on the hierarchy.
            let mut by_kind: std::collections::BTreeMap<&'static str, (AssetKind, i64)> =
                std::collections::BTreeMap::new();
            for (asset_id, list) in owners.iter() {
                if list
                    .iter()
                    .any(|o| o.id == principal.id && o.kind == principal.kind)
                    && let Some(asset) = assets.iter().find(|a| a.id == *asset_id)
                {
                    by_kind
                        .entry(asset.kind.as_str())
                        .or_insert((asset.kind, 0))
                        .1 += 1;
                }
            }
            drop(owners);
            drop(assets);
            let child_teams = match principal.kind {
                graph_owl_core::ownership::OwnerKind::Team => self
                    .teams
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|t| t.parent_team_id.as_deref() == Some(principal.id.as_str()))
                    .map(|t| t.id.clone())
                    .collect(),
                graph_owl_core::ownership::OwnerKind::User => Vec::new(),
            };
            Ok(graph_owl_storage::Holdings {
                owned_by_kind: by_kind.into_values().collect(),
                child_teams,
            })
        }

        async fn delete_principal(
            &self,
            principal: &graph_owl_core::ownership::OwnerRef,
            reassign_to: Option<&graph_owl_core::ownership::OwnerRef>,
        ) -> Result<graph_owl_storage::PrincipalDeletion, StorageError> {
            self.guard_write("delete_principal");
            let known = match principal.kind {
                graph_owl_core::ownership::OwnerKind::User => self
                    .users
                    .lock()
                    .expect("lock")
                    .iter()
                    .any(|u| u.id == principal.id),
                graph_owl_core::ownership::OwnerKind::Team => self
                    .teams
                    .lock()
                    .expect("lock")
                    .iter()
                    .any(|t| t.id == principal.id),
            };
            if !known {
                return Ok(graph_owl_storage::PrincipalDeletion::NotFound);
            }

            let holdings = self.principal_holdings(principal).await?;
            let mut reassigned = 0_i64;
            if let Some(target) = reassign_to {
                let target_known = match target.kind {
                    graph_owl_core::ownership::OwnerKind::User => self
                        .users
                        .lock()
                        .expect("lock")
                        .iter()
                        .any(|u| u.id == target.id),
                    graph_owl_core::ownership::OwnerKind::Team => self
                        .teams
                        .lock()
                        .expect("lock")
                        .iter()
                        .any(|t| t.id == target.id),
                };
                if !target_known {
                    return Ok(graph_owl_storage::PrincipalDeletion::UnknownTarget);
                }
                let mut owners = self.owners.lock().expect("lock");
                let mut assets = self.assets.lock().expect("lock");
                for (asset_id, list) in owners.iter_mut() {
                    let mut moved = false;
                    for entry in list.iter_mut() {
                        if entry.id == principal.id && entry.kind == principal.kind {
                            entry.id = target.id.clone();
                            entry.kind = target.kind;
                            moved = true;
                        }
                    }
                    if moved {
                        reassigned += 1;
                        // The version bump the port promises; without it a transfer
                        // is invisible to anyone subscribed to Minor changes.
                        if let Some(asset) = assets.iter_mut().find(|a| a.id == *asset_id) {
                            asset.version.minor += 1;
                        }
                    }
                }
            } else if !holdings.is_empty() {
                return Ok(graph_owl_storage::PrincipalDeletion::StillHolds(Box::new(
                    holdings,
                )));
            }

            match principal.kind {
                graph_owl_core::ownership::OwnerKind::User => self
                    .users
                    .lock()
                    .expect("lock")
                    .retain(|u| u.id != principal.id),
                graph_owl_core::ownership::OwnerKind::Team => self
                    .teams
                    .lock()
                    .expect("lock")
                    .retain(|t| t.id != principal.id),
            }
            Ok(graph_owl_storage::PrincipalDeletion::Deleted { reassigned })
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

        async fn upsert_webhook_endpoint(
            &self,
            endpoint: graph_owl_storage::WebhookEndpoint,
            secret: Option<&[u8]>,
        ) -> Result<graph_owl_storage::WebhookEndpoint, StorageError> {
            let mut held = self.webhook_endpoints.lock().unwrap();
            if held
                .iter()
                .any(|(e, _)| e.path == endpoint.path && e.id != endpoint.id)
            {
                return Err(StorageError::Conflict {
                    detail: format!("path '{}' is already registered", endpoint.path),
                    existing_id: None,
                    kind: ConflictKind::WebhookPathExists,
                });
            }
            // `None` leaves an existing key alone, matching
            // `upsert_connector_config`'s own fake exactly.
            let existing_secret = held
                .iter()
                .find(|(e, _)| e.id == endpoint.id)
                .and_then(|(_, s)| s.clone());
            held.retain(|(e, _)| e.id != endpoint.id);
            let kept = secret.map(<[u8]>::to_vec).or(existing_secret);
            let mut stored = endpoint;
            stored.has_secret = kept.is_some();
            held.push((stored.clone(), kept));
            Ok(stored)
        }

        async fn get_webhook_endpoint(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
            Ok(self
                .webhook_endpoints
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e.id == id)
                .map(|(e, _)| e.clone()))
        }

        async fn get_webhook_endpoint_by_path(
            &self,
            path: &str,
        ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
            Ok(self
                .webhook_endpoints
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e.path == path)
                .map(|(e, _)| e.clone()))
        }

        async fn list_webhook_endpoints(
            &self,
        ) -> Result<Vec<graph_owl_storage::WebhookEndpoint>, StorageError> {
            Ok(self
                .webhook_endpoints
                .lock()
                .unwrap()
                .iter()
                .map(|(e, _)| e.clone())
                .collect())
        }

        async fn webhook_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self
                .webhook_endpoints
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e.id == id)
                .and_then(|(_, s)| s.clone()))
        }

        async fn upsert_stream_subscription(
            &self,
            subscription: graph_owl_storage::StreamSubscription,
            secret: Option<&[u8]>,
        ) -> Result<graph_owl_storage::StreamSubscription, StorageError> {
            let mut held = self.stream_subscriptions.lock().unwrap();
            if held.iter().any(|(s, _)| {
                s.topic == subscription.topic
                    && s.consumer_group == subscription.consumer_group
                    && s.id != subscription.id
            }) {
                return Err(StorageError::Conflict {
                    detail: format!(
                        "topic '{}' with consumer group '{}' is already registered",
                        subscription.topic, subscription.consumer_group
                    ),
                    existing_id: None,
                    kind: ConflictKind::StreamSubscriptionExists,
                });
            }
            let existing_secret = held
                .iter()
                .find(|(s, _)| s.id == subscription.id)
                .and_then(|(_, s)| s.clone());
            held.retain(|(s, _)| s.id != subscription.id);
            let kept = secret.map(<[u8]>::to_vec).or(existing_secret);
            let mut stored = subscription;
            stored.has_secret = kept.is_some();
            held.push((stored.clone(), kept));
            Ok(stored)
        }

        async fn get_stream_subscription(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::StreamSubscription>, StorageError> {
            Ok(self
                .stream_subscriptions
                .lock()
                .unwrap()
                .iter()
                .find(|(s, _)| s.id == id)
                .map(|(s, _)| s.clone()))
        }

        async fn list_stream_subscriptions(
            &self,
        ) -> Result<Vec<graph_owl_storage::StreamSubscription>, StorageError> {
            Ok(self
                .stream_subscriptions
                .lock()
                .unwrap()
                .iter()
                .map(|(s, _)| s.clone())
                .collect())
        }

        async fn stream_subscription_secret(
            &self,
            id: Uuid,
        ) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self
                .stream_subscriptions
                .lock()
                .unwrap()
                .iter()
                .find(|(s, _)| s.id == id)
                .and_then(|(_, s)| s.clone()))
        }

        async fn create_stream_dead_letter(
            &self,
            letter: graph_owl_storage::StreamDeadLetter,
        ) -> Result<graph_owl_storage::StreamDeadLetter, StorageError> {
            self.stream_dead_letters
                .lock()
                .unwrap()
                .push(letter.clone());
            Ok(letter)
        }

        async fn get_stream_dead_letter(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::StreamDeadLetter>, StorageError> {
            Ok(self
                .stream_dead_letters
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.id == id)
                .cloned())
        }

        async fn list_stream_dead_letters(
            &self,
            subscription: Option<Uuid>,
        ) -> Result<Vec<graph_owl_storage::StreamDeadLetter>, StorageError> {
            let mut letters: Vec<_> = self
                .stream_dead_letters
                .lock()
                .unwrap()
                .iter()
                .filter(|l| subscription.is_none_or(|s| l.subscription_id == s))
                .cloned()
                .collect();
            letters.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            Ok(letters)
        }

        async fn delete_stream_dead_letter(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut held = self.stream_dead_letters.lock().unwrap();
            let before = held.len();
            held.retain(|l| l.id != id);
            Ok(held.len() < before)
        }

        async fn create_inbound_event(
            &self,
            mut event: graph_owl_core::webhook::InboundEvent,
        ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
            let mut held = self.inbound_events.lock().unwrap();
            // Mirrors the Postgres impl's dedup marker: the lock held across
            // the check and the push is what makes this atomic for a single
            // process, the same way `(endpoint_id, dedup_key)`'s primary key
            // makes it atomic across connections.
            if held
                .iter()
                .any(|e| e.endpoint == event.endpoint && e.dedup_key == event.dedup_key)
            {
                event.state = graph_owl_core::webhook::EventState::Duplicate;
            }
            held.push(event.clone());
            Ok(event)
        }

        async fn get_inbound_event(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_core::webhook::InboundEvent>, StorageError> {
            Ok(self
                .inbound_events
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned())
        }

        async fn update_inbound_event_state(
            &self,
            id: Uuid,
            state: graph_owl_core::webhook::EventState,
            reason: Option<&str>,
        ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
            let mut held = self.inbound_events.lock().unwrap();
            let event = held
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| StorageError::Unexpected(format!("no inbound event {id}")))?;
            event.state = state;
            event.reason = reason.map(str::to_string);
            Ok(event.clone())
        }

        async fn list_dead_letters(
            &self,
            filter: &graph_owl_storage::DeadLetterFilter,
        ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
            let mut matching: Vec<_> = self
                .inbound_events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.state == graph_owl_core::webhook::EventState::Failed)
                .filter(|e| {
                    filter
                        .endpoint
                        .is_none_or(|endpoint| e.endpoint == endpoint)
                })
                .filter(|e| {
                    filter.reason_contains.as_ref().is_none_or(|needle| {
                        e.reason
                            .as_ref()
                            .is_some_and(|reason| reason.contains(needle.as_str()))
                    })
                })
                .cloned()
                .collect();
            matching.sort_by_key(|e| std::cmp::Reverse(e.received_at));
            Ok(matching
                .into_iter()
                .skip(filter.offset)
                .take(filter.limit)
                .collect())
        }

        async fn list_inbound_events_in_window(
            &self,
            endpoint: Uuid,
            since: chrono::DateTime<chrono::Utc>,
            until: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
            let mut matching: Vec<_> = self
                .inbound_events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| {
                    e.endpoint == endpoint && e.received_at >= since && e.received_at <= until
                })
                .cloned()
                .collect();
            matching.sort_by_key(|e| e.sender_timestamp.unwrap_or(e.received_at));
            Ok(matching)
        }

        async fn purge_dead_letters(
            &self,
            older_than: chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StorageError> {
            let mut held = self.inbound_events.lock().unwrap();
            let before = held.len();
            held.retain(|e| {
                !(e.state == graph_owl_core::webhook::EventState::Failed
                    && e.received_at < older_than)
            });
            Ok((before - held.len()) as u64)
        }

        async fn last_applied_timestamp(
            &self,
            fully_qualified_name: &str,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
            Ok(self
                .entity_last_applied
                .lock()
                .unwrap()
                .get(fully_qualified_name)
                .copied())
        }

        async fn record_applied_timestamp(
            &self,
            fully_qualified_name: &str,
            sender_timestamp: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), StorageError> {
            self.entity_last_applied
                .lock()
                .unwrap()
                .insert(fully_qualified_name.to_string(), sender_timestamp);
            Ok(())
        }

        async fn upsert_mapping(
            &self,
            mut mapping: graph_owl_storage::Mapping,
        ) -> Result<graph_owl_storage::Mapping, StorageError> {
            self.guard_write("upsert_mapping");
            let mut held = self.mapping_versions.lock().unwrap();
            mapping.version = held
                .iter()
                .filter(|m| m.name == mapping.name)
                .map(|m| m.version)
                .max()
                .unwrap_or(0)
                + 1;
            mapping.created_at = chrono::Utc::now();
            held.push(mapping.clone());
            Ok(mapping)
        }

        async fn get_mapping(
            &self,
            name: &str,
        ) -> Result<Option<graph_owl_storage::Mapping>, StorageError> {
            Ok(self
                .mapping_versions
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.name == name)
                .max_by_key(|m| m.version)
                .cloned())
        }

        async fn list_mapping_versions(
            &self,
            name: &str,
        ) -> Result<Vec<graph_owl_storage::Mapping>, StorageError> {
            let mut versions: Vec<_> = self
                .mapping_versions
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.name == name)
                .cloned()
                .collect();
            versions.sort_by_key(|m| std::cmp::Reverse(m.version));
            Ok(versions)
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

        async fn resolution_candidates(&self, asset_id: Uuid) -> Result<Vec<Asset>, StorageError> {
            let assets = self.assets.lock().unwrap();
            let Some(target) = assets.iter().find(|a| a.id == asset_id) else {
                return Ok(Vec::new());
            };
            let target_keys = asset_blocking_key_values(target, &assets);
            Ok(assets
                .iter()
                .filter(|a| a.id != asset_id && !a.deleted)
                .filter(|a| {
                    let keys = asset_blocking_key_values(a, &assets);
                    target_keys
                        .iter()
                        .any(|k| !k.is_empty() && keys.contains(k))
                })
                .cloned()
                .collect())
        }

        async fn create_merge_record(
            &self,
            record: graph_owl_core::resolution::MergeRecord,
        ) -> Result<graph_owl_core::resolution::MergeRecord, StorageError> {
            self.guard_write("create_merge_record");
            self.merge_records.lock().unwrap().push(record.clone());
            Ok(record)
        }

        async fn get_merge_record(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_core::resolution::MergeRecord>, StorageError> {
            Ok(self
                .merge_records
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id == id)
                .cloned())
        }

        async fn split_merge_record(
            &self,
            id: Uuid,
            split_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<SplitOutcome, StorageError> {
            self.guard_write("split_merge_record");
            let mut records = self.merge_records.lock().unwrap();
            let Some(record) = records.iter_mut().find(|r| r.id == id) else {
                return Ok(SplitOutcome::NotFound);
            };
            if let Some(already) = record.split_at {
                return Ok(SplitOutcome::AlreadySplit { split_at: already });
            }
            record.split_at = Some(split_at);
            Ok(SplitOutcome::Split(Box::new(record.clone())))
        }

        async fn most_recent_split_between(
            &self,
            a: Uuid,
            b: Uuid,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
            Ok(self
                .merge_records
                .lock()
                .unwrap()
                .iter()
                .filter(|r| {
                    (r.canonical == a && r.merged == b) || (r.canonical == b && r.merged == a)
                })
                .filter_map(|r| r.split_at)
                .max())
        }

        async fn queue_for_review(
            &self,
            entry: graph_owl_core::resolution::ReviewQueueEntry,
        ) -> Result<graph_owl_core::resolution::ReviewQueueEntry, StorageError> {
            let mut queue = self.resolution_queue.lock().unwrap();
            if let Some(existing) = queue
                .iter()
                .find(|e| e.target == entry.target && e.candidate == entry.candidate)
            {
                return Ok(existing.clone());
            }
            queue.push(entry.clone());
            Ok(entry)
        }

        async fn list_review_queue(
            &self,
            filter: &ReviewQueueFilter,
        ) -> Result<(Vec<graph_owl_core::resolution::ReviewQueueEntry>, i64), StorageError>
        {
            use graph_owl_core::resolution::ReviewStatus;
            let assets = self.assets.lock().unwrap();
            let status = filter.status.unwrap_or(ReviewStatus::Pending);
            let matching: Vec<_> = self
                .resolution_queue
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.status == status)
                .filter(|e| {
                    filter.kind.is_none_or(|kind| {
                        assets
                            .iter()
                            .find(|a| a.id == e.target)
                            .is_some_and(|a| a.kind == kind)
                    })
                })
                .filter(|e| filter.min_score.is_none_or(|min| e.score >= min))
                .filter(|e| filter.max_score.is_none_or(|max| e.score <= max))
                .cloned()
                .collect();
            let mut sorted = matching;
            sorted.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
            let total = i64::try_from(sorted.len()).unwrap_or(i64::MAX);
            let page = sorted
                .into_iter()
                .skip(filter.offset)
                .take(filter.limit)
                .collect();
            Ok((page, total))
        }

        async fn get_review_queue_entry(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
            Ok(self
                .resolution_queue
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned())
        }

        async fn decide_review_queue_entry(
            &self,
            id: Uuid,
            status: graph_owl_core::resolution::ReviewStatus,
            decided_by: graph_owl_core::resolution::MergeDecidedBy,
            decided_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
            use graph_owl_core::resolution::ReviewStatus;
            let mut queue = self.resolution_queue.lock().unwrap();
            let Some(entry) = queue.iter_mut().find(|e| e.id == id) else {
                return Ok(None);
            };
            if entry.status != ReviewStatus::Pending {
                return Ok(Some(entry.clone()));
            }
            entry.status = status;
            entry.decided_by = Some(decided_by);
            entry.decided_at = Some(decided_at);
            Ok(Some(entry.clone()))
        }

        async fn record_mention_resolution(
            &self,
            resolution: graph_owl_core::resolution::MentionResolution,
        ) -> Result<graph_owl_core::resolution::MentionResolution, StorageError> {
            self.mention_resolutions
                .lock()
                .unwrap()
                .push(resolution.clone());
            Ok(resolution)
        }

        async fn mention_resolutions_for_source(
            &self,
            source: Uuid,
        ) -> Result<Vec<graph_owl_core::resolution::MentionResolution>, StorageError> {
            let mut found: Vec<_> = self
                .mention_resolutions
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.source == source)
                .cloned()
                .collect();
            found.sort_by(|a, b| b.resolved_at.cmp(&a.resolved_at));
            Ok(found)
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
            versions.sort_by_key(|version| std::cmp::Reverse(version.version));
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
            runs.sort_by_key(|run| std::cmp::Reverse(run.started_at));
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
                    if let Some((_, found)) = owners.iter().find(|(id, _)| *id == current)
                        && !found.is_empty()
                    {
                        // Stops at the nearest owned ancestor rather than
                        // accumulating up the chain: "who do I ask" has one
                        // answer.
                        return found.iter().map(|o| o.id.clone()).collect();
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

        // ---- Epic 24 Slice A: glossary and terms ----

        async fn insert_glossary(
            &self,
            glossary: graph_owl_storage::Glossary,
        ) -> Result<graph_owl_storage::Glossary, StorageError> {
            let mut held = self.glossaries.lock().unwrap();
            if held
                .iter()
                .any(|g| g.fully_qualified_name == glossary.fully_qualified_name)
            {
                return Err(StorageError::Conflict {
                    detail: glossary.fully_qualified_name.clone(),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                });
            }
            held.push(glossary.clone());
            Ok(glossary)
        }

        async fn get_glossary(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::Glossary>, StorageError> {
            Ok(self
                .glossaries
                .lock()
                .unwrap()
                .iter()
                .find(|g| g.id == id)
                .cloned())
        }

        async fn list_glossaries(&self) -> Result<Vec<graph_owl_storage::Glossary>, StorageError> {
            let mut glossaries = self.glossaries.lock().unwrap().clone();
            glossaries.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
            Ok(glossaries)
        }

        async fn delete_glossary(
            &self,
            id: Uuid,
            recursive: bool,
        ) -> Result<graph_owl_storage::GlossaryDeletion, StorageError> {
            let mut glossaries = self.glossaries.lock().unwrap();
            if !glossaries.iter().any(|g| g.id == id) {
                return Ok(graph_owl_storage::GlossaryDeletion::NotFound);
            }
            let mut terms = self.glossary_terms.lock().unwrap();
            let term_count = i64::try_from(terms.iter().filter(|t| t.glossary_id == id).count())
                .unwrap_or(i64::MAX);
            if term_count > 0 && !recursive {
                return Ok(graph_owl_storage::GlossaryDeletion::HasTerms { term_count });
            }
            terms.retain(|t| t.glossary_id != id);
            glossaries.retain(|g| g.id != id);
            Ok(graph_owl_storage::GlossaryDeletion::Deleted)
        }

        async fn insert_term(
            &self,
            term: graph_owl_storage::GlossaryTermRecord,
        ) -> Result<graph_owl_storage::GlossaryTermRecord, StorageError> {
            let mut held = self.glossary_terms.lock().unwrap();
            if held
                .iter()
                .any(|t| t.fully_qualified_name == term.fully_qualified_name)
            {
                return Err(StorageError::Conflict {
                    detail: term.fully_qualified_name.clone(),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                });
            }
            held.push(term.clone());
            Ok(term)
        }

        async fn get_term(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
            Ok(self
                .glossary_terms
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn list_terms(
            &self,
            glossary_id: Uuid,
        ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
            let mut terms: Vec<_> = self
                .glossary_terms
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.glossary_id == glossary_id)
                .cloned()
                .collect();
            terms.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(terms)
        }

        async fn update_term(
            &self,
            id: Uuid,
            update: graph_owl_storage::GlossaryTermUpdate,
        ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
            let mut terms = self.glossary_terms.lock().unwrap();
            let Some(term) = terms.iter_mut().find(|t| t.id == id) else {
                return Ok(None);
            };
            if let Some(definition) = update.definition {
                term.definition = definition;
            }
            if let Some(synonyms) = update.synonyms {
                term.synonyms = synonyms;
            }
            if let Some(abbreviations) = update.abbreviations {
                term.abbreviations = abbreviations;
            }
            term.updated_at = Utc::now();
            Ok(Some(term.clone()))
        }

        async fn delete_term(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut terms = self.glossary_terms.lock().unwrap();
            let original_len = terms.len();
            terms.retain(|t| t.id != id);
            Ok(terms.len() != original_len)
        }

        async fn search_terms(
            &self,
            query: &str,
        ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
            // A substring match over the same fields the migration's
            // `search_vector` indexes. Not ranked the way Postgres's
            // `ts_rank_cd` is — this fake only has to prove the facade wires
            // the query through, not reproduce full-text relevance.
            let needle = query.to_lowercase();
            Ok(self
                .glossary_terms
                .lock()
                .unwrap()
                .iter()
                .filter(|t| {
                    t.name.to_lowercase().contains(&needle)
                        || t.definition.to_lowercase().contains(&needle)
                        || t.synonyms
                            .iter()
                            .any(|s| s.to_lowercase().contains(&needle))
                        || t.abbreviations
                            .iter()
                            .any(|a| a.to_lowercase().contains(&needle))
                })
                .cloned()
                .collect())
        }

        // ---- Epic 24 Slice B: SKOS relations ----

        async fn insert_term_relation(
            &self,
            term_id: Uuid,
            relation: graph_owl_core::glossary::SkosRelation,
        ) -> Result<(), StorageError> {
            let mut held = self.term_relations.lock().unwrap();
            if !held.iter().any(|(id, r)| *id == term_id && *r == relation) {
                held.push((term_id, relation));
            }
            Ok(())
        }

        async fn delete_term_relation(
            &self,
            term_id: Uuid,
            relation: &graph_owl_core::glossary::SkosRelation,
        ) -> Result<bool, StorageError> {
            let mut held = self.term_relations.lock().unwrap();
            let original_len = held.len();
            held.retain(|(id, r)| !(*id == term_id && r == relation));
            Ok(held.len() != original_len)
        }

        async fn term_relations_touching(
            &self,
            term_id: Uuid,
        ) -> Result<Vec<(String, graph_owl_core::glossary::SkosRelation)>, StorageError> {
            use graph_owl_core::glossary::SkosRelation;
            let id_text = term_id.to_string();
            Ok(self
                .term_relations
                .lock()
                .unwrap()
                .iter()
                .filter(|(owner, relation)| *owner == term_id || relation.target() == id_text)
                .map(|(owner, relation): &(Uuid, SkosRelation)| {
                    (owner.to_string(), relation.clone())
                })
                .collect())
        }

        async fn broader_edges(&self) -> Result<Vec<(String, String)>, StorageError> {
            use graph_owl_core::glossary::SkosRelation;
            Ok(self
                .term_relations
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(owner, relation)| match relation {
                    SkosRelation::Broader(target) => Some((owner.to_string(), target.clone())),
                    _ => None,
                })
                .collect())
        }

        // ---- Epic 24 Slice C: review workflow ----

        async fn set_term_reviewers(
            &self,
            term_id: Uuid,
            reviewers: &[String],
        ) -> Result<(), StorageError> {
            let mut held = self.term_reviewers.lock().unwrap();
            held.retain(|(id, _)| *id != term_id);
            held.push((term_id, reviewers.to_vec()));
            Ok(())
        }

        async fn term_reviewers(&self, term_id: Uuid) -> Result<Vec<String>, StorageError> {
            Ok(self
                .term_reviewers
                .lock()
                .unwrap()
                .iter()
                .find(|(id, _)| *id == term_id)
                .map(|(_, reviewers)| reviewers.clone())
                .unwrap_or_default())
        }

        async fn transition_term(
            &self,
            term_id: Uuid,
            _from: graph_owl_core::glossary::TermStatus,
            to: graph_owl_core::glossary::TermStatus,
            _actor: &str,
            _reason: Option<String>,
            _successor_term_id: Option<Uuid>,
        ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
            let mut terms = self.glossary_terms.lock().unwrap();
            let Some(term) = terms.iter_mut().find(|t| t.id == term_id) else {
                return Ok(None);
            };
            term.status = to;
            term.version.minor += 1;
            term.updated_at = Utc::now();
            Ok(Some(term.clone()))
        }

        // ---- Epic 24 Slice D: terms attach to assets and columns ----

        async fn attach_term(
            &self,
            term_id: Uuid,
            target_fqn: &str,
            _attached_by: &str,
        ) -> Result<(), StorageError> {
            let mut held = self.term_attachments.lock().unwrap();
            if !held
                .iter()
                .any(|(id, fqn)| *id == term_id && fqn == target_fqn)
            {
                held.push((term_id, target_fqn.to_string()));
            }
            Ok(())
        }

        async fn detach_term(&self, term_id: Uuid, target_fqn: &str) -> Result<bool, StorageError> {
            let mut held = self.term_attachments.lock().unwrap();
            let original_len = held.len();
            held.retain(|(id, fqn)| !(*id == term_id && fqn == target_fqn));
            Ok(held.len() != original_len)
        }

        async fn term_usage(
            &self,
            term_id: Uuid,
            page: &PageRequest,
        ) -> Result<Page<String>, StorageError> {
            let mut fqns: Vec<String> = self
                .term_attachments
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _)| *id == term_id)
                .map(|(_, fqn)| fqn.clone())
                .collect();
            fqns.sort();
            if let Some(cursor) = &page.after {
                fqns.retain(|fqn| fqn.as_str() > cursor.sort_key.as_str());
            }
            fqns.truncate(page.limit + 1);
            Ok(Page::from_overfetch(fqns, page.limit, |fqn: &String| {
                Cursor::new(fqn.clone(), term_id)
            }))
        }

        // ---- Epic 24 Slice E: Metric as a first-class entity ----

        async fn insert_metric(
            &self,
            metric: graph_owl_storage::MetricRecord,
        ) -> Result<graph_owl_storage::MetricRecord, StorageError> {
            let mut held = self.metrics.lock().unwrap();
            if held
                .iter()
                .any(|m| m.fully_qualified_name == metric.fully_qualified_name)
            {
                return Err(StorageError::Conflict {
                    detail: metric.fully_qualified_name.clone(),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                });
            }
            held.push(metric.clone());
            Ok(metric)
        }

        async fn get_metric(
            &self,
            id: Uuid,
        ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
            Ok(self
                .metrics
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id == id)
                .cloned())
        }

        async fn list_metrics(
            &self,
            page: &PageRequest,
        ) -> Result<Page<graph_owl_storage::MetricRecord>, StorageError> {
            let mut metrics = self.metrics.lock().unwrap().clone();
            metrics.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                metrics.retain(|m| {
                    (m.fully_qualified_name.as_str(), m.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            metrics.truncate(page.limit + 1);
            Ok(Page::from_overfetch(metrics, page.limit, |m| {
                Cursor::new(m.fully_qualified_name.clone(), m.id)
            }))
        }

        async fn update_metric(
            &self,
            id: Uuid,
            update: graph_owl_storage::MetricUpdate,
        ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
            let mut metrics = self.metrics.lock().unwrap();
            let Some(metric) = metrics.iter_mut().find(|m| m.id == id) else {
                return Ok(None);
            };
            if let Some(definition) = update.definition {
                metric.definition = definition;
            }
            if let Some(formula) = update.formula {
                metric.formula = Some(formula);
            }
            if let Some(unit) = update.unit {
                metric.unit = Some(unit);
            }
            if let Some(granularity) = update.granularity {
                metric.granularity = Some(granularity);
            }
            if let Some(calculation_type) = update.calculation_type {
                metric.calculation_type = calculation_type;
            }
            metric.updated_at = Utc::now();
            Ok(Some(metric.clone()))
        }

        async fn delete_metric(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut metrics = self.metrics.lock().unwrap();
            let original_len = metrics.len();
            metrics.retain(|m| m.id != id);
            Ok(metrics.len() != original_len)
        }

        async fn search_metrics(
            &self,
            query: &str,
        ) -> Result<Vec<graph_owl_storage::MetricRecord>, StorageError> {
            let needle = query.to_lowercase();
            let terms = self.glossary_terms.lock().unwrap();
            Ok(self
                .metrics
                .lock()
                .unwrap()
                .iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&needle)
                        || m.definition.to_lowercase().contains(&needle)
                        || m.defined_by.is_some_and(|term_id| {
                            terms
                                .iter()
                                .any(|t| t.id == term_id && t.name.to_lowercase().contains(&needle))
                        })
                })
                .cloned()
                .collect())
        }

        // ---- Epic 24 Slice F: metric lineage reconciliation ----

        async fn update_metric_sources(
            &self,
            metric_id: Uuid,
            sources: &[String],
        ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
            let mut metrics = self.metrics.lock().unwrap();
            let Some(metric) = metrics.iter_mut().find(|m| m.id == metric_id) else {
                return Ok(None);
            };
            metric.source_assets = sources.to_vec();
            metric.updated_at = Utc::now();
            Ok(Some(metric.clone()))
        }

        // ---- Epic 21, and again as strict as the port ----
        //
        // In particular the cascade is real here: deleting a run drops its
        // claims and discards, because "a bad run is deletable wholesale" is
        // the property the whole named-graph decision buys, and a double that
        // left orphans behind would let a broken cascade pass every test that
        // does not use Postgres.

        async fn find_extraction_run(
            &self,
            source_id: &str,
            fingerprint: &str,
            extractor: &str,
            version: &str,
        ) -> Result<Option<graph_owl_storage::ExtractionRunRecord>, StorageError> {
            let runs = self.extraction_runs.lock().unwrap();
            Ok(runs
                .iter()
                .find(|run| {
                    run.source_id == source_id
                        && run.source_fingerprint == fingerprint
                        && run.extractor == extractor
                        && run.extractor_version == version
                })
                .cloned())
        }

        async fn find_extraction_run_by_id(
            &self,
            run_id: Uuid,
        ) -> Result<Option<graph_owl_storage::ExtractionRunRecord>, StorageError> {
            let runs = self.extraction_runs.lock().unwrap();
            Ok(runs.iter().find(|run| run.id == run_id).cloned())
        }

        async fn save_extraction_run(
            &self,
            run: &graph_owl_storage::ExtractionRunRecord,
            queued: &[graph_owl_storage::QueuedClaimRecord],
            discarded: &[graph_owl_storage::DiscardedClaimRecord],
        ) -> Result<(), StorageError> {
            self.guard_write("save_extraction_run");
            self.extraction_runs.lock().unwrap().push(run.clone());
            self.extraction_claims
                .lock()
                .unwrap()
                .extend(queued.iter().cloned());
            self.extraction_discards
                .lock()
                .unwrap()
                .extend(discarded.iter().cloned());
            Ok(())
        }

        async fn pending_extraction_claims(
            &self,
            limit: i64,
        ) -> Result<Vec<graph_owl_storage::QueuedClaimRecord>, StorageError> {
            let claims = self.extraction_claims.lock().unwrap();
            Ok(claims
                .iter()
                .filter(|claim| claim.state == "pending")
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .cloned()
                .collect())
        }

        async fn decide_extraction_claim(
            &self,
            claim_id: Uuid,
            confirmed: bool,
            decided_by: &str,
        ) -> Result<Option<graph_owl_storage::QueuedClaimRecord>, StorageError> {
            let mut claims = self.extraction_claims.lock().unwrap();
            let Some(claim) = claims.iter_mut().find(|claim| claim.id == claim_id) else {
                return Ok(None);
            };
            claim.state = if confirmed { "confirmed" } else { "rejected" }.to_string();
            claim.decided_by = Some(decided_by.to_string());
            Ok(Some(claim.clone()))
        }

        async fn rejected_assertions(&self) -> Result<Vec<(String, String, String)>, StorageError> {
            let claims = self.extraction_claims.lock().unwrap();
            Ok(claims
                .iter()
                .filter(|claim| claim.state == "rejected")
                .map(|claim| {
                    (
                        claim.subject.clone(),
                        claim.predicate.clone(),
                        claim.object.clone(),
                    )
                })
                .collect())
        }

        async fn delete_extraction_run(&self, run_id: Uuid) -> Result<bool, StorageError> {
            let mut runs = self.extraction_runs.lock().unwrap();
            let before = runs.len();
            runs.retain(|run| run.id != run_id);
            self.extraction_claims
                .lock()
                .unwrap()
                .retain(|claim| claim.run_id != run_id);
            self.extraction_discards
                .lock()
                .unwrap()
                .retain(|discard| discard.run_id != run_id);
            Ok(runs.len() < before)
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

    // ---- Epic 24 Slice A: glossary and terms ----

    #[tokio::test]
    async fn a_glossarys_fqn_is_derived_from_its_name() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let created = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");

        assert_eq!(created.fully_qualified_name, "Finance");
    }

    #[tokio::test]
    async fn a_created_glossary_can_be_fetched_by_id() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");

        let found = catalog
            .get_glossary(created.id)
            .await
            .expect("get_glossary should succeed");

        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn every_created_glossary_is_listed() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_glossary("Support", None)
            .await
            .expect("create_glossary should succeed");

        let listed = catalog
            .list_glossaries()
            .await
            .expect("list_glossaries should succeed");

        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn a_glossary_needs_a_name() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .create_glossary("   ", None)
            .await
            .expect_err("a blank name should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn a_terms_fqn_nests_under_its_glossary() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");

        let term = catalog
            .create_term(
                finance.id,
                "Customer",
                "a paying party".to_string(),
                vec![],
                vec![],
            )
            .await
            .expect("create_term should succeed");

        assert_eq!(term.fully_qualified_name, "Finance.Customer");
        assert_eq!(term.status, graph_owl_core::glossary::TermStatus::Draft);
    }

    #[tokio::test]
    async fn a_created_term_can_be_fetched_by_id() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let created = catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let found = catalog
            .get_term(created.id)
            .await
            .expect("get_term should succeed");

        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn every_term_in_a_glossary_is_listed() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .create_term(finance.id, "Revenue", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let listed = catalog
            .list_terms(finance.id)
            .await
            .expect("list_terms should succeed");

        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn a_term_under_an_unknown_glossary_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .create_term(Uuid::new_v4(), "Customer", String::new(), vec![], vec![])
            .await
            .expect_err("an unknown glossary should refuse the term");

        assert!(matches!(error, CatalogError::NotFound));
    }

    // **The scoped-uniqueness pair the plan names**: the same term name in two
    // different glossaries must both succeed, because the FQN each one derives
    // to is different even though `name` is not.
    #[tokio::test]
    async fn the_same_term_name_in_two_glossaries_both_succeed() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let support = catalog
            .create_glossary("Support", None)
            .await
            .expect("create_glossary should succeed");

        let first = catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("the first glossary's term should succeed");
        let second = catalog
            .create_term(support.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("the second glossary's term should succeed");

        assert_eq!(first.fully_qualified_name, "Finance.Customer");
        assert_eq!(second.fully_qualified_name, "Support.Customer");
    }

    // And the negative: within one glossary the same name collides.
    #[tokio::test]
    async fn the_same_term_name_twice_in_one_glossary_conflicts() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("the first term should succeed");

        let error = catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect_err("a duplicate name in the same glossary should conflict");

        assert!(matches!(error, CatalogError::Conflict { .. }));
    }

    #[tokio::test]
    async fn deleting_a_glossary_with_terms_is_refused_without_recursive() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .delete_glossary(finance.id, false)
            .await
            .expect_err("a glossary with terms should refuse a non-recursive delete");

        assert!(matches!(
            error,
            CatalogError::Conflict {
                kind: ConflictKind::GlossaryHasTerms,
                ..
            }
        ));
    }

    // The positive half: the same request, recursively, succeeds and takes the
    // term with it. Written beside the refusal above because an unconditional
    // "glossary has terms" check would pass the test above and fail only here.
    #[tokio::test]
    async fn deleting_a_glossary_recursively_takes_its_terms_with_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        catalog
            .delete_glossary(finance.id, true)
            .await
            .expect("a recursive delete should succeed");

        assert_eq!(
            catalog.get_term(term.id).await.expect("get_term"),
            None,
            "the term should be gone with its glossary"
        );
    }

    #[tokio::test]
    async fn deleting_an_unknown_glossary_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .delete_glossary(Uuid::new_v4(), false)
            .await
            .expect_err("an unknown glossary should not be found");

        assert!(matches!(error, CatalogError::NotFound));
    }

    #[tokio::test]
    async fn updating_a_term_changes_only_the_provided_fields() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let created = catalog
            .create_term(
                finance.id,
                "Customer",
                "original".to_string(),
                vec!["client".to_string()],
                vec![],
            )
            .await
            .expect("create_term should succeed");

        let updated = catalog
            .update_term(
                created.id,
                graph_owl_storage::GlossaryTermUpdate {
                    definition: Some("revised".to_string()),
                    synonyms: None,
                    abbreviations: None,
                },
            )
            .await
            .expect("update_term should succeed");

        assert_eq!(updated.definition, "revised");
        assert_eq!(updated.synonyms, vec!["client".to_string()]);
    }

    #[tokio::test]
    async fn deleting_a_term_removes_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let created = catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        catalog
            .delete_term(created.id)
            .await
            .expect("delete_term should succeed");

        assert_eq!(catalog.get_term(created.id).await.expect("get_term"), None);
    }

    #[tokio::test]
    async fn a_synonym_match_finds_the_term() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_term(
                finance.id,
                "Customer",
                String::new(),
                vec!["client".to_string()],
                vec![],
            )
            .await
            .expect("create_term should succeed");

        let hits = catalog
            .search_terms("client")
            .await
            .expect("search_terms should succeed");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Customer");
    }

    // The negative half: an unrelated word must not match, or a search that
    // returns everything would pass the positive test above too.
    #[tokio::test]
    async fn an_unrelated_word_does_not_match() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let finance = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        catalog
            .create_term(finance.id, "Customer", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let hits = catalog
            .search_terms("zzzznomatch")
            .await
            .expect("search_terms should succeed");

        assert!(hits.is_empty());
    }

    // ---- Epic 24 Slice B: SKOS relations ----

    async fn glossary_and_two_terms(
        catalog: &Catalog,
    ) -> (
        graph_owl_storage::GlossaryTermRecord,
        graph_owl_storage::GlossaryTermRecord,
    ) {
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let child = catalog
            .create_term(
                glossary.id,
                "Checking Account",
                String::new(),
                vec![],
                vec![],
            )
            .await
            .expect("create_term should succeed");
        let parent = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        (child, parent)
    }

    #[tokio::test]
    async fn broader_implies_narrower_without_a_second_stored_edge() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let (child, parent) = glossary_and_two_terms(&catalog).await;

        catalog
            .add_term_relation(
                child.id,
                graph_owl_core::glossary::SkosRelation::Broader(parent.id.to_string()),
            )
            .await
            .expect("add_term_relation should succeed");

        let on_parent = catalog
            .term_relations(parent.id)
            .await
            .expect("term_relations should succeed");

        assert_eq!(
            on_parent,
            vec![graph_owl_core::glossary::SkosRelation::Narrower(
                child.id.to_string()
            )]
        );
    }

    // Asserting `narrower` directly would be the second stored edge for the
    // same fact the test above proves is derived — refused structurally.
    #[tokio::test]
    async fn narrower_cannot_be_asserted_directly() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let (child, parent) = glossary_and_two_terms(&catalog).await;

        let error = catalog
            .add_term_relation(
                parent.id,
                graph_owl_core::glossary::SkosRelation::Narrower(child.id.to_string()),
            )
            .await
            .expect_err("narrower should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn a_term_cannot_be_its_own_broader() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .add_term_relation(
                term.id,
                graph_owl_core::glossary::SkosRelation::Broader(term.id.to_string()),
            )
            .await
            .expect_err("a self-loop should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    // Depth 3, because a check that compares only the immediate parent
    // passes depth 1 and fails here.
    #[tokio::test]
    async fn a_three_term_cycle_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let a = catalog
            .create_term(glossary.id, "A", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let b = catalog
            .create_term(glossary.id, "B", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let c = catalog
            .create_term(glossary.id, "C", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .add_term_relation(
                a.id,
                graph_owl_core::glossary::SkosRelation::Broader(b.id.to_string()),
            )
            .await
            .expect("a broader b should succeed");
        catalog
            .add_term_relation(
                b.id,
                graph_owl_core::glossary::SkosRelation::Broader(c.id.to_string()),
            )
            .await
            .expect("b broader c should succeed");

        let error = catalog
            .add_term_relation(
                c.id,
                graph_owl_core::glossary::SkosRelation::Broader(a.id.to_string()),
            )
            .await
            .expect_err("closing the loop should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    // **Poly-hierarchy is legitimate SKOS** — the negative beside the cycle
    // tests above, so a checker that refused every second `broader` would
    // fail here rather than only passing the cycle cases.
    #[tokio::test]
    async fn a_term_may_have_more_than_one_broader_parent() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let child = catalog
            .create_term(
                glossary.id,
                "Savings Account",
                String::new(),
                vec![],
                vec![],
            )
            .await
            .expect("create_term should succeed");
        let first_parent = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let second_parent = catalog
            .create_term(
                glossary.id,
                "Financial Product",
                String::new(),
                vec![],
                vec![],
            )
            .await
            .expect("create_term should succeed");

        catalog
            .add_term_relation(
                child.id,
                graph_owl_core::glossary::SkosRelation::Broader(first_parent.id.to_string()),
            )
            .await
            .expect("the first parent should succeed");
        catalog
            .add_term_relation(
                child.id,
                graph_owl_core::glossary::SkosRelation::Broader(second_parent.id.to_string()),
            )
            .await
            .expect("the second parent should also succeed");

        let relations = catalog
            .term_relations(child.id)
            .await
            .expect("term_relations should succeed");
        assert_eq!(relations.len(), 2);
    }

    #[tokio::test]
    async fn related_is_symmetric_on_read() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let a = catalog
            .create_term(glossary.id, "A", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let b = catalog
            .create_term(glossary.id, "B", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        catalog
            .add_term_relation(
                a.id,
                graph_owl_core::glossary::SkosRelation::Related(b.id.to_string()),
            )
            .await
            .expect("add_term_relation should succeed");

        let on_b = catalog
            .term_relations(b.id)
            .await
            .expect("term_relations should succeed");

        assert_eq!(
            on_b,
            vec![graph_owl_core::glossary::SkosRelation::Related(
                a.id.to_string()
            )]
        );
    }

    // `exactMatch`/`closeMatch` point at an external IRI and are **not**
    // validated for reachability (decision 2) — an unresolvable-looking IRI
    // must still be accepted.
    #[tokio::test]
    async fn an_exact_match_to_an_external_iri_is_not_checked_for_reachability() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        catalog
            .add_term_relation(
                term.id,
                graph_owl_core::glossary::SkosRelation::ExactMatch(
                    "http://example.invalid/does-not-exist".to_string(),
                ),
            )
            .await
            .expect("an external IRI should not be checked for reachability");
    }

    #[tokio::test]
    async fn a_broader_target_that_is_not_a_known_term_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .add_term_relation(
                term.id,
                graph_owl_core::glossary::SkosRelation::Broader(Uuid::new_v4().to_string()),
            )
            .await
            .expect_err("an unknown target term should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn asserting_a_relation_on_an_unknown_term_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .add_term_relation(
                Uuid::new_v4(),
                graph_owl_core::glossary::SkosRelation::ExactMatch("http://x.example/1".into()),
            )
            .await
            .expect_err("an unknown term should be not found");

        assert!(matches!(error, CatalogError::NotFound));
    }

    #[tokio::test]
    async fn removing_a_relation_the_term_declared_deletes_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let (child, parent) = glossary_and_two_terms(&catalog).await;
        let relation = graph_owl_core::glossary::SkosRelation::Broader(parent.id.to_string());
        catalog
            .add_term_relation(child.id, relation.clone())
            .await
            .expect("add_term_relation should succeed");

        catalog
            .remove_term_relation(child.id, &relation)
            .await
            .expect("remove_term_relation should succeed");

        assert!(
            catalog
                .term_relations(child.id)
                .await
                .expect("term_relations")
                .is_empty()
        );
    }

    // The derived half is not a row: attempting to remove `narrower` from the
    // parent (which never stored it) must be `NotFound`, not a silent no-op
    // that happens to leave the graph looking right.
    #[tokio::test]
    async fn removing_a_derived_relation_that_was_never_stored_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let (child, parent) = glossary_and_two_terms(&catalog).await;
        catalog
            .add_term_relation(
                child.id,
                graph_owl_core::glossary::SkosRelation::Broader(parent.id.to_string()),
            )
            .await
            .expect("add_term_relation should succeed");

        let error = catalog
            .remove_term_relation(
                parent.id,
                &graph_owl_core::glossary::SkosRelation::Narrower(child.id.to_string()),
            )
            .await
            .expect_err("the derived inverse was never a row to delete");

        assert!(matches!(error, CatalogError::NotFound));
    }

    // ---- Epic 24 Slice C: review workflow ----

    async fn seed_user(catalog: &Catalog, id: &str) {
        catalog
            .storage
            .upsert_user(&graph_owl_storage::StoredUser {
                id: id.to_string(),
                display_name: id.to_string(),
                email: None,
                is_admin: false,
                is_bot: false,
                roles: vec![],
            })
            .await
            .expect("a user");
    }

    #[tokio::test]
    async fn a_term_walks_the_workflow_in_order() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        seed_user(&catalog, "alice").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");

        let in_review = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");
        assert_eq!(
            in_review.status,
            graph_owl_core::glossary::TermStatus::InReview
        );

        let approved = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect("in-review to approved should succeed");
        assert_eq!(
            approved.status,
            graph_owl_core::glossary::TermStatus::Approved
        );
    }

    // **The illegal move that matters.** Skipping review is the whole
    // mechanism a workflow exists to enforce.
    #[tokio::test]
    async fn a_term_cannot_skip_review() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect_err("draft to approved should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn approval_with_no_reviewer_assigned_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");

        let error = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect_err("approval with no reviewer assigned should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    // **Anyone approving their own term makes the reviewer list decoration**
    // — the negative that gives Slice C's `403` its meaning.
    #[tokio::test]
    async fn a_non_reviewer_cannot_approve() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        seed_user(&catalog, "alice").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "mallory",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");

        let error = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "mallory",
                None,
                None,
            )
            .await
            .expect_err("mallory was not assigned");

        assert!(matches!(error, CatalogError::Forbidden));
    }

    #[tokio::test]
    async fn each_transition_bumps_the_version() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let before = term.version;

        let after = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");

        assert!(after.version.minor > before.minor);
    }

    #[tokio::test]
    async fn deprecation_carries_a_reason_and_an_optional_successor() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        seed_user(&catalog, "alice").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let old_term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        let new_term = catalog
            .create_term(glossary.id, "Account V2", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(old_term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");
        catalog
            .transition_term(
                old_term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");
        catalog
            .transition_term(
                old_term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect("in-review to approved should succeed");

        let deprecated = catalog
            .transition_term(
                old_term.id,
                graph_owl_core::glossary::TermStatus::Deprecated,
                "alice",
                Some("superseded".to_string()),
                Some(new_term.id),
            )
            .await
            .expect("approved to deprecated should succeed");

        assert_eq!(
            deprecated.status,
            graph_owl_core::glossary::TermStatus::Deprecated
        );
    }

    #[tokio::test]
    async fn a_successor_that_is_not_a_known_term_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        seed_user(&catalog, "alice").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect("in-review to approved should succeed");

        let error = catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Deprecated,
                "alice",
                Some("superseded".to_string()),
                Some(Uuid::new_v4()),
            )
            .await
            .expect_err("an unknown successor should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn transitioning_an_unknown_term_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .transition_term(
                Uuid::new_v4(),
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect_err("an unknown term should be not found");

        assert!(matches!(error, CatalogError::NotFound));
    }

    #[tokio::test]
    async fn assigning_an_unknown_user_as_reviewer_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .set_term_reviewers(term.id, vec!["nobody".to_string()])
            .await
            .expect_err("an unknown reviewer should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn setting_reviewers_replaces_rather_than_merges() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        seed_user(&catalog, "alice").await;
        seed_user(&catalog, "bob").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");

        catalog
            .set_term_reviewers(term.id, vec!["bob".to_string()])
            .await
            .expect("set_term_reviewers should succeed");

        let reviewers = catalog
            .term_reviewers(term.id)
            .await
            .expect("term_reviewers should succeed");
        assert_eq!(reviewers, vec!["bob".to_string()]);
    }

    // ---- Epic 24 Slice D: terms attach to assets and columns ----

    async fn approved_term(catalog: &Catalog) -> graph_owl_storage::GlossaryTermRecord {
        seed_user(catalog, "alice").await;
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");
        catalog
            .set_term_reviewers(term.id, vec!["alice".to_string()])
            .await
            .expect("set_term_reviewers should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::InReview,
                "alice",
                None,
                None,
            )
            .await
            .expect("draft to in-review should succeed");
        catalog
            .transition_term(
                term.id,
                graph_owl_core::glossary::TermStatus::Approved,
                "alice",
                None,
                None,
            )
            .await
            .expect("in-review to approved should succeed")
    }

    #[tokio::test]
    async fn an_approved_term_can_be_attached() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let term = approved_term(&catalog).await;

        catalog
            .attach_term(term.id, "warehouse.public.orders", "alice")
            .await
            .expect("attach_term should succeed");

        let page = catalog
            .term_usage(term.id, &PageRequest::new(None, None).expect("valid"))
            .await
            .expect("term_usage should succeed");
        assert_eq!(page.data, vec!["warehouse.public.orders".to_string()]);
    }

    // **Only `Approved` terms attach** (decision 4) — the negative half.
    #[tokio::test]
    async fn a_draft_term_cannot_be_attached() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Account", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .attach_term(term.id, "warehouse.public.orders", "alice")
            .await
            .expect_err("a draft term should refuse attachment");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn attaching_an_unknown_term_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .attach_term(Uuid::new_v4(), "warehouse.public.orders", "alice")
            .await
            .expect_err("an unknown term should be not found");

        assert!(matches!(error, CatalogError::NotFound));
    }

    #[tokio::test]
    async fn detaching_a_term_removes_it_from_usage() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let term = approved_term(&catalog).await;
        catalog
            .attach_term(term.id, "warehouse.public.orders", "alice")
            .await
            .expect("attach_term should succeed");

        catalog
            .detach_term(term.id, "warehouse.public.orders")
            .await
            .expect("detach_term should succeed");

        let page = catalog
            .term_usage(term.id, &PageRequest::new(None, None).expect("valid"))
            .await
            .expect("term_usage should succeed");
        assert!(page.data.is_empty());
    }

    #[tokio::test]
    async fn detaching_something_never_attached_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let term = approved_term(&catalog).await;

        let error = catalog
            .detach_term(term.id, "warehouse.public.orders")
            .await
            .expect_err("nothing was attached");

        assert!(matches!(error, CatalogError::NotFound));
    }

    #[tokio::test]
    async fn usage_of_an_unknown_term_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .term_usage(
                Uuid::new_v4(),
                &PageRequest::new(None, None).expect("valid"),
            )
            .await
            .expect_err("an unknown term should be not found");

        assert!(matches!(error, CatalogError::NotFound));
    }

    // ---- Epic 24 Slice E: Metric as a first-class entity ----

    #[tokio::test]
    async fn a_metrics_fqn_is_namespaced_away_from_tables() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let metric = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        assert_eq!(metric.fully_qualified_name, "metric.revenue");
    }

    #[tokio::test]
    async fn a_metric_needs_a_name() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .create_metric(
                "  ",
                "definition".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect_err("a blank name should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn a_metric_needs_a_definition() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .create_metric(
                "revenue",
                String::new(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect_err("a blank definition should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    // **A source-less metric is permitted**, not refused — it is the
    // commonest metric there is and the one most worth cataloguing.
    #[tokio::test]
    async fn a_metric_with_no_sources_is_permitted() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let metric = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("a source-less metric should be permitted");

        assert!(metric.source_assets.is_empty());
        let gaps = graph_owl_core::metric::gaps(&graph_owl_core::metric::MetricClaims {
            source_assets: &metric.source_assets,
            defined_by: None,
            formula: metric.formula.as_deref(),
        });
        assert!(gaps.contains(&graph_owl_core::metric::MetricGap::NoSources));
    }

    #[tokio::test]
    async fn a_source_that_is_not_a_known_asset_is_refused() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec!["warehouse.public.orders".to_string()],
                None,
            )
            .await
            .expect_err("an unknown source asset should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn a_known_source_asset_is_accepted() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let asset = catalog
            .upsert_asset(
                &Principal::system(),
                UpsertAsset {
                    kind: AssetKind::Service,
                    name: "orders-service".to_string(),
                    parent_id: None,
                    description: None,
                    properties: None,
                },
            )
            .await
            .expect("upsert_asset should succeed");

        let metric = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![asset.fully_qualified_name.clone()],
                None,
            )
            .await
            .expect("a known source asset should be accepted");

        assert_eq!(metric.source_assets, vec![asset.fully_qualified_name]);
    }

    #[tokio::test]
    async fn defined_by_must_reference_an_approved_term() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let glossary = catalog
            .create_glossary("Finance", None)
            .await
            .expect("create_glossary should succeed");
        let term = catalog
            .create_term(glossary.id, "Revenue", String::new(), vec![], vec![])
            .await
            .expect("create_term should succeed");

        let error = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                Some(term.id),
            )
            .await
            .expect_err("a draft defining term should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn defined_by_an_approved_term_is_accepted() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let term = approved_term(&catalog).await;

        let metric = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                Some(term.id),
            )
            .await
            .expect("an approved defining term should be accepted");

        assert_eq!(metric.defined_by, Some(term.id));
    }

    #[tokio::test]
    async fn a_created_metric_can_be_fetched_updated_and_deleted() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let found = catalog
            .get_metric(created.id)
            .await
            .expect("get_metric should succeed");
        assert_eq!(found, Some(created.clone()));

        let updated = catalog
            .update_metric(
                created.id,
                graph_owl_storage::MetricUpdate {
                    definition: Some("revised definition".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("update_metric should succeed");
        assert_eq!(updated.definition, "revised definition");

        catalog
            .delete_metric(created.id)
            .await
            .expect("delete_metric should succeed");
        assert_eq!(
            catalog.get_metric(created.id).await.expect("get_metric"),
            None
        );
    }

    #[tokio::test]
    async fn every_created_metric_is_listed() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .create_metric(
                "revenue",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");
        catalog
            .create_metric(
                "churn",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Ratio,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let page = catalog
            .list_metrics(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_metrics should succeed");

        assert_eq!(page.data.len(), 2);
    }

    #[tokio::test]
    async fn a_metric_is_found_by_its_defining_terms_name() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let term = approved_term(&catalog).await;
        catalog
            .create_metric(
                "revenue",
                "total recognised revenue".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                Some(term.id),
            )
            .await
            .expect("create_metric should succeed");

        let hits = catalog
            .search_metrics(&term.name)
            .await
            .expect("search_metrics should succeed");

        assert_eq!(hits.len(), 1);
    }

    // ---- Epic 24 Slice F: metric lineage reconciliation ----

    async fn asset_fqn(catalog: &Catalog, name: &str) -> String {
        catalog
            .upsert_asset(
                &Principal::system(),
                UpsertAsset {
                    kind: AssetKind::Service,
                    name: name.to_string(),
                    parent_id: None,
                    description: None,
                    properties: None,
                },
            )
            .await
            .expect("upsert_asset should succeed")
            .fully_qualified_name
    }

    #[tokio::test]
    async fn declaring_sources_is_reflected_on_the_metric() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = asset_fqn(&catalog, "orders").await;
        let metric = catalog
            .create_metric(
                "revenue",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let updated = catalog
            .set_metric_sources(metric.id, vec![orders.clone()])
            .await
            .expect("set_metric_sources should succeed");

        assert_eq!(updated.source_assets, vec![orders]);
    }

    #[tokio::test]
    async fn removing_a_source_retracts_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = asset_fqn(&catalog, "orders").await;
        let metric = catalog
            .create_metric(
                "revenue",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![orders],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let updated = catalog
            .set_metric_sources(metric.id, vec![])
            .await
            .expect("set_metric_sources should succeed");

        assert!(updated.source_assets.is_empty());
    }

    #[tokio::test]
    async fn a_source_named_twice_is_deduplicated() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = asset_fqn(&catalog, "orders").await;
        let metric = catalog
            .create_metric(
                "revenue",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let updated = catalog
            .set_metric_sources(metric.id, vec![orders.clone(), orders])
            .await
            .expect("set_metric_sources should succeed");

        assert_eq!(updated.source_assets.len(), 1);
    }

    #[tokio::test]
    async fn a_source_that_is_not_a_known_asset_is_refused_on_reconciliation() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let metric = catalog
            .create_metric(
                "revenue",
                "d".to_string(),
                None,
                None,
                None,
                graph_owl_core::metric::CalculationType::Simple,
                vec![],
                None,
            )
            .await
            .expect("create_metric should succeed");

        let error = catalog
            .set_metric_sources(metric.id, vec!["warehouse.public.orders".to_string()])
            .await
            .expect_err("an unknown asset should be refused");

        assert!(matches!(error, CatalogError::Validation(_)));
    }

    #[tokio::test]
    async fn setting_sources_on_an_unknown_metric_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let error = catalog
            .set_metric_sources(Uuid::new_v4(), vec![])
            .await
            .expect_err("an unknown metric should be not found");

        assert!(matches!(error, CatalogError::NotFound));
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
mod resolution_decides_before_it_merges {
    //! Epic 17 Slices D, E and F at the **facade**.
    //!
    //! `RecordingGraph` resolves current state (including `as_of`) the same
    //! way Postgres does (`projection_isolation_tests`'s own doc comment),
    //! which is what makes it trustworthy for the split round-trip test
    //! below — a double that only recorded calls without resolving them
    //! could not prove the pre-merge state was restored.

    use super::*;
    use graph_owl_core::flake::{Sid, namespace};
    use graph_owl_core::resolution::{Evidence, MergeDecidedBy, Resolution, ReviewStatus};
    use graph_owl_storage::ReviewQueueFilter;
    use projection_isolation_tests::RecordingGraph;
    use tests::InMemoryStorage;

    fn asset_req(kind: AssetKind, name: &str, parent_id: Option<Uuid>) -> UpsertAsset {
        UpsertAsset {
            kind,
            name: name.to_string(),
            parent_id,
            description: None,
            properties: None,
        }
    }

    fn seeded() -> (Catalog, Arc<InMemoryStorage>, Arc<RecordingGraph>) {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage.clone()).with_graph(graph.clone());
        (catalog, storage, graph)
    }

    /// One schema, reachable only through the full containment chain
    /// (`upsert_asset` requires a `Table`'s parent to be a `Schema`, a
    /// `Schema`'s a `Database`, and so on) — needed so two tables can share
    /// a real parent for the `same_parent` term.
    async fn a_schema(catalog: &Catalog, principal: &Principal) -> Uuid {
        let svc = catalog
            .upsert_asset(principal, asset_req(AssetKind::Service, "svc", None))
            .await
            .expect("service");
        let db = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Database, "db", Some(svc.id)),
            )
            .await
            .expect("database");
        let schema = catalog
            .upsert_asset(principal, asset_req(AssetKind::Schema, "sch", Some(db.id)))
            .await
            .expect("schema");
        schema.id
    }

    fn only_merge_record(storage: &InMemoryStorage) -> graph_owl_core::resolution::MergeRecord {
        let records = storage.merge_records.lock().unwrap();
        assert_eq!(records.len(), 1, "expected exactly one merge record");
        records[0].clone()
    }

    /// `ReviewQueueFilter::default()`'s `limit` is `0` (the zero value of
    /// `usize`), which would return no rows regardless of matches — every
    /// test needs a real limit, matching `ValidationFilter`'s own test
    /// helper in `validation_decides_before_it_stores`.
    fn all_pending() -> ReviewQueueFilter {
        ReviewQueueFilter {
            limit: 50,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_case_variant_fqn_merges_and_records_the_decision() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();

        let lower = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");

        let resolution = catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");

        match resolution {
            Resolution::Existing { entity, confidence } => {
                assert_eq!(entity, lower.id);
                assert!((confidence - 1.0).abs() < 1e-9);
            }
            other => panic!("expected Existing, got {other:?}"),
        }

        let upper_sid = Sid::new(namespace::DSC, upper.id.to_string());
        let lower_sid = Sid::new(namespace::DSC, lower.id.to_string());
        assert!(
            graph.retracted_flakes().iter().any(|f| f.s == upper_sid),
            "the merged entity's own flakes should have been retracted"
        );
        assert!(
            !graph.retracted_flakes().iter().any(|f| f.s == lower_sid),
            "the canonical entity's own flakes must not be touched by the merge \
             — proves the retraction query was scoped to the merged subject, \
             not every entity in the graph"
        );
        let same_as = Sid::new(namespace::OWL, "sameAs");
        assert!(
            graph
                .asserted_flakes()
                .iter()
                .any(|f| f.s == upper_sid && f.p == same_as),
            "a sameAs assertion should point the merged entity at the canonical one"
        );

        let record = only_merge_record(&storage);
        assert_eq!(record.canonical, lower.id);
        assert_eq!(record.merged, upper.id);
        assert_eq!(record.evidence, vec![Evidence::NormalizedFqn]);
        assert_eq!(record.decided_by, MergeDecidedBy::Auto);
        assert_eq!(record.split_at, None);
    }

    /// A merge only touches the merged entity's facts **in the default
    /// graph**. Something like a SHACL shape or a reasoning derivation lives
    /// in a named graph and must survive a merge undisturbed — proven by
    /// seeding one directly and checking it after.
    #[tokio::test]
    async fn a_merge_does_not_touch_the_merged_entitys_named_graph_facts() {
        let (catalog, _storage, graph) = seeded();
        let principal = Principal::system();

        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");

        let upper_sid = Sid::new(namespace::DSC, upper.id.to_string());
        let named_graph = Sid::dsc("some-named-graph");
        let t = graph.next_time().await.expect("time");
        graph
            .assert_flakes(&[Flake {
                s: upper_sid.clone(),
                p: Sid::dsc("extra"),
                o: graph_owl_core::flake::FlakeValue::Boolean(true),
                cx: Some(named_graph.clone()),
                t,
                op: true,
            }])
            .await
            .expect("seed a named-graph fact");

        catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");

        let still_there = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(upper_sid),
                cx: Some(Some(named_graph)),
                ..Default::default()
            })
            .await
            .expect("query the named graph");
        assert_eq!(
            still_there.len(),
            1,
            "a merge must only retract the default-graph facts, not a named graph's"
        );
    }

    #[tokio::test]
    async fn a_deterministic_match_merges_even_when_auto_merge_is_disabled() {
        let storage = Arc::new(InMemoryStorage::default());
        let graph = RecordingGraph::working();
        let catalog = Catalog::new(storage.clone())
            .with_graph(graph.clone())
            .with_auto_merge_enabled(false);
        let principal = Principal::system();

        let lower = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");

        let resolution = catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");

        // Deterministic matching is a different, more certain mechanism than
        // the confidence-band decision the toggle governs (Slice A's
        // short-circuit is unconditional) — disabling auto-merge must not
        // reach into it.
        assert!(matches!(resolution, Resolution::Existing { entity, .. } if entity == lower.id));
    }

    #[tokio::test]
    async fn a_review_band_score_creates_nothing_and_reports_evidence() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();
        let schema = a_schema(&catalog, &principal).await;

        let a = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders", Some(schema)),
            )
            .await
            .expect("table a");
        let b = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders_v2", Some(schema)),
            )
            .await
            .expect("table b");
        for (parent, col) in [
            (a.id, "id"),
            (a.id, "amount"),
            (b.id, "id"),
            (b.id, "amount"),
        ] {
            catalog
                .upsert_asset(&principal, asset_req(AssetKind::Column, col, Some(parent)))
                .await
                .expect("column");
        }

        // Captured after all the setup writes above (which project their own
        // flakes) and before the call under test, so any *new* assert/retract
        // can only have come from `resolve_asset` itself.
        let asserted_before = graph.asserted_flakes().len();
        let retracted_before = graph.retracted_flakes().len();

        let resolution = catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("resolve");

        match resolution {
            Resolution::Ambiguous { candidates } => {
                assert_eq!(candidates.len(), 1);
                assert_eq!(candidates[0].entity, a.id);
                assert!(
                    (0.6..0.9).contains(&candidates[0].score),
                    "expected a review-band score, got {}",
                    candidates[0].score
                );
                // Exact counts, not just "some overlap was reported" — proves
                // the real column list flowed through rather than some fixed
                // placeholder both sides would coincidentally agree on.
                assert!(
                    candidates[0]
                        .evidence
                        .contains(&Evidence::StructuralOverlap {
                            shared_columns: 2,
                            total: 2,
                        }),
                    "expected exactly the two real shared columns, got {:?}",
                    candidates[0].evidence
                );
                assert!(candidates[0].evidence.contains(&Evidence::SameParent));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }

        assert!(
            storage.merge_records.lock().unwrap().is_empty(),
            "a review-band resolution must create nothing"
        );
        assert_eq!(
            graph.asserted_flakes().len(),
            asserted_before,
            "a review-band resolution must not assert anything to the graph"
        );
        assert_eq!(
            graph.retracted_flakes().len(),
            retracted_before,
            "a review-band resolution must not retract anything from the graph"
        );
    }

    #[tokio::test]
    async fn dissimilar_entities_resolve_to_new() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();

        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("a");
        let b = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "zzqxw", None))
            .await
            .expect("b");

        // Different soundex, different name+parent, different normalized
        // FQN — `b` shares no blocking key with anything, so it has no
        // candidates at all.
        let resolution = catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("resolve");
        assert_eq!(resolution, Resolution::New);
    }

    #[tokio::test]
    async fn resolving_an_unknown_asset_is_not_found() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();

        let result = catalog.resolve_asset(&principal, Uuid::new_v4()).await;
        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn split_restores_exactly_the_pre_merge_state() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();

        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");

        let upper_sid = Sid::new(namespace::DSC, upper.id.to_string());
        let before_merge = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(upper_sid.clone()),
                cx: Some(None),
                ..Default::default()
            })
            .await
            .expect("state before merge");
        assert!(
            !before_merge.is_empty(),
            "the merged entity must have real flakes to restore"
        );

        catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");
        let merge_id = only_merge_record(&storage).id;

        let restored = catalog
            .split_merge(&principal, merge_id)
            .await
            .expect("split");
        assert!(restored.split_at.is_some());

        let after_split = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(upper_sid),
                cx: Some(None),
                ..Default::default()
            })
            .await
            .expect("state after split");

        let mut before_sorted = before_merge;
        let mut after_sorted = after_split;
        before_sorted.sort_by_key(|f| format!("{:?}", f.p));
        after_sorted.sort_by_key(|f| format!("{:?}", f.p));
        assert_eq!(
            before_sorted
                .iter()
                .map(|f| (&f.p, &f.o))
                .collect::<Vec<_>>(),
            after_sorted
                .iter()
                .map(|f| (&f.p, &f.o))
                .collect::<Vec<_>>(),
            "the state after a split must equal the state before its merge"
        );
    }

    /// A split's restoration query must be scoped to exactly the merged
    /// entity's **default-graph** facts — not every entity in the graph, and
    /// not a named graph's facts, which a merge never touched in the first
    /// place (see the sibling merge test) and a split must not move into the
    /// default graph either.
    #[tokio::test]
    async fn split_only_restores_the_merged_entitys_default_graph_facts() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();

        let lower = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");

        let upper_sid = Sid::new(namespace::DSC, upper.id.to_string());
        let lower_sid = Sid::new(namespace::DSC, lower.id.to_string());
        let named_graph = Sid::dsc("some-named-graph");
        let t = graph.next_time().await.expect("time");
        graph
            .assert_flakes(&[Flake {
                s: upper_sid.clone(),
                p: Sid::dsc("extra"),
                o: graph_owl_core::flake::FlakeValue::Boolean(true),
                cx: Some(named_graph.clone()),
                t,
                op: true,
            }])
            .await
            .expect("seed a named-graph fact");

        catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");
        let merge_id = only_merge_record(&storage).id;

        let asserted_before_split = graph.asserted_flakes().len();
        catalog
            .split_merge(&principal, merge_id)
            .await
            .expect("split");
        let newly_asserted = graph.asserted_flakes()[asserted_before_split..].to_vec();

        assert!(
            newly_asserted.iter().all(|f| f.s == upper_sid),
            "a split must only reassert the merged entity's own facts, not \
             the canonical's — proves the restoration query was scoped to \
             the merged subject: {newly_asserted:?}"
        );
        assert!(
            !newly_asserted.iter().any(|f| f.s == lower_sid),
            "the canonical entity's facts must not be reasserted by a split"
        );

        let leaked_into_default_graph = graph
            .query_pattern(&graph_owl_core::flake::TriplePattern {
                s: Some(upper_sid),
                p: Some(Sid::dsc("extra")),
                cx: Some(None),
                ..Default::default()
            })
            .await
            .expect("query the default graph");
        assert!(
            leaked_into_default_graph.is_empty(),
            "a named-graph fact must not be restored into the default graph"
        );
    }

    #[tokio::test]
    async fn splitting_an_already_split_merge_is_a_conflict() {
        let (catalog, storage, _graph) = seeded();
        let principal = Principal::system();

        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");
        catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");
        let merge_id = only_merge_record(&storage).id;

        catalog
            .split_merge(&principal, merge_id)
            .await
            .expect("first split");
        let second = catalog.split_merge(&principal, merge_id).await;

        assert!(matches!(
            second,
            Err(CatalogError::Conflict {
                kind: ConflictKind::MergeAlreadySplit,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn splitting_an_unknown_merge_is_not_found() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();

        let result = catalog.split_merge(&principal, Uuid::new_v4()).await;
        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn a_split_pair_is_not_immediately_re_merged() {
        let (catalog, storage, _graph) = seeded();
        let principal = Principal::system();

        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("lower");
        let upper = catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "ORDERS", None))
            .await
            .expect("upper");
        catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve");
        let merge_id = only_merge_record(&storage).id;
        catalog
            .split_merge(&principal, merge_id)
            .await
            .expect("split");

        // Re-resolving right after the split must not immediately re-merge
        // what a human (or, here, the test) just took apart — the pair is
        // excluded for the cooldown, not merely downgraded to a review.
        let resolution = catalog
            .resolve_asset(&principal, upper.id)
            .await
            .expect("resolve again");
        assert_eq!(resolution, Resolution::New);
        assert_eq!(
            storage.merge_records.lock().unwrap().len(),
            1,
            "no second merge record should have been created"
        );
    }

    // ---- Epic 17 Slice F: the review queue ----

    #[tokio::test]
    async fn an_ambiguous_resolution_queues_the_candidate_as_pending() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        let schema = a_schema(&catalog, &principal).await;

        let a = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders", Some(schema)),
            )
            .await
            .expect("table a");
        let b = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders_v2", Some(schema)),
            )
            .await
            .expect("table b");
        for (parent, col) in [
            (a.id, "id"),
            (a.id, "amount"),
            (b.id, "id"),
            (b.id, "amount"),
        ] {
            catalog
                .upsert_asset(&principal, asset_req(AssetKind::Column, col, Some(parent)))
                .await
                .expect("column");
        }

        catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("resolve");

        let (entries, total) = catalog
            .review_queue(&principal, &all_pending())
            .await
            .expect("queue");
        assert_eq!(total, 1);
        assert_eq!(entries[0].target, b.id);
        assert_eq!(entries[0].candidate, a.id);
        assert_eq!(entries[0].status, ReviewStatus::Pending);
    }

    #[tokio::test]
    async fn rejecting_a_queued_pair_is_not_re_queued_by_a_later_resolution() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        let schema = a_schema(&catalog, &principal).await;

        let a = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders", Some(schema)),
            )
            .await
            .expect("table a");
        let b = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders_v2", Some(schema)),
            )
            .await
            .expect("table b");
        for (parent, col) in [
            (a.id, "id"),
            (a.id, "amount"),
            (b.id, "id"),
            (b.id, "amount"),
        ] {
            catalog
                .upsert_asset(&principal, asset_req(AssetKind::Column, col, Some(parent)))
                .await
                .expect("column");
        }

        catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("first resolve");
        let (entries, _) = catalog
            .review_queue(&principal, &all_pending())
            .await
            .expect("queue");
        let entry_id = entries[0].id;

        catalog
            .reject_review(&principal, entry_id)
            .await
            .expect("reject");

        // Re-ingestion of the same draft re-runs resolution, which recomputes
        // the identical candidate — this is the rejection-persistence test
        // Slice F's RED demands: without idempotent queuing, this would
        // create a second, fresh `pending` entry for the same pair.
        catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("second resolve");

        let (pending, pending_total) = catalog
            .review_queue(&principal, &all_pending())
            .await
            .expect("pending queue");
        assert_eq!(
            pending_total, 0,
            "a rejected pair must not reappear as pending: {pending:?}"
        );

        let (all_rejected, rejected_total) = catalog
            .review_queue(
                &principal,
                &ReviewQueueFilter {
                    status: Some(ReviewStatus::Rejected),
                    limit: 50,
                    ..Default::default()
                },
            )
            .await
            .expect("rejected queue");
        assert_eq!(rejected_total, 1);
        assert_eq!(all_rejected[0].id, entry_id);
    }

    #[tokio::test]
    async fn confirming_a_queued_pair_writes_the_merge() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();
        let schema = a_schema(&catalog, &principal).await;

        let a = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders", Some(schema)),
            )
            .await
            .expect("table a");
        let b = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders_v2", Some(schema)),
            )
            .await
            .expect("table b");
        for (parent, col) in [
            (a.id, "id"),
            (a.id, "amount"),
            (b.id, "id"),
            (b.id, "amount"),
        ] {
            catalog
                .upsert_asset(&principal, asset_req(AssetKind::Column, col, Some(parent)))
                .await
                .expect("column");
        }
        catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("resolve");
        let (entries, _) = catalog
            .review_queue(&principal, &all_pending())
            .await
            .expect("queue");
        let entry_id = entries[0].id;

        let resolution = catalog
            .confirm_review(&principal, entry_id)
            .await
            .expect("confirm");
        match resolution {
            Resolution::Existing { entity, .. } => assert_eq!(entity, a.id),
            other => panic!("expected Existing, got {other:?}"),
        }

        let record = only_merge_record(&storage);
        assert_eq!(record.canonical, a.id);
        assert_eq!(record.merged, b.id);
        assert_eq!(
            record.decided_by,
            MergeDecidedBy::Human {
                user_id: principal.id.clone()
            }
        );
        let b_sid = Sid::new(namespace::DSC, b.id.to_string());
        assert!(graph.retracted_flakes().iter().any(|f| f.s == b_sid));
    }

    #[tokio::test]
    async fn confirming_an_already_decided_entry_is_a_conflict() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        let schema = a_schema(&catalog, &principal).await;

        let a = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders", Some(schema)),
            )
            .await
            .expect("table a");
        let b = catalog
            .upsert_asset(
                &principal,
                asset_req(AssetKind::Table, "orders_v2", Some(schema)),
            )
            .await
            .expect("table b");
        for (parent, col) in [
            (a.id, "id"),
            (a.id, "amount"),
            (b.id, "id"),
            (b.id, "amount"),
        ] {
            catalog
                .upsert_asset(&principal, asset_req(AssetKind::Column, col, Some(parent)))
                .await
                .expect("column");
        }
        catalog
            .resolve_asset(&principal, b.id)
            .await
            .expect("resolve");
        let (entries, _) = catalog
            .review_queue(&principal, &all_pending())
            .await
            .expect("queue");
        let entry_id = entries[0].id;

        catalog
            .confirm_review(&principal, entry_id)
            .await
            .expect("first confirm");
        let second = catalog.confirm_review(&principal, entry_id).await;

        assert!(matches!(
            second,
            Err(CatalogError::Conflict {
                kind: ConflictKind::ReviewAlreadyDecided,
                ..
            })
        ));
    }

    // ---- Epic 17 Slice G: mention resolution ----

    fn mention(text: &str, context: &str) -> graph_owl_core::resolution::TextMention {
        graph_owl_core::resolution::TextMention {
            text: text.to_string(),
            expected_type: None,
            context: context.to_string(),
        }
    }

    async fn two_same_named_tables_in_different_schemas(
        catalog: &Catalog,
        principal: &Principal,
    ) -> (Uuid, Uuid) {
        let service = catalog
            .upsert_asset(principal, asset_req(AssetKind::Service, "svc", None))
            .await
            .expect("service");
        let database = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Database, "db", Some(service.id)),
            )
            .await
            .expect("database");
        let staging = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Schema, "staging", Some(database.id)),
            )
            .await
            .expect("staging schema");
        let prod = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Schema, "prod", Some(database.id)),
            )
            .await
            .expect("prod schema");
        let in_staging = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Table, "orders", Some(staging.id)),
            )
            .await
            .expect("table in staging");
        let in_prod = catalog
            .upsert_asset(
                principal,
                asset_req(AssetKind::Table, "orders", Some(prod.id)),
            )
            .await
            .expect("table in prod");
        (in_staging.id, in_prod.id)
    }

    #[tokio::test]
    async fn context_resolves_a_mention_to_the_matching_schema() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        let (in_staging, _in_prod) =
            two_same_named_tables_in_different_schemas(&catalog, &principal).await;
        let source = Uuid::new_v4();

        let resolution = catalog
            .resolve_mention(
                &principal,
                source,
                mention("orders", "the orders table in staging"),
            )
            .await
            .expect("resolve_mention")
            .expect("should resolve");

        assert_eq!(resolution.entity, in_staging);
        assert_eq!(resolution.source, source);
        assert!(resolution.confidence > 0.5);
    }

    #[tokio::test]
    async fn a_mention_never_creates_a_merge_record() {
        let (catalog, storage, graph) = seeded();
        let principal = Principal::system();
        two_same_named_tables_in_different_schemas(&catalog, &principal).await;

        let asserted_before = graph.asserted_flakes().len();
        let retracted_before = graph.retracted_flakes().len();

        catalog
            .resolve_mention(
                &principal,
                Uuid::new_v4(),
                mention("orders", "the orders table in staging"),
            )
            .await
            .expect("resolve_mention");

        assert!(
            storage.merge_records.lock().unwrap().is_empty(),
            "a mention must never write a MergeRecord — mentions link, entities merge"
        );
        assert_eq!(
            graph.asserted_flakes().len(),
            asserted_before,
            "a mention must not touch the graph"
        );
        assert_eq!(graph.retracted_flakes().len(), retracted_before);
    }

    #[tokio::test]
    async fn a_mention_with_no_matching_candidate_resolves_to_none() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        catalog
            .upsert_asset(&principal, asset_req(AssetKind::Service, "orders", None))
            .await
            .expect("orders");

        let resolution = catalog
            .resolve_mention(&principal, Uuid::new_v4(), mention("zzqxw", ""))
            .await
            .expect("resolve_mention");

        assert_eq!(resolution, None);
    }

    #[tokio::test]
    async fn a_resolved_mention_is_recorded_against_its_source() {
        let (catalog, _storage, _graph) = seeded();
        let principal = Principal::system();
        let (in_staging, _) =
            two_same_named_tables_in_different_schemas(&catalog, &principal).await;
        let source = Uuid::new_v4();

        catalog
            .resolve_mention(
                &principal,
                source,
                mention("orders", "the orders table in staging"),
            )
            .await
            .expect("resolve_mention")
            .expect("should resolve");

        let recorded = catalog
            .storage
            .mention_resolutions_for_source(source)
            .await
            .expect("recorded mentions");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].entity, in_staging);
        assert_eq!(recorded[0].text, "orders");
    }
}

#[cfg(test)]
mod webhooks_are_verified_before_they_are_believed {
    //! Epic 18 Slice A at the **facade**.
    //!
    //! The Postgres repository tests (`webhook_endpoints.rs`) prove the
    //! schema; the connector crate's own tests prove HMAC/Ed25519
    //! verification is correct in isolation. This proves the
    //! **orchestration**: that `Catalog::receive_webhook` actually calls
    //! into that verification rather than trusting the caller, and that a
    //! disabled endpoint and a bad signature produce the right *kind* of
    //! refusal.

    use super::*;
    use graph_owl_storage::{SignatureScheme, WebhookEndpoint};
    use tests::InMemoryStorage;

    fn endpoint(scheme: SignatureScheme) -> WebhookEndpoint {
        let now = chrono::Utc::now();
        WebhookEndpoint {
            id: Uuid::new_v4(),
            path: "dbt".to_string(),
            source: "dbt-bot".to_string(),
            signature_scheme: scheme,
            mapping: "dbt-run-completed".to_string(),
            event_filter: vec!["run.completed".to_string()],
            enabled: true,
            has_secret: false,
            rate_limit_per_minute: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn hmac_scheme() -> SignatureScheme {
        SignatureScheme::HmacSha256 {
            header: "X-Signature".to_string(),
            prefix: "sha256=".to_string(),
        }
    }

    fn hmac_sign(secret: &[u8], body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
        mac.update(body);
        let bytes = mac.finalize().into_bytes();
        format!(
            "sha256={}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }

    #[tokio::test]
    async fn a_correctly_signed_delivery_is_recorded() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(secret))
            .await
            .expect("register");

        let body = br#"{"event":"run.completed"}"#;
        let signature = hmac_sign(secret, body);

        let event = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await
            .expect("verified delivery should be recorded");

        assert_eq!(event.endpoint, registered.id);
        assert_eq!(event.raw, body);
        assert_eq!(event.state, graph_owl_core::webhook::EventState::Received);
    }

    /// Epic 18 Slice E: the malformed-JSON check happens **inside
    /// `receive_webhook` itself**, not only when `process_inbound_event`
    /// later gets around to mapping it. A signature can verify over any
    /// bytes; a body this broken is worth failing before the async
    /// pipeline ever sees it, so the HTTP layer can answer `400`
    /// synchronously instead of `201` for something that was never going
    /// anywhere.
    #[tokio::test]
    async fn a_verified_but_unparseable_body_is_recorded_failed_synchronously() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(secret))
            .await
            .expect("register");

        let body = b"this is not json";
        let signature = hmac_sign(secret, body);

        let event = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await
            .expect("a verified delivery is recorded even when unparseable");

        assert_eq!(event.state, graph_owl_core::webhook::EventState::Failed);
        assert!(
            event.reason.as_deref().is_some_and(|r| r.contains("JSON")),
            "{:?}",
            event.reason
        );
    }

    #[tokio::test]
    async fn a_bad_signature_is_unauthenticated_and_records_nothing() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(b"the-real-secret"))
            .await
            .expect("register");

        let body = br#"{"event":"run.completed"}"#;
        let wrong_signature = hmac_sign(b"a-guessed-secret", body);

        let result = catalog
            .receive_webhook(&registered, Some(&wrong_signature), body)
            .await;

        assert!(matches!(result, Err(CatalogError::Unauthenticated)));
    }

    #[tokio::test]
    async fn a_missing_signature_header_is_unauthenticated() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(b"secret"))
            .await
            .expect("register");

        let result = catalog
            .receive_webhook(&registered, None, br#"{"event":"run.completed"}"#)
            .await;

        assert!(matches!(result, Err(CatalogError::Unauthenticated)));
    }

    #[tokio::test]
    async fn a_tampered_body_is_unauthenticated() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(secret))
            .await
            .expect("register");

        let signature = hmac_sign(secret, br#"{"amount":10}"#);
        let result = catalog
            .receive_webhook(&registered, Some(&signature), br#"{"amount":99999}"#)
            .await;

        assert!(matches!(result, Err(CatalogError::Unauthenticated)));
    }

    #[tokio::test]
    async fn a_disabled_endpoint_is_not_found_not_forbidden() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let mut disabled = endpoint(hmac_scheme());
        disabled.enabled = false;
        let registered = catalog
            .register_webhook_endpoint(disabled, Some(secret))
            .await
            .expect("register");

        let body = br#"{"event":"run.completed"}"#;
        let signature = hmac_sign(secret, body);
        let result = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await;

        // An existence signal is unnecessary here (Slice E's own reasoning,
        // applied a slice early since it falls out of `enabled` for free):
        // a disabled endpoint reads the same as one that was never
        // registered.
        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn an_ed25519_delivery_verifies_against_the_stored_public_key() {
        use ed25519_dalek::{Signer, SigningKey};
        use rand::rngs::OsRng;

        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();
        let registered = catalog
            .register_webhook_endpoint(
                endpoint(SignatureScheme::Ed25519 {
                    header: "X-Signature-Ed25519".to_string(),
                }),
                Some(&public_key),
            )
            .await
            .expect("register");

        let body = br#"{"event":"dag.completed"}"#;
        let signature = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(signing_key.sign(body).to_bytes())
        };

        let event = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await
            .expect("verified delivery should be recorded");
        assert_eq!(event.endpoint, registered.id);
    }

    #[tokio::test]
    async fn registering_a_taken_path_is_a_conflict() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(b"secret-a"))
            .await
            .expect("first registration");

        let result = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(b"secret-b"))
            .await;

        assert!(matches!(
            result,
            Err(CatalogError::Conflict {
                kind: ConflictKind::WebhookPathExists,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn a_registered_endpoint_is_found_by_id_by_path_and_in_the_list() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let registered = catalog
            .register_webhook_endpoint(endpoint(hmac_scheme()), Some(b"secret"))
            .await
            .expect("register");

        let by_id = catalog
            .webhook_endpoint(registered.id)
            .await
            .expect("read by id")
            .expect("endpoint exists");
        assert_eq!(by_id.id, registered.id);

        let by_path = catalog
            .webhook_endpoint_by_path(&registered.path)
            .await
            .expect("read by path")
            .expect("endpoint exists");
        assert_eq!(by_path.id, registered.id);

        assert!(
            catalog
                .webhook_endpoint(Uuid::new_v4())
                .await
                .expect("read by id")
                .is_none()
        );

        let listed = catalog
            .list_webhook_endpoints()
            .await
            .expect("list endpoints");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, registered.id);
    }
}

#[cfg(test)]
mod redelivered_webhooks_are_deduped_not_reapplied {
    //! Epic 18 Slice B at the **facade**.
    //!
    //! `sender_event_id`/`sender_timestamp` extraction from a payload is
    //! Slice C's declarative-mapping problem, not this slice's — so every
    //! delivery through `Catalog::receive_webhook` today has no sender id,
    //! which means content-hash dedup is the only path this facade can
    //! exercise end-to-end. The sender-id path and the last-writer-wins
    //! comparison itself are proven in isolation in `graph-owl-core`
    //! (`dedup_key`, `compare_timestamps`); this proves the *storage-backed
    //! mechanism* — that a real redelivery is recognized and recorded as
    //! `Duplicate` rather than reapplied.

    use super::*;
    use graph_owl_storage::{SignatureScheme, WebhookEndpoint};
    use tests::InMemoryStorage;

    fn endpoint() -> WebhookEndpoint {
        let now = chrono::Utc::now();
        WebhookEndpoint {
            id: Uuid::new_v4(),
            path: "dbt".to_string(),
            source: "dbt-bot".to_string(),
            signature_scheme: SignatureScheme::HmacSha256 {
                header: "X-Signature".to_string(),
                prefix: "sha256=".to_string(),
            },
            mapping: "dbt-run-completed".to_string(),
            event_filter: vec!["run.completed".to_string()],
            enabled: true,
            has_secret: false,
            rate_limit_per_minute: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn hmac_sign(secret: &[u8], body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
        mac.update(body);
        let bytes = mac.finalize().into_bytes();
        format!(
            "sha256={}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }

    #[tokio::test]
    async fn a_redelivered_payload_is_recorded_as_duplicate_not_reapplied() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let registered = catalog
            .register_webhook_endpoint(endpoint(), Some(secret))
            .await
            .expect("register");

        let body = br#"{"event":"run.completed","run_id":42}"#;
        let signature = hmac_sign(secret, body);

        let first = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await
            .expect("first delivery should verify and record");
        assert_eq!(first.state, graph_owl_core::webhook::EventState::Received);

        let second = catalog
            .receive_webhook(&registered, Some(&signature), body)
            .await
            .expect("a redelivery still verifies; it is a duplicate, not a rejection");
        assert_eq!(second.state, graph_owl_core::webhook::EventState::Duplicate);
        assert_ne!(
            second.id, first.id,
            "the redelivery is its own recorded row, not the same event returned twice"
        );
    }

    #[tokio::test]
    async fn two_different_payloads_to_the_same_endpoint_are_not_deduped_against_each_other() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let secret = b"shared-secret";
        let registered = catalog
            .register_webhook_endpoint(endpoint(), Some(secret))
            .await
            .expect("register");

        let first_body = br#"{"event":"run.completed","run_id":1}"#;
        let second_body = br#"{"event":"run.completed","run_id":2}"#;

        let first = catalog
            .receive_webhook(
                &registered,
                Some(&hmac_sign(secret, first_body)),
                first_body,
            )
            .await
            .expect("first delivery");
        let second = catalog
            .receive_webhook(
                &registered,
                Some(&hmac_sign(secret, second_body)),
                second_body,
            )
            .await
            .expect("second delivery");

        assert_eq!(first.state, graph_owl_core::webhook::EventState::Received);
        assert_eq!(
            second.state,
            graph_owl_core::webhook::EventState::Received,
            "different content must not collide with an unrelated delivery's dedup key"
        );
    }
}

#[cfg(test)]
mod mappings_turn_payloads_into_drafts {
    //! Epic 18 Slice C at the **facade**.
    //!
    //! The expression evaluator itself is proven in isolation
    //! (`graph-owl-connectors::webhook_mapping`); this proves the
    //! *orchestration* — that `Catalog::dry_run_mapping` looks up the right
    //! mapping, reports a missing field or an invalid kind without
    //! guessing, reuses `validate_draft` rather than a second shape check,
    //! and never writes anything regardless of outcome.

    use super::*;
    use graph_owl_core::flake::{FlakeValue, Sid, namespace};
    use projection_isolation_tests::RecordingGraph;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tests::InMemoryStorage;

    fn path(pointer: &str) -> graph_owl_storage::Expression {
        graph_owl_storage::Expression::Path {
            pointer: pointer.to_string(),
        }
    }

    fn mapping(name: &str) -> graph_owl_storage::Mapping {
        graph_owl_storage::Mapping {
            name: name.to_string(),
            version: 0, // ignored on write
            kind: path("/kind"),
            entity_name: path("/tableName"),
            parent_fqn: None,
            description: Some(path("/description")),
            properties: BTreeMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn a_complete_payload_produces_a_draft() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        let payload = json!({"kind": "table", "tableName": "orders"});
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        match outcome {
            MappingOutcome::Draft(draft) => {
                assert_eq!(draft.kind, "table");
                assert_eq!(draft.name, "orders");
            }
            other => panic!("expected a draft, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_missing_required_path_names_the_field_not_just_invalid() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        // No `tableName` in the payload — the mapping's `entity_name` path
        // resolves to nothing.
        let payload = json!({"kind": "table"});
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        assert_eq!(outcome, MappingOutcome::MissingField { field: "name" });
    }

    #[tokio::test]
    async fn a_kind_that_names_no_known_asset_kind_is_reported() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        let payload = json!({"kind": "dbt_run", "tableName": "orders"});
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        assert_eq!(
            outcome,
            MappingOutcome::InvalidKind {
                kind: "dbt_run".to_string()
            }
        );
    }

    #[tokio::test]
    async fn an_unregistered_mapping_name_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog.dry_run_mapping("no-such-mapping", &json!({})).await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn a_dry_run_never_writes_anything() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone());
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");
        storage.forbid_writes();

        let payload = json!({"kind": "table", "tableName": "orders"});
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        assert!(matches!(outcome, MappingOutcome::Draft(_)));
    }

    // ---- shape rejection: reuses `validate_draft`, not a second check ----

    fn a(id: &str) -> Sid {
        Sid::dsc(id)
    }
    fn sh(term: &str) -> Sid {
        Sid::new(namespace::SHACL, term)
    }
    fn rdf_type() -> Sid {
        Sid::new(namespace::RDF, "type")
    }

    /// Every entity that has a kind at all (every asset) needs a
    /// `description`, stated in the shapes graph.
    fn shape_facts(t: i64) -> Vec<graph_owl_core::flake::Flake> {
        let in_shapes = |s: Sid, p: Sid, o: FlakeValue| graph_owl_core::flake::Flake {
            s,
            p,
            o,
            cx: Some(shapes_graph()),
            t,
            op: true,
        };
        vec![
            in_shapes(
                a("TableNeedsDescription"),
                rdf_type(),
                FlakeValue::Ref(sh("NodeShape")),
            ),
            // `targetClass` selects on literal `rdf:type`, which
            // `asset_to_flakes` never asserts — an asset's kind is its own
            // `dsc("type")` predicate, a different one. `targetSubjectsOf`
            // selects any subject with that predicate at all, which every
            // projected asset has.
            in_shapes(
                a("TableNeedsDescription"),
                sh("targetSubjectsOf"),
                FlakeValue::Ref(a("type")),
            ),
            in_shapes(
                a("TableNeedsDescription"),
                sh("property"),
                FlakeValue::Ref(a("TableNeedsDescription/description")),
            ),
            in_shapes(
                a("TableNeedsDescription/description"),
                sh("path"),
                FlakeValue::Ref(a("description")),
            ),
            in_shapes(
                a("TableNeedsDescription/description"),
                sh("minCount"),
                FlakeValue::Int(1),
            ),
        ]
    }

    #[tokio::test]
    async fn a_draft_that_would_violate_a_shape_is_rejected_naming_it() {
        let graph = RecordingGraph::working();
        graph
            .assert_flakes(&shape_facts(1))
            .await
            .expect("seed the shape");
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        // No `description` in the payload, and the mapping's own
        // `description` field is `None` for anything absent — the
        // resulting draft has no description, which the shape refuses.
        let payload = json!({"kind": "table", "tableName": "orders"});
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        match outcome {
            MappingOutcome::ShapeViolation { reason } => {
                assert!(reason.contains("TableNeedsDescription"), "{reason}");
            }
            other => panic!("expected a shape violation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_draft_that_satisfies_every_shape_is_returned() {
        let graph = RecordingGraph::working();
        graph
            .assert_flakes(&shape_facts(1))
            .await
            .expect("seed the shape");
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()))
            .with_graph(graph.clone() as Arc<dyn TripleStore>);
        let mut m = mapping("dbt-run-completed");
        m.description = Some(path("/description"));
        catalog.upsert_mapping(m).await.expect("register mapping");

        let payload = json!({
            "kind": "table",
            "tableName": "orders",
            "description": "one row per order",
        });
        let outcome = catalog
            .dry_run_mapping("dbt-run-completed", &payload)
            .await
            .expect("dry run");

        assert!(matches!(outcome, MappingOutcome::Draft(_)), "{outcome:?}");
    }

    // ---- versioning: every update is a new version, auditable ----

    #[tokio::test]
    async fn updating_a_mapping_adds_a_version_rather_than_overwriting() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let first = catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("first version");
        assert_eq!(first.version, 1);

        let second = catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("second version");
        assert_eq!(second.version, 2);

        let latest = catalog
            .mapping("dbt-run-completed")
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(latest.version, 2, "the latest read is the newest version");

        let history = catalog
            .mapping_versions("dbt-run-completed")
            .await
            .expect("history");
        assert_eq!(
            history.len(),
            2,
            "the old version is still there, not overwritten"
        );
        assert_eq!(history[0].version, 2, "newest first");
        assert_eq!(history[1].version, 1);
    }
}

#[cfg(test)]
mod inbound_events_are_mapped_and_applied {
    //! Epic 18 Slice D at the **facade**.
    //!
    //! The mapping engine and shape-check reuse are proven in isolation
    //! (Slice C's own tests); this proves the *pipeline* —
    //! `Catalog::process_inbound_event` actually reaches the catalog, a
    //! rejection at any step lands the event in the dead-letter queue
    //! naming why, and replaying a window is idempotent against events
    //! that already succeeded.

    use super::*;
    use graph_owl_storage::{
        DeadLetterFilter, Expression, Mapping, SignatureScheme, WebhookEndpoint,
    };
    use std::collections::BTreeMap;
    use tests::InMemoryStorage;

    fn path(pointer: &str) -> Expression {
        Expression::Path {
            pointer: pointer.to_string(),
        }
    }

    fn endpoint() -> WebhookEndpoint {
        let now = chrono::Utc::now();
        WebhookEndpoint {
            id: Uuid::new_v4(),
            path: "dbt".to_string(),
            source: "dbt-bot".to_string(),
            signature_scheme: SignatureScheme::HmacSha256 {
                header: "X-Signature".to_string(),
                prefix: "sha256=".to_string(),
            },
            mapping: "dbt-run-completed".to_string(),
            event_filter: vec!["run.completed".to_string()],
            enabled: true,
            has_secret: false,
            rate_limit_per_minute: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn mapping() -> Mapping {
        Mapping {
            name: "dbt-run-completed".to_string(),
            version: 0,
            kind: path("/kind"),
            entity_name: path("/tableName"),
            parent_fqn: None,
            description: Some(path("/description")),
            properties: BTreeMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    fn hmac_sign(secret: &[u8], body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
        mac.update(body);
        let bytes = mac.finalize().into_bytes();
        format!(
            "sha256={}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }

    async fn seeded() -> (Catalog, WebhookEndpoint) {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_mapping(mapping())
            .await
            .expect("register mapping");
        let registered = catalog
            .register_webhook_endpoint(endpoint(), Some(b"secret"))
            .await
            .expect("register endpoint");
        (catalog, registered)
    }

    async fn seeded_with_storage() -> (Catalog, Arc<InMemoryStorage>, WebhookEndpoint) {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone());
        catalog
            .upsert_mapping(mapping())
            .await
            .expect("register mapping");
        let registered = catalog
            .register_webhook_endpoint(endpoint(), Some(b"secret"))
            .await
            .expect("register endpoint");
        (catalog, storage, registered)
    }

    async fn deliver(catalog: &Catalog, endpoint: &WebhookEndpoint, body: &[u8]) -> Uuid {
        let signature = hmac_sign(b"secret", body);
        catalog
            .receive_webhook(endpoint, Some(&signature), body)
            .await
            .expect("delivery should verify")
            .id
    }

    /// Constructs an event directly with a `sender_timestamp` set, bypassing
    /// `receive_webhook` — extracting one from a real payload is still
    /// Slice C's declarative-mapping problem, unsolved, so this is the only
    /// way to exercise the out-of-order comparison until that lands.
    async fn deliver_with_timestamp(
        storage: &Arc<InMemoryStorage>,
        endpoint_id: Uuid,
        body: &[u8],
        sender_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Uuid {
        let event = graph_owl_core::webhook::InboundEvent {
            id: Uuid::new_v4(),
            endpoint: endpoint_id,
            sender_event_id: None,
            sender_timestamp,
            received_at: chrono::Utc::now(),
            dedup_key: graph_owl_core::webhook::dedup_key(None, body),
            raw: body.to_vec(),
            state: graph_owl_core::webhook::EventState::Received,
            reason: None,
        };
        storage
            .create_inbound_event(event)
            .await
            .expect("create")
            .id
    }

    #[tokio::test]
    async fn a_valid_delivery_is_mapped_and_applied() {
        let (catalog, endpoint) = seeded().await;
        // `service` — a root-kind asset needs no parent, the cheap fixture
        // for a happy path that isn't testing containment.
        let body = br#"{"kind":"service","tableName":"orders","description":"one row per order"}"#;
        let event_id = deliver(&catalog, &endpoint, body).await;

        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");

        let processed = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            processed.state,
            graph_owl_core::webhook::EventState::Applied
        );
        assert_eq!(processed.reason, None);

        let asset = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("the mapped entity was actually applied");
        assert_eq!(asset.kind, AssetKind::Service);
        assert_eq!(asset.description.as_deref(), Some("one row per order"));
    }

    #[tokio::test]
    async fn a_missing_required_field_dead_letters_naming_the_mapping_and_field() {
        let (catalog, endpoint) = seeded().await;
        let body = br#"{"kind":"table"}"#; // no tableName
        let event_id = deliver(&catalog, &endpoint, body).await;

        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");

        let processed = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(processed.state, graph_owl_core::webhook::EventState::Failed);
        let reason = processed.reason.expect("a failed event names why");
        assert!(reason.contains("dbt-run-completed"), "{reason}");
        assert!(reason.contains("name"), "{reason}");
    }

    #[tokio::test]
    async fn an_unparseable_payload_is_dead_lettered_not_a_panic() {
        let (catalog, endpoint) = seeded().await;
        let event_id = deliver(&catalog, &endpoint, b"this is not json").await;

        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");

        let processed = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(processed.state, graph_owl_core::webhook::EventState::Failed);
        assert!(
            processed
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("JSON")),
            "{:?}",
            processed.reason
        );
    }

    #[tokio::test]
    async fn a_missing_mapping_is_dead_lettered_naming_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        // Note: no `upsert_mapping` call — the endpoint names a mapping
        // that was never registered.
        let registered = catalog
            .register_webhook_endpoint(endpoint(), Some(b"secret"))
            .await
            .expect("register endpoint");
        let event_id = deliver(&catalog, &registered, br#"{"kind":"table"}"#).await;

        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");

        let processed = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(processed.state, graph_owl_core::webhook::EventState::Failed);
        assert!(
            processed
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("dbt-run-completed")),
            "{:?}",
            processed.reason
        );
    }

    /// **The mapping and the shape check can both pass and the write can
    /// still be invalid** — a `table` with no parent is well-formed by
    /// both of those measures and refused by `upsert_asset`'s own
    /// containment rule. This must dead-letter exactly like a mapping or
    /// shape failure, not propagate as an unhandled error: from a webhook's
    /// perspective this is the same kind of outcome, a payload this
    /// mapping cannot turn into a writable entity.
    #[tokio::test]
    async fn a_structural_failure_from_the_upsert_itself_is_dead_lettered_too() {
        let (catalog, endpoint) = seeded().await;
        let body = br#"{"kind":"table","tableName":"orders"}"#;
        let event_id = deliver(&catalog, &endpoint, body).await;

        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process must not propagate the upsert's validation error");

        let processed = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(processed.state, graph_owl_core::webhook::EventState::Failed);
        assert!(
            processed
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("parent")),
            "{:?}",
            processed.reason
        );
    }

    #[tokio::test]
    async fn reprocessing_an_already_applied_event_is_a_no_op() {
        let (catalog, endpoint) = seeded().await;
        let body = br#"{"kind":"service","tableName":"orders"}"#;
        let event_id = deliver(&catalog, &endpoint, body).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("first process");
        let first_pass = catalog
            .inbound_event(event_id)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            first_pass.state,
            graph_owl_core::webhook::EventState::Applied
        );

        let after_first_apply = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists")
            .version;

        // Reprocessing must not error and must not attempt anything —
        // state-gated inside `process_inbound_event` itself.
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("reprocess is a no-op, not an error");

        let asset = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            asset.version, after_first_apply,
            "reprocessing must not write a second version"
        );
    }

    #[tokio::test]
    async fn a_dead_lettered_event_is_listed_and_filterable() {
        let (catalog, endpoint) = seeded().await;
        let event_id = deliver(&catalog, &endpoint, br#"{"kind":"table"}"#).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");

        let dlq = catalog
            .dead_letter_queue(&DeadLetterFilter {
                limit: 50,
                ..Default::default()
            })
            .await
            .expect("dlq");
        assert!(dlq.iter().any(|e| e.id == event_id), "{dlq:?}");

        let by_endpoint = catalog
            .dead_letter_queue(&DeadLetterFilter {
                endpoint: Some(endpoint.id),
                limit: 50,
                ..Default::default()
            })
            .await
            .expect("dlq");
        assert!(by_endpoint.iter().any(|e| e.id == event_id));

        let by_other_endpoint = catalog
            .dead_letter_queue(&DeadLetterFilter {
                endpoint: Some(Uuid::new_v4()),
                limit: 50,
                ..Default::default()
            })
            .await
            .expect("dlq");
        assert!(
            !by_other_endpoint.iter().any(|e| e.id == event_id),
            "a different endpoint's filter must not return this event"
        );
    }

    #[tokio::test]
    async fn replaying_after_a_mapping_fix_re_processes_and_applies() {
        let (catalog, endpoint) = seeded().await;
        // Delivered while the mapping requires `tableName` and the payload
        // does not have it — dead-lettered. `service`, not `table`: once
        // fixed below this must reach the actual upsert and succeed, and a
        // `table` would fail there too, on requiring a parent — a second,
        // unrelated failure this test is not about.
        let body = br#"{"kind":"service"}"#;
        let event_id = deliver(&catalog, &endpoint, body).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("first attempt fails");
        assert_eq!(
            catalog
                .inbound_event(event_id)
                .await
                .expect("read")
                .expect("exists")
                .state,
            graph_owl_core::webhook::EventState::Failed
        );

        // Fix the mapping: `name` now comes from a literal, so the same
        // payload resolves.
        let mut fixed = mapping();
        fixed.entity_name = Expression::Literal {
            value: "orders".to_string(),
        };
        catalog.upsert_mapping(fixed).await.expect("fixed mapping");

        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let until = chrono::Utc::now() + chrono::Duration::hours(1);
        let summary = catalog
            .replay_window(endpoint.id, since, until)
            .await
            .expect("replay");

        assert_eq!(summary.attempted, 1);
        assert_eq!(summary.applied, 1, "{summary:?}");
        assert_eq!(
            catalog
                .inbound_event(event_id)
                .await
                .expect("read")
                .expect("exists")
                .state,
            graph_owl_core::webhook::EventState::Applied
        );
    }

    #[tokio::test]
    async fn replaying_without_fixing_the_problem_counts_as_still_failed() {
        let (catalog, endpoint) = seeded().await;
        let body = br#"{"kind":"table"}"#; // no tableName, and no mapping fix follows
        let event_id = deliver(&catalog, &endpoint, body).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("first attempt fails");

        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let until = chrono::Utc::now() + chrono::Duration::hours(1);
        let summary = catalog
            .replay_window(endpoint.id, since, until)
            .await
            .expect("replay");

        assert_eq!(
            summary,
            ReplaySummary {
                attempted: 1,
                applied: 0,
                still_failed: 1,
                skipped: 0,
            },
            "an unresolved failure must be counted as attempted-but-still-failed, not silently dropped"
        );
    }

    #[tokio::test]
    async fn replaying_a_window_skips_already_applied_events() {
        let (catalog, endpoint) = seeded().await;
        let body = br#"{"kind":"service","tableName":"orders"}"#;
        let event_id = deliver(&catalog, &endpoint, body).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");
        let after_first_apply = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists")
            .version;

        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let until = chrono::Utc::now() + chrono::Duration::hours(1);
        let summary = catalog
            .replay_window(endpoint.id, since, until)
            .await
            .expect("replay");

        assert_eq!(
            summary,
            ReplaySummary {
                attempted: 0,
                applied: 0,
                still_failed: 0,
                skipped: 1,
            },
            "an already-Applied event must be skipped, not reprocessed"
        );
        let asset = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            asset.version, after_first_apply,
            "replay must not double-apply an already-applied event"
        );
    }

    #[tokio::test]
    async fn purging_removes_old_dead_letters() {
        let (catalog, endpoint) = seeded().await;
        let event_id = deliver(&catalog, &endpoint, br#"{"kind":"table"}"#).await;
        catalog
            .process_inbound_event(event_id)
            .await
            .expect("process");
        assert_eq!(
            catalog
                .inbound_event(event_id)
                .await
                .expect("read")
                .expect("exists")
                .state,
            graph_owl_core::webhook::EventState::Failed
        );

        // Nothing older than a year — the just-created row survives.
        let purged = catalog
            .purge_dead_letters(chrono::Utc::now() - chrono::Duration::days(365))
            .await
            .expect("purge");
        assert_eq!(purged, 0);
        assert!(
            catalog
                .inbound_event(event_id)
                .await
                .expect("read")
                .is_some()
        );

        // Everything up to now — the row is gone.
        let purged = catalog
            .purge_dead_letters(chrono::Utc::now() + chrono::Duration::seconds(1))
            .await
            .expect("purge");
        assert_eq!(purged, 1);
        assert!(
            catalog
                .inbound_event(event_id)
                .await
                .expect("read")
                .is_none()
        );
    }

    // ---- out-of-order protection ----

    fn t(seconds_ago: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now() - chrono::Duration::seconds(seconds_ago)
    }

    #[tokio::test]
    async fn a_newer_event_is_applied_and_updates_the_high_water_mark() {
        let (catalog, storage, endpoint) = seeded_with_storage().await;
        let first = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v1"}"#,
            Some(t(20)),
        )
        .await;
        catalog
            .process_inbound_event(first)
            .await
            .expect("first apply");

        let second = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v2"}"#,
            Some(t(10)),
        )
        .await;
        catalog
            .process_inbound_event(second)
            .await
            .expect("second apply");

        assert_eq!(
            catalog
                .inbound_event(second)
                .await
                .expect("read")
                .expect("exists")
                .state,
            graph_owl_core::webhook::EventState::Applied
        );
        let asset = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(asset.description.as_deref(), Some("v2"));
    }

    #[tokio::test]
    async fn an_older_event_is_superseded_and_does_not_overwrite_newer_state() {
        let (catalog, storage, endpoint) = seeded_with_storage().await;
        let newer = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v2"}"#,
            Some(t(10)),
        )
        .await;
        catalog
            .process_inbound_event(newer)
            .await
            .expect("newer applies first");

        // Arrives *after* the newer one but describes an *earlier* state.
        let older = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v1"}"#,
            Some(t(20)),
        )
        .await;
        catalog
            .process_inbound_event(older)
            .await
            .expect("process must not error");

        let processed = catalog
            .inbound_event(older)
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            processed.state,
            graph_owl_core::webhook::EventState::Superseded
        );
        assert!(
            processed.reason.is_some(),
            "a superseded event still names why, like a failed one does"
        );

        let asset = catalog
            .get_asset_by_fqn("orders")
            .await
            .expect("read")
            .expect("exists");
        assert_eq!(
            asset.description.as_deref(),
            Some("v2"),
            "the older delivery must not have overwritten the newer state"
        );
    }

    #[tokio::test]
    async fn an_event_with_no_sender_timestamp_still_applies_falling_back_to_arrival_order() {
        let (catalog, storage, endpoint) = seeded_with_storage().await;
        let first = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v1"}"#,
            Some(t(10)),
        )
        .await;
        catalog
            .process_inbound_event(first)
            .await
            .expect("first apply");

        // No sender_timestamp at all — Slice C's own extraction gap, not
        // this mechanism's — must not block processing.
        let second = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v2"}"#,
            None,
        )
        .await;
        catalog
            .process_inbound_event(second)
            .await
            .expect("second apply");

        assert_eq!(
            catalog
                .inbound_event(second)
                .await
                .expect("read")
                .expect("exists")
                .state,
            graph_owl_core::webhook::EventState::Applied
        );
    }

    #[tokio::test]
    async fn a_superseded_event_is_skipped_on_replay_like_an_applied_one() {
        let (catalog, storage, endpoint) = seeded_with_storage().await;
        let newer = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v2"}"#,
            Some(t(10)),
        )
        .await;
        catalog
            .process_inbound_event(newer)
            .await
            .expect("newer applies");
        let older = deliver_with_timestamp(
            &storage,
            endpoint.id,
            br#"{"kind":"service","tableName":"orders","description":"v1"}"#,
            Some(t(20)),
        )
        .await;
        catalog
            .process_inbound_event(older)
            .await
            .expect("older is superseded");

        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let until = chrono::Utc::now() + chrono::Duration::hours(1);
        let summary = catalog
            .replay_window(endpoint.id, since, until)
            .await
            .expect("replay");

        assert_eq!(
            summary,
            ReplaySummary {
                attempted: 0,
                applied: 0,
                still_failed: 0,
                skipped: 2,
            },
            "both the applied and the superseded event must be skipped: {summary:?}"
        );
    }
}

#[cfg(test)]
mod streamed_messages_are_mapped_applied_and_resolved {
    //! Epic 19 Slice A at the **facade**.
    //!
    //! The mapping evaluator is proven in isolation
    //! (`graph-owl-connectors::webhook_mapping`), and its orchestration
    //! (mapping lookup, shape validation) is already proven by Epic 18's own
    //! facade tests, since `Catalog::apply_streamed_message` reuses the same
    //! private `resolve_and_validate_draft` helper. What is unique to this
    //! slice, and therefore what this module proves, is the one behavioral
    //! difference from a webhook: **resolution runs automatically**, with no
    //! caller ever asking for it — decision 7.

    use super::*;
    use projection_isolation_tests::RecordingGraph;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tests::InMemoryStorage;

    /// `resolve_asset` needs a graph engine to look up candidates against —
    /// same fixture `resolution_decides_before_it_merges` uses.
    fn seeded() -> (Catalog, Arc<InMemoryStorage>) {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone()).with_graph(RecordingGraph::working());
        (catalog, storage)
    }

    fn path(pointer: &str) -> graph_owl_storage::Expression {
        graph_owl_storage::Expression::Path {
            pointer: pointer.to_string(),
        }
    }

    fn mapping(name: &str) -> graph_owl_storage::Mapping {
        graph_owl_storage::Mapping {
            name: name.to_string(),
            version: 0, // ignored on write
            kind: path("/kind"),
            entity_name: path("/name"),
            parent_fqn: None,
            description: None,
            properties: BTreeMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    /// **The criterion this slice exists to prove.** A streamed message
    /// mapping to a near-duplicate of an already-cataloged entity (same
    /// name, different case — `is_deterministic_match`'s own normalization)
    /// must produce a merge record, with nothing in this test ever calling
    /// `resolve_asset` directly. A merge retracts the losing entity's
    /// graph-level flakes rather than deleting its `assets` row (Epic 17's
    /// own reversibility design — `POST /merges/{id}/split` needs the row to
    /// restore), so a merge record is the correct, direct signal that
    /// resolution ran — not the row's absence, which a merge was never
    /// going to produce.
    #[tokio::test]
    async fn a_streamed_near_duplicate_is_merged_not_duplicated() {
        let (catalog, storage) = seeded();
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");
        let original = catalog
            .upsert_asset(
                &Principal::system(),
                UpsertAsset {
                    kind: AssetKind::Service,
                    name: "Orders".to_string(),
                    parent_id: None,
                    description: None,
                    properties: None,
                },
            )
            .await
            .expect("seed the existing entity");

        catalog
            .apply_streamed_message(
                "dbt-run-completed",
                &json!({"kind": "service", "name": "orders"}),
            )
            .await
            .expect("apply");

        let records = storage.merge_records.lock().unwrap();
        assert_eq!(
            records.len(),
            1,
            "resolution must have merged the near-duplicate into the original"
        );
        assert_eq!(
            records[0].canonical, original.id,
            "the original entity must be the merge's canonical id"
        );
    }

    /// A message mapping to a genuinely new entity is applied as one —
    /// resolution running automatically does not mean *everything* merges.
    #[tokio::test]
    async fn a_streamed_message_with_no_existing_match_is_applied_as_new() {
        let (catalog, _storage) = seeded();
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        catalog
            .apply_streamed_message(
                "dbt-run-completed",
                &json!({"kind": "service", "name": "Payments"}),
            )
            .await
            .expect("apply");

        let created = catalog
            .get_asset_by_fqn("Payments")
            .await
            .expect("read")
            .expect("the new entity must exist");
        assert_eq!(created.name, "Payments");
    }

    #[tokio::test]
    async fn an_unregistered_mapping_is_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .apply_streamed_message("no-such-mapping", &json!({"kind": "service", "name": "x"}))
            .await;

        assert!(matches!(result, Err(CatalogError::NotFound)));
    }

    #[tokio::test]
    async fn a_missing_required_field_is_a_validation_error_naming_it() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        catalog
            .upsert_mapping(mapping("dbt-run-completed"))
            .await
            .expect("register mapping");

        // No `name` in the payload — the mapping's `entity_name` path
        // resolves to nothing.
        let result = catalog
            .apply_streamed_message("dbt-run-completed", &json!({"kind": "service"}))
            .await;

        match result {
            Err(CatalogError::Validation(errors)) => {
                assert!(errors.iter().any(|e| e.field == "name"), "{errors:?}");
            }
            other => panic!("expected a validation error naming the field, got {other:?}"),
        }
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
                parent_team_id: None,
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

/// Epic 16 Slice C — what a batch job says happened.
///
/// These run against the in-memory double at the **real** chunk size, so the
/// cancellation and heartbeat behaviour under test is the behaviour that
/// ships. A test-only chunk knob would have made these cheaper and would
/// have proved nothing about production.
#[cfg(test)]
mod batch_jobs_report_what_landed {
    use super::*;
    use graph_owl_connectors::rows::Format;
    use tests::InMemoryStorage;

    fn jsonl(rows: impl IntoIterator<Item = String>) -> std::io::Cursor<String> {
        let mut text = String::new();
        for row in rows {
            text.push_str(&row);
            text.push('\n');
        }
        std::io::Cursor::new(text)
    }

    fn service_rows(count: usize) -> Vec<String> {
        (0..count)
            .map(|n| format!("{{\"kind\":\"service\",\"name\":\"svc-{n}\"}}"))
            .collect()
    }

    async fn run(
        body: std::io::Cursor<String>,
        error_cap: usize,
    ) -> (Catalog, graph_owl_storage::IngestJob) {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let id = Uuid::new_v4();
        catalog
            .create_ingest_job(id, "jsonl", "tester")
            .await
            .expect("registered");
        catalog
            .run_batch_ingest(
                id,
                std::io::BufReader::new(body),
                Format::Jsonl,
                Principal::system(),
                error_cap,
            )
            .await
            .expect("ran");
        let job = catalog.ingest_job(id).await.expect("read").expect("job");
        (catalog, job)
    }

    #[tokio::test]
    async fn a_clean_file_lands_every_row_and_succeeds() {
        let (_, job) = run(jsonl(service_rows(3)), BATCH_ERROR_CAP).await;

        assert_eq!(job.state, "succeeded");
        assert_eq!(job.accepted, 3);
        assert_eq!(job.rejected, 0);
        assert_eq!(job.rows_read, 3);
        assert!(job.failures.is_empty());
        assert!(job.finished_at.is_some(), "a settled job has an end");
    }

    /// **The partial-success criterion, at file scale.** One typo must not
    /// cost the other rows, and the report has to name the line so a client
    /// can find it in a file they cannot read by eye.
    #[tokio::test]
    async fn one_bad_row_is_reported_by_line_number_and_the_rest_land() {
        let mut rows = service_rows(3);
        rows.insert(1, "not json at all".to_string());

        let (_, job) = run(jsonl(rows), BATCH_ERROR_CAP).await;

        assert_eq!(job.state, "partial");
        assert_eq!(job.accepted, 3);
        assert_eq!(job.rejected, 1);
        assert_eq!(job.failures.len(), 1);
        assert_eq!(
            job.failures[0].row, 2,
            "the second line of the file, not the second failure"
        );
    }

    /// Nothing landed is `failed`, not `partial` — `partial` is the bucket
    /// for jobs that mostly worked, and a client seeing it re-pushes only
    /// what was rejected.
    #[tokio::test]
    async fn a_file_where_nothing_lands_is_failed_not_partial() {
        let (_, job) = run(jsonl(vec!["nope".into(), "also nope".into()]), 100).await;

        assert_eq!(job.state, "failed");
        assert_eq!(job.accepted, 0);
    }

    /// **The cap stops reading.** A 500k-row file with the wrong delimiter
    /// produces 500k identical errors, and the report nobody can read is the
    /// failure this prevents — so `rows_read` must be far short of the file.
    #[tokio::test]
    async fn the_error_cap_halts_reading_and_names_itself() {
        let bad: Vec<String> = (0..50).map(|n| format!("garbage {n}")).collect();

        let (_, job) = run(jsonl(bad), 3).await;

        assert_eq!(job.state, "failed");
        assert_eq!(job.rejected, 3);
        assert!(
            job.rows_read < 50,
            "reading continued past the cap: {} rows",
            job.rows_read
        );
        let reason = job.halt_reason.expect("a halt says why");
        assert!(reason.contains("too many errors"), "{reason}");
    }

    /// A halt still reports the rows it had read, so a client's counts are
    /// not silently short by up to one chunk.
    #[tokio::test]
    async fn a_halted_job_still_reports_the_failures_from_its_last_chunk() {
        let bad: Vec<String> = (0..10).map(|n| format!("garbage {n}")).collect();

        let (_, job) = run(jsonl(bad), 2).await;

        assert_eq!(job.failures.len(), 2, "{:?}", job.failures);
        assert_eq!(job.failures[0].row, 1);
    }

    /// **Cancellation is observed at a chunk boundary**, which is why this
    /// test uses a file larger than one chunk: a smaller file finishes
    /// before there is anything to stop, and a test that pretended otherwise
    /// would be testing a knob rather than the product.
    #[tokio::test]
    async fn a_cancelled_job_stops_mid_file_and_reports_what_landed() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let id = Uuid::new_v4();
        catalog
            .create_ingest_job(id, "jsonl", "tester")
            .await
            .expect("registered");
        assert!(
            catalog.cancel_ingest_job(id).await.expect("cancelled"),
            "an unfinished job can be cancelled"
        );

        catalog
            .run_batch_ingest(
                id,
                std::io::BufReader::new(jsonl(service_rows(BATCH_CHUNK_ROWS * 2 + 10))),
                Format::Jsonl,
                Principal::system(),
                BATCH_ERROR_CAP,
            )
            .await
            .expect("ran");

        let job = catalog.ingest_job(id).await.expect("read").expect("job");
        assert_eq!(job.state, "failed");
        assert_eq!(
            job.rows_read,
            i64::try_from(BATCH_CHUNK_ROWS).expect("fits"),
            "it stopped at the first chunk boundary, not at the end of the file"
        );
        assert_eq!(
            job.accepted,
            i64::try_from(BATCH_CHUNK_ROWS).expect("fits"),
            "what landed before the stop still landed"
        );
        let reason = job.halt_reason.expect("a halt says why");
        assert!(reason.contains("cancelled"), "{reason}");
    }

    /// A job already settled cannot be cancelled — there is nothing running,
    /// and rewriting a finished verdict would lose the answer a client came
    /// back for.
    #[tokio::test]
    async fn a_finished_job_cannot_be_cancelled() {
        let (catalog, job) = run(jsonl(service_rows(1)), BATCH_ERROR_CAP).await;

        assert!(!catalog.cancel_ingest_job(job.id).await.expect("asked"));
    }

    /// **A crashed worker must not leave a job reading `running` forever.**
    /// The reaper runs on poll rather than on a timer, so this is the exact
    /// path a waiting client takes.
    #[tokio::test]
    async fn a_job_that_stopped_reporting_is_failed_when_somebody_polls_it() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone());
        let id = Uuid::new_v4();
        let stale = chrono::Utc::now() - chrono::Duration::seconds(ABANDONED_AFTER_SECONDS + 1);
        storage
            .create_ingest_job(&graph_owl_storage::IngestJob {
                id,
                format: "jsonl".to_string(),
                state: "running".to_string(),
                rows_read: 12,
                accepted: 12,
                rejected: 0,
                failures: Vec::new(),
                halt_reason: None,
                cancel_requested: false,
                submitted_by: "tester".to_string(),
                started_at: stale,
                heartbeat_at: stale,
                finished_at: None,
            })
            .await
            .expect("registered");

        let job = catalog.ingest_job(id).await.expect("read").expect("job");

        assert_eq!(job.state, "failed");
        assert!(job.finished_at.is_some());
        assert!(
            job.halt_reason
                .expect("a reaped job says why")
                .contains("abandoned"),
            "a reaped job must say it was reaped, not merely fail"
        );
    }

    /// And a live job is *not* reaped, which is the assertion that stops the
    /// reaper from being a random job-killer.
    #[tokio::test]
    async fn a_job_that_is_still_reporting_survives_a_poll() {
        let storage = Arc::new(InMemoryStorage::default());
        let catalog = Catalog::new(storage.clone());
        let id = Uuid::new_v4();
        catalog
            .create_ingest_job(id, "jsonl", "tester")
            .await
            .expect("registered");

        let job = catalog.ingest_job(id).await.expect("read").expect("job");

        assert_eq!(job.state, "queued");
        assert!(job.finished_at.is_none());
    }

    /// Row numbers are file line numbers all the way through, including past
    /// a chunk boundary — a client greps their file with this, and a number
    /// that restarted per chunk would send them to the wrong line.
    #[tokio::test]
    async fn a_row_number_survives_a_chunk_boundary() {
        let mut rows = service_rows(BATCH_CHUNK_ROWS + 20);
        let broken = BATCH_CHUNK_ROWS + 5;
        rows[broken] = "not json".to_string();

        let (_, job) = run(jsonl(rows), BATCH_ERROR_CAP).await;

        assert_eq!(job.failures.len(), 1, "{:?}", job.failures);
        assert_eq!(
            job.failures[0].row,
            u64::try_from(broken + 1).expect("fits"),
            "line numbers are 1-based and do not restart per chunk"
        );
    }

    /// A failure is reported **once**. Storage appends, so a worker that did
    /// not clear its buffer after reporting would repeat every earlier
    /// failure in each subsequent chunk — a report that grows quadratically
    /// and names the same row over and over.
    #[tokio::test]
    async fn a_failure_is_not_repeated_in_the_next_chunk() {
        let mut rows = service_rows(BATCH_CHUNK_ROWS * 2);
        rows[1] = "not json".to_string();

        let (_, job) = run(jsonl(rows), BATCH_ERROR_CAP).await;

        assert_eq!(job.failures.len(), 1, "{:?}", job.failures);
        assert_eq!(job.rejected, 1);
    }

    /// A row naming a kind this catalog does not have is a **row** failure,
    /// not a job failure: one bad `kind` cell must not stop a file.
    #[tokio::test]
    async fn an_unknown_kind_rejects_the_row_and_names_what_was_expected() {
        let rows = vec![
            "{\"kind\":\"tabel\",\"name\":\"orders\"}".to_string(),
            "{\"kind\":\"service\",\"name\":\"svc\"}".to_string(),
        ];

        let (_, job) = run(jsonl(rows), BATCH_ERROR_CAP).await;

        assert_eq!(job.state, "partial");
        assert_eq!(job.accepted, 1);
        assert!(
            job.failures[0].detail.contains("tabel"),
            "{}",
            job.failures[0].detail
        );
    }

    /// A file that cannot even be opened still settles the job — a client
    /// polling must never wait on a job that will never move.
    #[tokio::test]
    async fn a_job_that_never_started_is_failed_rather_than_left_queued() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let id = Uuid::new_v4();
        catalog
            .create_ingest_job(id, "jsonl", "tester")
            .await
            .expect("registered");

        catalog
            .fail_ingest_job(id, "the spooled upload could not be read")
            .await
            .expect("failed");

        let job = catalog.ingest_job(id).await.expect("read").expect("job");
        assert_eq!(job.state, "failed");
        assert!(job.finished_at.is_some());
    }
}
