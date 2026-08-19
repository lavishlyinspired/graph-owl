//! The graph engine port: what a triple store must do, independent of where
//! the flakes actually live.
//!
//! Implemented by `graph-owl-engine-postgres`. Kept separate from
//! `graph-owl-storage` because the two answer different questions — storage
//! owns the entity rows that are the source of truth, this owns the graph
//! projection of them (`plans/04-engine-triples.md` decision 1).

use async_trait::async_trait;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    /// A flake carried namespace code 0, which is reserved for "unset".
    /// Storing it would put an undiagnosable row in the graph, so it is
    /// refused at the boundary rather than written and puzzled over later.
    #[error(
        "namespace code 0 is reserved for unset; the {position} of this flake is uninitialized"
    )]
    UnsetNamespace { position: &'static str },

    /// A flake named a predicate the registry has never heard of.
    ///
    /// Refused rather than written, because a predicate is what makes a flake
    /// readable: an undefined one has no datatype and no cardinality, so
    /// nothing downstream can say what the value *means*. Time travel makes
    /// that permanent — the row cannot be cleaned up later without deleting
    /// history, which this store does not do.
    #[error(
        "predicate {namespace}:{name} is not defined; define it in the predicate registry before asserting it"
    )]
    UnregisteredPredicate { namespace: u16, name: String },

    /// A flake's object is not the `FlakeValue` variant the predicate is
    /// registered to carry.
    ///
    /// Refused rather than written, for the same reason an unregistered
    /// predicate is: every reader of `dsc:confidence` assumes a `Float`
    /// because the registry says so, and a `String` written past this check
    /// is indistinguishable from a correct value until something tries to
    /// use it — at which point the failure is far from its cause and the row
    /// is already permanent.
    #[error(
        "{namespace}:{name} is registered as value_type {expected}, but this flake carries value_type {actual}"
    )]
    WrongValueType {
        namespace: u16,
        name: String,
        expected: i16,
        actual: i16,
    },

    /// One assertion batch names two *different* values for the same
    /// (subject, predicate) on a predicate the registry declares
    /// single-valued (`many = FALSE`).
    ///
    /// **Scoped to one batch, not the store's current state.** Catching a
    /// batch that contradicts itself is unambiguous — nothing about ordering
    /// or a concurrent writer is in question, both values arrived in the
    /// same call. Catching a batch that contradicts an *existing* row would
    /// need a query per candidate subject before every write and a decision
    /// about the retract-then-assert sequence every update path in this
    /// codebase already uses (retract the old value, assert the new one, as
    /// two separate calls) — a real, larger obligation, not this check's
    /// job. An update that skips the retract still overwrites cleanly at
    /// read time, because current-state resolution already takes the
    /// latest `t`; it just leaves a stale row time travel can still see,
    /// which is a data-hygiene question, not a correctness one this check
    /// is positioned to catch.
    #[error(
        "{namespace}:{name} is single-valued (many = false), but this batch asserts more than one value for {subject}"
    )]
    CardinalityViolation {
        namespace: u16,
        name: String,
        subject: String,
    },

    #[error("engine backend failed: {0}")]
    Backend(String),
}

/// Storage and retrieval of flakes.
///
/// Deliberately not one method per query shape: [`query_pattern`] takes a
/// pattern with any combination of bound and unbound terms, and the adapter
/// picks the index. A method per shape would push index selection into every
/// caller, which is exactly the knowledge the adapter exists to hold.
///
/// [`query_pattern`]: TripleStore::query_pattern
#[async_trait]
pub trait TripleStore: Send + Sync {
    /// Write flakes. One statement per call regardless of batch size — a
    /// projection of a wide table is hundreds of flakes, and a round trip
    /// each would make projection the slowest part of every write.
    ///
    /// Re-asserting an identical flake at the same `t` is a no-op, so a
    /// retried projection converges rather than duplicating.
    ///
    /// Every predicate must be defined in the [`PredicateRegistry`] first.
    /// Retraction is deliberately not gated the same way — see
    /// [`retract_flakes`].
    ///
    /// # Errors
    ///
    /// [`EngineError::UnsetNamespace`] if any flake carries namespace 0;
    /// [`EngineError::UnregisteredPredicate`] if any names an undefined
    /// predicate, in which case **no** flake in the batch is written;
    /// [`EngineError::Backend`] if the write fails.
    ///
    /// [`retract_flakes`]: TripleStore::retract_flakes
    async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError>;

    /// Withdraw facts, by writing a retraction row for each.
    ///
    /// **Never a delete.** The asserting row stays, which is the entire reason
    /// history here is recoverable by construction rather than reconstructed
    /// from a parallel audit table that can drift.
    ///
    /// `op` is forced to `false` on every flake regardless of what it carries,
    /// so a caller can hand this the original assertion and get that fact's
    /// retraction — which is exactly what a projection update does.
    ///
    /// Retracting a fact that was never asserted is a no-op, not an error: a
    /// reconciler re-projecting an entity cannot know which facts the previous
    /// projection managed to write.
    ///
    /// Unlike [`assert_flakes`], this is **not** gated on the predicate
    /// registry. A retraction only ever withdraws a fact already in the graph;
    /// refusing one because its predicate is no longer welcome would strand
    /// that fact permanently — unwritable and equally un-take-back-able.
    ///
    /// [`assert_flakes`]: TripleStore::assert_flakes
    ///
    /// # Errors
    ///
    /// [`EngineError::UnsetNamespace`] if any flake carries namespace 0 — a
    /// retraction that names no valid fact would withdraw nothing while
    /// reporting success;
    /// [`EngineError::Backend`] if the write fails.
    async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError>;

    /// Flakes matching the pattern, in current state unless the pattern names
    /// an `as_of`. Retracted facts are excluded; the rows recording them are
    /// not deleted.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn query_pattern(&self, pattern: &TriplePattern) -> Result<Vec<Flake>, EngineError>;

    /// How many flakes the pattern matches.
    ///
    /// Must agree with `query_pattern(..).len()` for the same pattern — a
    /// count computed by a different path than the rows is a count that can
    /// disagree with them, and the disagreement always surfaces as a paging
    /// bug rather than as a count bug.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn count(&self, pattern: &TriplePattern) -> Result<u64, EngineError>;

    /// How many **distinct subjects** match `pattern` — a node count, as
    /// opposed to [`count`](Self::count)'s fact count.
    ///
    /// **Exists because the two are routinely confused and the confusion is
    /// invisible.** A console tile reading "graph nodes" reported zero against
    /// a store holding 724 facts, because the count behind it matched flakes
    /// carrying one specific type predicate — which only projected catalog
    /// assets have. Every subject an RDF import had ever landed was uncounted,
    /// and nothing said so: a plausible number, quietly wrong
    /// (`plans/123-reconow-agentic-reconciliation.md` §9).
    ///
    /// A subject is counted once however many flakes it carries, and
    /// regardless of which vocabulary typed it — or whether anything typed it
    /// at all.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn count_distinct_subjects(&self, pattern: &TriplePattern) -> Result<u64, EngineError>;

    /// How many facts are **edges** — those whose object references another
    /// subject rather than holding a literal.
    ///
    /// Exists for the same reason [`count_distinct_subjects`] does, and was
    /// found by the same investigation: a console tile labelled "edges"
    /// counted one specific predicate that only *projected catalog
    /// relationships* carry, so a store full of imported RDF reported zero
    /// edges beside hundreds of nodes. An edge is a reference, whatever
    /// predicate names it.
    ///
    /// [`count_distinct_subjects`]: Self::count_distinct_subjects
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn count_edges(&self) -> Result<u64, EngineError>;

    /// Reserve the next transaction time.
    ///
    /// Every flake in one logical change shares the `t` this returns, which is
    /// what makes "the state after change N" a well-defined thing to ask for.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the clock cannot be advanced.
    async fn next_time(&self) -> Result<i64, EngineError>;

    /// The newest transaction time at or before `at`.
    ///
    /// `None` means nothing had happened yet — which is a different answer
    /// from "the entity did not exist yet", and callers must not collapse the
    /// two: the first says the graph is younger than the question.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the lookup fails.
    async fn time_at(&self, at: chrono::DateTime<chrono::Utc>) -> Result<Option<i64>, EngineError>;

    /// Fold up to `batch_size` rows of write-side storage into the
    /// read-optimized store, oldest first, and report how many moved —
    /// Epic 102. A default no-op returning `0`: only a backend with a
    /// genuine read/write partition split (`PostgresTripleStore`) has
    /// anything to fold, and this trait must not force every other
    /// implementation (an in-memory test fake, a future backend with no
    /// such split) to reject a call that is simply meaningless for it.
    ///
    /// **Had no caller anywhere outside its own tests until this default
    /// was added** — found auditing `plans/EPIC-COMPLETION-PLAN.md` Phase
    /// 1.5: `PostgresTripleStore::compact` existed, was correct and
    /// tested, but nothing above the storage layer ever called it, so in a
    /// real deployment `flakes_delta` grew forever and the whole point of
    /// the partition split degraded to nothing.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the move fails.
    async fn compact(&self, batch_size: i64) -> Result<u64, EngineError> {
        let _ = batch_size;
        Ok(0)
    }

    /// The write-side partition's own backlog, for an operator deciding
    /// whether [`compact`] needs running — Epic 102. `None` for a backend
    /// with no partition split, the same "nothing to report" convention
    /// [`compact`]'s own no-op default uses.
    ///
    /// [`compact`]: TripleStore::compact
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the read fails.
    async fn partition_health(&self) -> Result<Option<PartitionHealth>, EngineError> {
        Ok(None)
    }

    /// Retracted flakes (`op: false`) with `t > since`, in no particular
    /// order — Epic 97 decision 4.4's server-tracked retraction watermark.
    /// **Never a separate log**: retractions are already durable, ordinary
    /// rows in the same append-only store [`retract_flakes`] writes to
    /// (never deleted — see its own doc comment), so this is a query over
    /// what already exists, not new state to keep in sync with it. A
    /// caller reasoning about *base* retractions specifically (rather than
    /// churn in some other named graph, such as a reasoning overlay
    /// replacing its own prior conclusions) filters the result by `cx` —
    /// this trait has no opinion on any graph's meaning.
    ///
    /// A default no-op (`Ok(vec![])`) for a backend that cannot answer
    /// this, the same convention [`compact`]/[`partition_health`] already
    /// use: a caller building incremental maintenance on top of this
    /// falls back to a full pass when nothing comes back, exactly as it
    /// already does when there is nothing to maintain against.
    ///
    /// [`retract_flakes`]: TripleStore::retract_flakes
    /// [`compact`]: TripleStore::compact
    /// [`partition_health`]: TripleStore::partition_health
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn retractions_since(&self, since: i64) -> Result<Vec<Flake>, EngineError> {
        let _ = since;
        Ok(Vec::new())
    }
}

/// [`TripleStore::partition_health`]'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionHealth {
    /// Rows waiting in the write-side partition, not yet folded into the
    /// read-optimized store.
    pub delta_rows: u64,
    /// The oldest transaction time still sitting in the delta partition —
    /// `None` when it is empty. Age, not just count: a thousand rows
    /// written a second ago is healthy; ten rows a week old is not.
    pub oldest_delta_t: Option<i64>,
}

/// A predicate's definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicateDef {
    pub namespace: u16,
    pub name: String,
    /// Which [`graph_owl_core::flake::FlakeValue`] variant objects must be.
    pub value_type: i16,
    /// `false` = at most one value per subject.
    ///
    /// Cardinality belongs to the predicate, not the writer: `dsc:name` is
    /// single-valued for everyone, and leaving it to each caller means the
    /// first one that forgets gives a table two names with nothing to say
    /// which is current.
    pub many: bool,
    /// Ships with the binary and cannot be redefined at runtime.
    pub core: bool,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("predicate {namespace}:{name} is already defined")]
    Duplicate { namespace: u16, name: String },

    /// Redefining a core predicate would not migrate the flakes already
    /// written against it — it would make every one of them unreadable.
    #[error("{namespace}:{name} is a core predicate and cannot be redefined")]
    CoreImmutable { namespace: u16, name: String },

    #[error("registry backend failed: {0}")]
    Backend(String),
}

/// Predicates definable at runtime, so an organisation can extend the
/// vocabulary without a release.
#[async_trait]
pub trait PredicateRegistry: Send + Sync {
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if `(namespace, name)` already exists;
    /// [`RegistryError::CoreImmutable`] if it names a core predicate.
    async fn define(&self, definition: &PredicateDef) -> Result<(), RegistryError>;

    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the lookup fails.
    async fn lookup(
        &self,
        namespace: u16,
        name: &str,
    ) -> Result<Option<PredicateDef>, RegistryError>;

    /// Every definition, or those in one namespace.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn list(&self, namespace: Option<u16>) -> Result<Vec<PredicateDef>, RegistryError>;
}

/// A namespace a deployment declared, rather than one the binary ships.
///
/// `camelCase` on the wire like every other type this project serializes.
/// Per-field `rename` rather than `rename_all_fields`, matching the fix
/// `CertificationStatus` already carries — utoipa 5's schema derive does not
/// read `rename_all_fields`, so a type that ever gains `ToSchema` would ship a
/// contract saying `declared_by` while the wire says `declaredBy`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamespaceDef {
    /// The stored half of a [`Sid`]. Always `>= namespace::RUNTIME_START`.
    pub code: u16,
    /// The IRI prefix the code stands for.
    pub iri: String,
    /// Which pack or operator declared it — provenance, not ownership. A
    /// namespace outlives whatever introduced it, because its flakes stay
    /// readable after that pack is removed.
    #[serde(rename = "declaredBy")]
    pub declared_by: String,
}

/// Namespaces definable at runtime, so a domain can bring its own vocabulary
/// without a release.
///
/// The sibling of [`PredicateRegistry`], and it closes that trait's own
/// limitation: predicates were always runtime-definable, but only ever inside
/// a namespace the binary already knew, because
/// [`Sid::from_iri`](graph_owl_core::flake::Sid::from_iri) scans a fixed
/// compile-time array. A vocabulary in any other namespace could not become a
/// graph term at all — which is why three medical namespaces ended up as Rust
/// constants in `graph-owl-core` rather than as rows.
///
/// **Declared, never inferred.** There is deliberately no "register whatever
/// namespaces this document mentions" call: a malformed import would then mint
/// namespaces silently, and a typo'd prefix would become a permanent code
/// nobody chose.
#[async_trait]
pub trait NamespaceRegistry: Send + Sync {
    /// Claim `code` for `iri`.
    ///
    /// Re-declaring an identical pair succeeds and changes nothing, because
    /// reloading a pack must be safe to repeat.
    ///
    /// # Errors
    ///
    /// [`RegistryError::CoreImmutable`] if the code is below
    /// `namespace::RUNTIME_START` — that range belongs to the binary, and the
    /// same reasoning applies as to a core predicate: flakes already written
    /// against it would not migrate, they would simply change meaning.
    /// [`RegistryError::Duplicate`] if the code already means a different IRI,
    /// or the IRI already has a different code.
    async fn declare(&self, definition: &NamespaceDef) -> Result<(), RegistryError>;

    /// Every declared namespace, for building a resolver at startup.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn namespaces(&self) -> Result<Vec<NamespaceDef>, RegistryError>;

    /// The next free code at or above `namespace::RUNTIME_START`.
    ///
    /// Allocation is monotonic: a code, once assigned, is never handed out
    /// again even if its namespace is later abandoned, because flakes carrying
    /// it are still readable and would silently change meaning.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn next_code(&self) -> Result<u16, RegistryError>;
}

/// One evidence binding a finding rule extracts from its query — Epic 105
/// P5b (`plans/105b-native-reconcile-engine.md`).
///
/// `predicate` is written out rather than inferred from `var`, because a
/// SPARQL variable is named for whoever reads the query and a predicate is
/// named by the ontology — a runtime that guessed would file evidence citing
/// a predicate that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBinding {
    pub predicate: String,
    pub var: String,
}

/// A pack's `[[findings]]` rule, registered so the native reconcile engine
/// can evaluate it without parsing a pack manifest itself — the same posture
/// [`NamespaceRegistry`] and [`PredicateRegistry`] already take toward pack
/// configuration.
///
/// **`query` carries the SPARQL text, not a file path.** The pack loader
/// reads `packs/<id>/queries/<name>.sparql` at install time and inlines it
/// here; this registry, like its two siblings, never touches the filesystem
/// or a manifest.
///
/// **`similarity` and `span` stay opaque JSON rather than typed fields.**
/// They are pack-authored configuration, evaluated only at reconcile time by
/// `graph_owl_resolution::rule_match`, which is where their real shape
/// belongs — typing them a second time here would be a second definition
/// that could drift from the one that actually runs it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingRuleDef {
    pub pack: String,
    pub label: String,
    pub summary: String,
    pub governed_by: String,
    pub query: String,
    pub subject_var: String,
    pub evidence: Vec<EvidenceBinding>,
    pub similarity: Option<serde_json::Value>,
    pub span: Option<serde_json::Value>,
    /// How this rule ranks against a pack's other rules when more than one
    /// fires on the same subject — Epic 105 P10
    /// (`plans/119-architecture-audit.md` §10). Lower ranks more
    /// actionable. Pack-authored (`[[findings]]`'s own `priority` key),
    /// copied onto every [`Finding`](graph_owl_core::finding::Finding) the
    /// rule produces, the same way `summary`/`governed_by` already are.
    ///
    /// **Exists for a consumer that must collapse several findings on one
    /// subject into a single decision** — reco-now's one-row-per-invoice
    /// table is the motivating case: an invoice both filed-but-absent-
    /// from-2B and genuinely mismatched against what was filed needs one
    /// of the two to win, and that ranking is a property of the finding
    /// *kind*, not something each consumer should hardcode a table of
    /// labels to re-derive. graph-owl's own console has no such
    /// constraint — every finding is its own row there — so this is
    /// read, never required.
    ///
    /// `None` when a rule declares none, treated as least urgent by any
    /// consumer that ranks by it — a declared priority always outranks an
    /// undeclared one.
    #[serde(default)]
    pub priority: Option<i16>,
    /// Classes this rule cannot conclude anything without — IRIs, checked for
    /// at least one instance before the rule runs.
    ///
    /// Exists so that "ran, found nothing" and "could not run" stop being the
    /// same answer. `reconcile_pack` reported only a count of rules evaluated,
    /// so a rule whose input data was absent looked exactly like a rule that
    /// checked and was satisfied — on a compliance screen, "no issues" for
    /// both. They are opposite claims.
    ///
    /// **Declared by the pack, never inferred from the query.** Inference is
    /// cheaper and wrong in the case that matters: a rule may mention a class
    /// inside `OPTIONAL`, where the class being absent is precisely what the
    /// rule detects rather than something that stops it. Only the author knows
    /// which inputs are load-bearing.
    ///
    /// Empty for the great majority of rules, which read only what any
    /// reconciliation already has.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// Finding rules definable at runtime, so a pack's reconciliation logic
/// lives as registered configuration rather than a manifest the engine has
/// to parse.
///
/// **Upsert on `(pack, label)`, not idempotent-or-reject like
/// [`NamespaceRegistry`]/[`PredicateRegistry`].** A finding rule carries no
/// stored artifact that a changed query would invalidate — unlike a
/// namespace code or a predicate's value type, nothing else in the graph is
/// keyed to a rule's current text. So reloading a pack whose author edited a
/// query is a normal update, not a conflict.
#[async_trait]
pub trait FindingRuleRegistry: Send + Sync {
    /// Register a rule, replacing any existing rule with the same
    /// `(pack, label)`.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the write fails.
    async fn declare(&self, rule: &FindingRuleDef) -> Result<(), RegistryError>;

    /// Every rule registered for one pack, for a reconcile run to evaluate.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn for_pack(&self, pack: &str) -> Result<Vec<FindingRuleDef>, RegistryError>;
}

/// A pack's `[[queries]]` entry, registered so it can be invoked by name
/// with runtime bindings — Epic 105 P106 Slice 4a (`plans/
/// 106-agent-trace-hygiene.md`). The fourth sibling in this registry
/// shape, deliberately smaller than [`FindingRuleDef`]: a named query is a
/// neutral lookup, not a detector, so it carries none of a finding's
/// `summary`/`governed_by`/`subject_var`/`evidence` — reusing that shape
/// here would force those columns to mean nothing for every row of this
/// kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackQueryDef {
    pub pack: String,
    pub name: String,
    pub query: String,
}

/// Named, parameterized queries definable at runtime — a pack's `query`
/// text, addressable by `(pack, name)` without the server ever reading a
/// pack manifest.
///
/// **Upsert on `(pack, name)`**, for the identical reason
/// [`FindingRuleRegistry`] upserts on `(pack, label)`: nothing else in the
/// graph is keyed to a query's current text.
#[async_trait]
pub trait PackQueryRegistry: Send + Sync {
    /// Register a query, replacing any existing one with the same
    /// `(pack, name)`.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the write fails.
    async fn declare_query(&self, def: &PackQueryDef) -> Result<(), RegistryError>;

    /// The query text registered for `(pack, name)`, if any.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn pack_query(&self, pack: &str, name: &str) -> Result<Option<String>, RegistryError>;
}

/// Rejects a flake whose subject, predicate, graph or reference object carries
/// a namespace that was never set.
///
/// Lives here rather than in the adapter so every backend refuses the same
/// rows: an adapter-local check makes validity a property of which backend you
/// happen to be running.
///
/// # Errors
///
/// [`EngineError::UnsetNamespace`] naming the offending position.
pub fn reject_unset_namespaces(flakes: &[Flake]) -> Result<(), EngineError> {
    for flake in flakes {
        check(&flake.s, "subject")?;
        check(&flake.p, "predicate")?;
        if let Some(cx) = &flake.cx {
            check(cx, "graph")?;
        }
        if let FlakeValue::Ref(o) = &flake.o {
            check(o, "object")?;
        }
    }
    Ok(())
}

/// Rejects a flake naming a predicate that is not in `registered`.
///
/// Takes the known map rather than the registry itself so it stays pure and
/// synchronous: the adapter owns *how* the set is obtained and cached, this
/// owns what makes a batch acceptable. Lives here for the same reason
/// [`reject_unset_namespaces`] does — every backend must refuse the same rows.
///
/// Only the predicate position is checked. Subjects and objects are data:
/// `dsc:table-upi-transactions` is an entity nobody will ever define, and
/// reaching into those positions would reject the whole catalog.
///
/// # Errors
///
/// [`EngineError::UnregisteredPredicate`] naming the first one not defined.
pub fn reject_unregistered_predicates<S: std::hash::BuildHasher>(
    flakes: &[Flake],
    registered: &std::collections::HashMap<Sid, PredicateDef, S>,
) -> Result<(), EngineError> {
    for flake in flakes {
        if !registered.contains_key(&flake.p) {
            return Err(EngineError::UnregisteredPredicate {
                namespace: flake.p.namespace_code,
                name: flake.p.id.clone(),
            });
        }
    }
    Ok(())
}

/// Rejects a flake whose object is not the `FlakeValue` variant its
/// predicate is registered to carry.
///
/// A flake naming an unregistered predicate is not reported here a second
/// time — [`reject_unregistered_predicates`] already refuses it, and a
/// predicate absent from `registered` has no declared type to check against.
///
/// # Errors
///
/// [`EngineError::WrongValueType`] naming the first mismatch.
pub fn reject_wrong_datatypes<S: std::hash::BuildHasher>(
    flakes: &[Flake],
    registered: &std::collections::HashMap<Sid, PredicateDef, S>,
) -> Result<(), EngineError> {
    for flake in flakes {
        let Some(def) = registered.get(&flake.p) else {
            continue;
        };
        let actual = flake.o.value_type();
        if actual != def.value_type {
            return Err(EngineError::WrongValueType {
                namespace: flake.p.namespace_code,
                name: flake.p.id.clone(),
                expected: def.value_type,
                actual,
            });
        }
    }
    Ok(())
}

/// Rejects a batch that asserts two *different* values for the same
/// (subject, predicate) on a predicate registered `many = false`.
///
/// **Scoped to this batch only** — see [`EngineError::CardinalityViolation`]
/// for why checking against the store's current state is a separate, larger
/// obligation this function does not attempt. The *same* value repeated
/// twice is not a violation: idempotent re-assertion is ordinary, and
/// refusing it would make a caller that retries a partially-failed batch
/// worse off than one that never retried.
///
/// # Errors
///
/// [`EngineError::CardinalityViolation`] naming the first subject/predicate
/// pair asserting more than one distinct value.
pub fn reject_cardinality_violations<S: std::hash::BuildHasher>(
    flakes: &[Flake],
    registered: &std::collections::HashMap<Sid, PredicateDef, S>,
) -> Result<(), EngineError> {
    let mut seen: std::collections::HashMap<(&Sid, &Sid), &FlakeValue> =
        std::collections::HashMap::new();
    for flake in flakes {
        let Some(def) = registered.get(&flake.p) else {
            continue;
        };
        if def.many {
            continue;
        }
        match seen.entry((&flake.s, &flake.p)) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(&flake.o);
            }
            std::collections::hash_map::Entry::Occupied(slot) => {
                if *slot.get() != &flake.o {
                    return Err(EngineError::CardinalityViolation {
                        namespace: flake.p.namespace_code,
                        name: flake.p.id.clone(),
                        subject: flake.s.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn check(sid: &Sid, position: &'static str) -> Result<(), EngineError> {
    if sid.namespace_code == namespace::UNSET {
        return Err(EngineError::UnsetNamespace { position });
    }
    Ok(())
}

#[cfg(test)]
mod unregistered_predicate_tests {
    use super::*;
    use std::collections::HashMap;

    fn def() -> PredicateDef {
        PredicateDef {
            namespace: namespace::DSC,
            name: String::new(),
            value_type: 1,
            many: false,
            core: true,
        }
    }

    fn registered() -> HashMap<Sid, PredicateDef> {
        [Sid::dsc("name"), Sid::dsc("fqn")]
            .into_iter()
            .map(|sid| (sid, def()))
            .collect()
    }

    fn about(predicate: Sid) -> Flake {
        Flake::assert(
            Sid::dsc("table-upi-transactions"),
            predicate,
            FlakeValue::String("upi_transactions".into()),
            1,
        )
    }

    #[test]
    fn a_registered_predicate_is_accepted() {
        assert!(reject_unregistered_predicates(&[about(Sid::dsc("name"))], &registered()).is_ok());
    }

    /// Naming it is the whole point. "Unknown predicate" tells an operator
    /// nothing they can act on; `1:rbiCircular` tells them exactly what to
    /// define.
    #[test]
    fn an_unregistered_predicate_is_refused_and_named() {
        let error =
            reject_unregistered_predicates(&[about(Sid::dsc("rbiCircular"))], &registered())
                .expect_err("an undefined predicate must be refused");

        assert!(
            matches!(
                &error,
                EngineError::UnregisteredPredicate { namespace, name }
                    if *namespace == namespace::DSC && name == "rbiCircular"
            ),
            "the error must carry both halves of the identity: {error:?}"
        );
        assert!(
            error.to_string().contains("rbiCircular"),
            "and say it out loud: {error}"
        );
    }

    /// `dsc:type` and `rdf:type` are different predicates that differ only in
    /// namespace. A check that compared names alone would accept an
    /// unregistered predicate whenever *any* namespace had defined that name.
    #[test]
    fn the_namespace_is_half_of_the_predicate_identity() {
        let dsc_type: HashMap<Sid, PredicateDef> = [Sid::dsc("type")]
            .into_iter()
            .map(|sid| (sid, def()))
            .collect();

        let error =
            reject_unregistered_predicates(&[about(Sid::new(namespace::RDF, "type"))], &dsc_type)
                .expect_err("rdf:type is not dsc:type");
        assert!(
            matches!(&error, EngineError::UnregisteredPredicate { namespace, .. } if *namespace == namespace::RDF),
            "got {error:?}"
        );
    }

    /// A batch is written as one statement, so one undefined predicate
    /// anywhere in it poisons the whole write — the scan cannot stop at the
    /// first flake.
    #[test]
    fn an_unregistered_predicate_later_in_a_batch_is_still_caught() {
        let batch = [
            about(Sid::dsc("name")),
            about(Sid::dsc("fqn")),
            about(Sid::dsc("rbiCircular")),
        ];
        assert!(reject_unregistered_predicates(&batch, &registered()).is_err());
    }

    /// Subjects and objects are *data*. `dsc:table-upi-transactions` is an
    /// entity, not vocabulary, and nothing will ever define it — a check that
    /// reached into those positions would reject every flake in the catalog.
    #[test]
    fn only_the_predicate_position_is_vocabulary() {
        let flake = Flake::assert(
            Sid::dsc("some-entity-nobody-defined"),
            Sid::dsc("name"),
            FlakeValue::Ref(Sid::dsc("another-entity-nobody-defined")),
            1,
        );
        assert!(reject_unregistered_predicates(&[flake], &registered()).is_ok());
    }

    /// Two undefined predicates in one batch report the *first*, so the
    /// message an operator sees is stable across runs rather than depending on
    /// iteration order.
    #[test]
    fn the_first_unregistered_predicate_is_the_one_reported() {
        let batch = [
            about(Sid::dsc("rbiCircular")),
            about(Sid::dsc("dataResidency")),
        ];
        let error =
            reject_unregistered_predicates(&batch, &registered()).expect_err("must be refused");
        assert!(
            matches!(&error, EngineError::UnregisteredPredicate { name, .. } if name == "rbiCircular"),
            "got {error:?}"
        );
    }

    #[test]
    fn an_empty_batch_is_vacuously_valid() {
        assert!(reject_unregistered_predicates(&[], &registered()).is_ok());
    }

    /// An empty registry is not a licence to write anything. It is what a
    /// database with no migrations looks like, and asserting into it should
    /// fail loudly rather than fill the graph with unreadable rows.
    #[test]
    fn an_empty_registry_admits_nothing() {
        assert!(
            reject_unregistered_predicates(&[about(Sid::dsc("name"))], &HashMap::new()).is_err()
        );
    }
}

#[cfg(test)]
mod wrong_datatype_tests {
    use super::*;
    use std::collections::HashMap;

    fn registered() -> HashMap<Sid, PredicateDef> {
        [
            (Sid::dsc("confidence"), 4), // float
            (Sid::dsc("name"), 1),       // string
            (Sid::dsc("owner"), 0),      // ref
        ]
        .into_iter()
        .map(|(sid, value_type)| {
            (
                sid,
                PredicateDef {
                    namespace: namespace::DSC,
                    name: String::new(),
                    value_type,
                    many: false,
                    core: true,
                },
            )
        })
        .collect()
    }

    #[test]
    fn a_value_matching_its_predicates_type_is_accepted() {
        let flake = Flake::assert(
            Sid::dsc("orders"),
            Sid::dsc("confidence"),
            FlakeValue::Float(0.9),
            1,
        );
        assert!(reject_wrong_datatypes(&[flake], &registered()).is_ok());
    }

    /// **The real failure mode this check exists for**: a `String` written
    /// where every reader assumes a `Float` looks like an ordinary row until
    /// something tries to use it, far from where the mistake happened.
    #[test]
    fn a_string_where_a_float_is_registered_is_refused_naming_both_types() {
        let flake = Flake::assert(
            Sid::dsc("orders"),
            Sid::dsc("confidence"),
            FlakeValue::String("high".into()),
            1,
        );

        let error = reject_wrong_datatypes(&[flake], &registered())
            .expect_err("a String is not the registered Float");

        assert!(
            matches!(
                &error,
                EngineError::WrongValueType { name, expected, actual, .. }
                    if name == "confidence" && *expected == 4 && *actual == 1
            ),
            "got {error:?}"
        );
    }

    /// And the negative half of `a_value_matching_its_predicates_type_is_accepted`:
    /// a `Ref` is not a `String`, even though both are common object shapes.
    #[test]
    fn a_ref_where_a_string_is_registered_is_refused() {
        let flake = Flake::assert(
            Sid::dsc("orders"),
            Sid::dsc("name"),
            FlakeValue::Ref(Sid::dsc("not-a-name")),
            1,
        );
        assert!(reject_wrong_datatypes(&[flake], &registered()).is_err());
    }

    /// An unregistered predicate has no declared type to check against — that
    /// is [`reject_unregistered_predicates`]'s failure to report, not this
    /// one's, and reporting it here too would name the wrong problem.
    #[test]
    fn an_unregistered_predicate_is_not_reported_as_a_wrong_type() {
        let flake = Flake::assert(
            Sid::dsc("orders"),
            Sid::dsc("rbiCircular"),
            FlakeValue::String("anything".into()),
            1,
        );
        assert!(reject_wrong_datatypes(&[flake], &registered()).is_ok());
    }

    #[test]
    fn a_ref_object_matching_a_ref_predicate_is_accepted() {
        let flake = Flake::assert(
            Sid::dsc("orders"),
            Sid::dsc("owner"),
            FlakeValue::Ref(Sid::dsc("ops-team")),
            1,
        );
        assert!(reject_wrong_datatypes(&[flake], &registered()).is_ok());
    }
}

#[cfg(test)]
mod cardinality_violation_tests {
    use super::*;
    use std::collections::HashMap;

    fn registered() -> HashMap<Sid, PredicateDef> {
        [
            (Sid::dsc("name"), false), // single-valued
            (Sid::dsc("owner"), true), // many-valued
        ]
        .into_iter()
        .map(|(sid, many)| {
            (
                sid,
                PredicateDef {
                    namespace: namespace::DSC,
                    name: String::new(),
                    value_type: 1,
                    many,
                    core: true,
                },
            )
        })
        .collect()
    }

    fn named(subject: &str, value: &str) -> Flake {
        Flake::assert(
            Sid::dsc(subject),
            Sid::dsc("name"),
            FlakeValue::String(value.into()),
            1,
        )
    }

    #[test]
    fn one_value_for_a_single_valued_predicate_is_accepted() {
        assert!(reject_cardinality_violations(&[named("orders", "Orders")], &registered()).is_ok());
    }

    /// **The failure mode this check exists for**: a batch that asserts two
    /// different names for the same table would leave "which one is current"
    /// unanswerable, with nothing in the write path ever having refused it.
    #[test]
    fn two_different_values_for_one_subject_are_refused_naming_the_subject() {
        let batch = [named("orders", "Orders"), named("orders", "OrdersRenamed")];

        let error = reject_cardinality_violations(&batch, &registered())
            .expect_err("two different names for one subject must be refused");

        assert!(
            matches!(
                &error,
                EngineError::CardinalityViolation { name, subject, .. }
                    if name == "name" && subject == &Sid::dsc("orders").to_string()
            ),
            "got {error:?}"
        );
    }

    /// **Idempotent re-assertion is not a violation.** A retried batch that
    /// happens to repeat the same value must not be worse off than one that
    /// never retried.
    #[test]
    fn the_same_value_repeated_is_not_a_violation() {
        let batch = [named("orders", "Orders"), named("orders", "Orders")];
        assert!(reject_cardinality_violations(&batch, &registered()).is_ok());
    }

    /// A many-valued predicate is exactly what several values per subject
    /// look like — the whole reason the registry marks some predicates this
    /// way rather than refusing every repeat.
    #[test]
    fn several_values_for_a_many_valued_predicate_are_accepted() {
        let batch = [
            Flake::assert(
                Sid::dsc("orders"),
                Sid::dsc("owner"),
                FlakeValue::Ref(Sid::dsc("ops-team")),
                1,
            ),
            Flake::assert(
                Sid::dsc("orders"),
                Sid::dsc("owner"),
                FlakeValue::Ref(Sid::dsc("data-team")),
                1,
            ),
        ];
        assert!(reject_cardinality_violations(&batch, &registered()).is_ok());
    }

    /// Two *different subjects* asserting the same single-valued predicate is
    /// not a clash — cardinality is per subject, not per predicate.
    #[test]
    fn the_same_predicate_on_different_subjects_is_not_a_clash() {
        let batch = [named("orders", "Orders"), named("returns", "Returns")];
        assert!(reject_cardinality_violations(&batch, &registered()).is_ok());
    }

    #[test]
    fn an_unregistered_predicate_is_not_reported_as_a_cardinality_violation() {
        let batch = [
            Flake::assert(
                Sid::dsc("orders"),
                Sid::dsc("rbiCircular"),
                FlakeValue::String("a".into()),
                1,
            ),
            Flake::assert(
                Sid::dsc("orders"),
                Sid::dsc("rbiCircular"),
                FlakeValue::String("b".into()),
                1,
            ),
        ];
        assert!(reject_cardinality_violations(&batch, &registered()).is_ok());
    }
}

#[cfg(test)]
mod unset_namespace_tests {
    use super::*;

    fn valid() -> Flake {
        Flake::assert(
            Sid::dsc("table-1"),
            Sid::dsc("name"),
            FlakeValue::String("upi_transactions".into()),
            1,
        )
    }

    #[test]
    fn a_fully_initialized_flake_is_accepted() {
        assert!(reject_unset_namespaces(&[valid()]).is_ok());
    }

    /// Each position is checked separately, because a check that only covers
    /// the subject lets an uninitialized predicate through — and an
    /// uninitialized predicate makes the flake unqueryable rather than merely
    /// wrong.
    #[test]
    fn every_sid_position_is_checked() {
        let cases = [
            (
                "subject",
                Flake {
                    s: Sid::new(namespace::UNSET, "x"),
                    ..valid()
                },
            ),
            (
                "predicate",
                Flake {
                    p: Sid::new(namespace::UNSET, "x"),
                    ..valid()
                },
            ),
            (
                "graph",
                Flake {
                    cx: Some(Sid::new(namespace::UNSET, "x")),
                    ..valid()
                },
            ),
            (
                "object",
                Flake {
                    o: FlakeValue::Ref(Sid::new(namespace::UNSET, "x")),
                    ..valid()
                },
            ),
        ];
        for (position, flake) in cases {
            let error =
                reject_unset_namespaces(&[flake]).expect_err("an unset namespace must be refused");
            assert!(
                matches!(&error, EngineError::UnsetNamespace { position: p } if *p == position),
                "expected position {position}, got {error:?}"
            );
        }
    }

    /// A literal object has no namespace to check. Reaching into it anyway
    /// would reject every string-valued flake in the catalog.
    #[test]
    fn a_literal_object_carries_no_namespace_to_reject() {
        let flake = Flake {
            o: FlakeValue::String(String::new()),
            ..valid()
        };
        assert!(reject_unset_namespaces(&[flake]).is_ok());
    }

    /// The default graph is `None`, not namespace 0. Confusing the two would
    /// reject every flake in the default graph, which is nearly all of them.
    #[test]
    fn the_default_graph_is_absence_not_an_unset_namespace() {
        let flake = Flake {
            cx: None,
            ..valid()
        };
        assert!(reject_unset_namespaces(&[flake]).is_ok());
    }

    /// The scan must not stop at the first flake — a batch is written as one
    /// statement, so one bad flake anywhere in it poisons the whole write.
    #[test]
    fn a_bad_flake_later_in_a_batch_is_still_caught() {
        let batch = [
            valid(),
            valid(),
            Flake {
                p: Sid::new(namespace::UNSET, "x"),
                ..valid()
            },
        ];
        assert!(reject_unset_namespaces(&batch).is_err());
    }

    #[test]
    fn an_empty_batch_is_vacuously_valid() {
        assert!(reject_unset_namespaces(&[]).is_ok());
    }

    #[test]
    fn the_error_names_the_position_so_the_bad_field_is_findable() {
        let error = reject_unset_namespaces(&[Flake {
            p: Sid::new(namespace::UNSET, "x"),
            ..valid()
        }])
        .expect_err("must reject");
        assert!(error.to_string().contains("predicate"), "got {error}");
    }
}
