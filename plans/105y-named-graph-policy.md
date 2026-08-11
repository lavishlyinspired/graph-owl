# Plan: per-named-graph policy — the domain-neutrality follow-up

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing the same completion pass as
`105u`/`105v`/`105w`/`105x`.
**Crates**: `graph-owl-authz` (`ResourceMatcher::NamedGraph`,
`AccessPredicate::NamedGraph`, `compile_named_graph`), `graph-owl-api`
(`scope_facts`'s vocabulary-namespace admission gains a named-graph check),
`graph-owl-storage-postgres` (`lower()`'s exhaustiveness only — a
`NamedGraph` predicate never reaches SQL). No changes to `graph-owl-core`,
`graph-owl-server`, or any pack config.

## The gap, stated by the document that found it

`plans/105-domain-neutrality.md`'s own closing line on this: "a pack's
facts are now readable by any principal who can query... it is not the
right long-term answer for a pack carrying real invoices. **Per-named-graph
policy**, so access to `graph:import:{source}` is a policy decision rather
than a namespace one, is the follow-up. It needs a policy model that does
not exist and is a deliberate design decision, not something to infer."

Concretely: `scope_facts` (`graph-owl-api`) admits a flake whose subject
sits in a runtime pack namespace (`>= RUNTIME_START`) unconditionally, the
same way it already admits the three hardcoded medical namespaces
(CUI/SNOMED/RxNorm, Epic 104). That was correct for vocabulary content —
never a catalog asset, never carrying its own access policy — but a pack's
imported rows (`Catalog::import_rdf`, landing in `graph:import:{source}`)
are exactly the kind of principal-scoped data every other write path in
this system authorizes. Nothing before this slice checked that.

## What was built

**Two new, parallel pieces in `graph-owl-authz`**, deliberately kept
separate from the FQN-scoped ones rather than reused:

- `ResourceMatcher::NamedGraph(String)` — the named-graph counterpart of
  `ResourceMatcher::FqnPrefix`. `ResourceMatcher::matches()` (the row-level
  `Resource` check) returns `false` for it: a named graph is not a
  relational asset, so per-resource matching does not apply to it, the
  same way `Tagged` already returns `false` for a check it cannot answer.
- `AccessPredicate::NamedGraph { allow_prefixes, deny_prefixes }` — same
  shape as `AccessPredicate::Fqn`, a separate variant so a caller cannot
  check an FQN-scoped predicate against a named-graph identifier (or the
  reverse) and have it silently compile. `admits()` handles both variants
  in one combined match arm, since the prefix logic is identical; only the
  *type* differs, which is the whole point of splitting it.
- `compile()` refactored into a shared `compile_prefixes(subject,
  operation, policies, dimension: PrefixDimension)`, where
  `PrefixDimension::{Fqn, NamedGraph}` determines which `ResourceMatcher`
  variant contributes a prefix (`prefix_of()`) and which `AccessPredicate`
  variant wraps the result (`wrap()`). `compile_named_graph()` is the new
  public entry point; `compile()` itself is now one line delegating with
  `PrefixDimension::Fqn`, so its own extensive existing test suite is the
  regression check that the refactor changed nothing about FQN compilation.

**`ResourceMatcher::All` grants both dimensions** (matching its own name —
an unqualified "allow everything" rule was never meant to be FQN-only), and
**`Tagged` contributes to neither** (the existing safe default: a
row-level predicate cannot resolve tags, so a `Tagged` rule is silently
dropped from compilation rather than treated as unrestricted on either
axis — unchanged behaviour, now proven for the new dimension too).

**In `graph-owl-api`**: `is_vocabulary_namespace` split into
`is_fixed_vocabulary_namespace` (the three medical codes — unconditional,
unchanged, since nothing imports medical data into a named graph the way a
pack's own facts do) and `is_runtime_pack_namespace` (`>= RUNTIME_START`).
A new `admitted_by_vocabulary_namespace(namespace_code, cx,
named_graph_predicate)` combines them: the fixed namespaces still pass
unconditionally; a runtime pack namespace additionally requires — *when
the flake has a `cx`* — that `named_graph_predicate.admits(&cx.id)`. A
flake with no `cx` (not import-sourced) is unaffected, matching every
prior test's existing behaviour. This one helper is called at both sites
`scope_facts` already checked vocabulary namespaces: endpoint visibility
(`fromEntity`/`toEntity`/`alignmentLeft`/`alignmentRight`) and subject
visibility.

`Catalog` gained `named_graph_predicate_for(principal, operation)` —
**deliberately uncached**, unlike the FQN `predicate_for`. The FQN
predicate is cached because it is compiled from `policies_for` on nearly
every read path this crate has; the named-graph predicate is new and has
exactly one caller (`scoped_facts`) so far. Caching a predicate no one
else reads yet would be speculative machinery paying for a hit rate of
one. `Catalog::scoped_facts()` fetches it alongside the existing FQN
`predicate`/`visible` computation and passes it into `scope_facts()`,
whose signature grew a fourth parameter (`named_graph_predicate:
&AccessPredicate`) — a real, deliberate breaking change to every existing
caller of that free function (all in-crate tests), fixed by passing
`&AccessPredicate::All` at each pre-existing call site, since none of
those tests concern named-graph policy.

**In `graph-owl-storage-postgres`**: `lower()` (the FQN-predicate-to-SQL
lowering used by relational asset queries) gained an
`AccessPredicate::NamedGraph { .. } => None` arm for exhaustiveness,
combined with the existing `Nothing => None` arm since the bodies are
identical. `None` (fail closed) rather than `unreachable!()` (a caller bug
would panic and take down a request) or a real lowering (SQL never sees
named-graph identifiers — the FQN column this function targets has
nothing to do with them, so any lowering here would be meaningless at
best): a `NamedGraph` predicate reaching this function is *always* a
caller bug, since `compile_named_graph`'s own predicate is checked
directly in Rust (`scope_facts`), never through SQL.

## `Principal::system().is_admin == true` — why almost nothing else changed

Both `compile()` and `compile_named_graph()` short-circuit `if
subject.is_admin { return AccessPredicate::All }` before doing anything
else. The overwhelming majority of this crate's existing tests build their
principal via `Principal::system()`, which is always `is_admin: true` — so
for every one of them, `named_graph_predicate_for` now also resolves to
`AccessPredicate::All` and `admitted_by_vocabulary_namespace` behaves
exactly as it did before this slice (`cx.is_none_or(|g|
AccessPredicate::All.admits(&g.id))` is unconditionally `true`). Verified
by running the full pre-existing `graph-owl-api --lib` suite after the
change: all passed, zero new failures. Only a *non-admin* principal
querying *import-sourced* pack data is newly restricted — exactly the gap
`105-domain-neutrality.md` named.

## The RED tests, and the debugging journey that found a second, unrelated bug

Two new tests in `graph-owl-api`, both against a real `RecordingGraph` and
a real `InMemoryStorage` (not a hand-rigged fixture — the same "prove
against the real adapter" precedent this crate's other end-to-end tests
already established):

1. `imported_pack_data_needs_an_explicit_named_graph_grant_for_a_non_admin_principal`
   — seeds one flake with `cx: Some(Sid::dsc("graph:import:gst-purchase-register"))`.
   A non-admin principal's query returns nothing before any grant, then
   one row after `upsert_policy` attaches a `NamedGraph("graph:import:gst")`
   allow rule to the principal's role. Admin visibility (`Principal::system()`)
   is asserted unaffected throughout.
2. `a_pack_fact_with_no_named_graph_is_unaffected_by_named_graph_policy` —
   the scope boundary stated as a test: a pack fact with `cx: None` (never
   imported, declared directly) is visible to the same non-admin principal
   with **no** grant at all, because there is nothing to check it against.

**The first test failed for a reason that had nothing to do with
authorization**, and finding that was most of this slice's actual time.
The initial query was a plain `SELECT ?v WHERE { ?s <predicate> ?v }`.
`facts_scanned` on the "after grant" query read `1` — the flake *was*
passing `scope_facts` correctly — but `rows` still came back empty.
`plans/105-domain-neutrality.md` already documents exactly why, from the
pack-loading work that shipped earlier in this epic: **"every pattern must
be inside `GRAPH ?g` — imports land in `graph:import:{source}`, never the
default graph, so a query without a `GRAPH` clause matches nothing —
silently."** `FlakeDataset::from_flakes` places a flake with a `cx` into a
genuinely *named* RDF graph (`graph_owl_query::dataset`'s own
`internal_quads_for_pattern` deliberately treats an unbound `GRAPH`
variable as "any named graph, but never the default" — the comment there
calls out getting this backwards as a spec violation), and a bare `SELECT`
with no `GRAPH` clause only ever queries the default graph. The test's
query was wrong for the data it seeded, independent of policy — the same
class of self-inflicted RED-test bug `105v`'s `as_of` interaction test hit
earlier in this pass. Fixed by rewriting the query as `SELECT ?v WHERE {
GRAPH ?g { ?s <predicate> ?v } }`, which `graph_owl_query::pushdown`
already handles (`scans_for` descends into `GraphPattern::Graph`).

## Mutation report

**`graph-owl-authz/src/lib.rs`**, `--in-diff`, `--lib`: 8 new tests added
mirroring `compile()`'s own existing suite one-for-one (admin bypass,
no-policy-denies-not-all, prefix-allow, cross-dimension leak prevention in
both directions, blanket-deny, `All`-grants-both-dimensions). Full crate
suite: 71 passed (63 pre-existing + 8 new), 0 failed.

**`graph-owl-api/src/lib.rs`**, `--in-diff`: full `--lib` suite 673
passed, 0 failed, after fixing the `GRAPH ?g` query-construction bug above.

**`graph-owl-storage-postgres/src/lib.rs`**: `lower()` has no unit-test
coverage of its own — every caller (`list_assets_visible`,
`search_assets_visible`, and four siblings) is exercised only through
Postgres-backed integration tests, so the default `--lib`-scoped run
(`scripts/mutants.sh`'s hardcoded fast path) reported all 10 mutants
MISSED — the same coverage-shape blind spot this project's own
`observability.rs` precedent already documents, not a real gap. Re-run
without `--lib`, scoped to the one integration binary that exercises
`lower()` (`--cargo-test-arg --test asset_owners`, `--test-threads=1` to
avoid the documented `PortNotExposed` contention): **10 mutants tested,
10 caught, 0 missed.**

## What this deliberately does not do

- **Does not change how imports are written.** `Catalog::import_rdf`
  still lands every triple in `graph:import:{source}` unconditionally, per
  `plans/105-domain-neutrality.md`'s own design (`import_graph`'s own
  admin-only gate at the route is the write-time control). This slice is
  read-time authorization only.
- **Does not give a pack's own `pack.toml` a way to declare its default
  named-graph grant.** A policy naming `NamedGraph("graph:import:gst")`
  is created the same way any other policy is — through
  `storage.upsert_policy`, by an administrator — not auto-derived from a
  pack's own name at load time. Auto-granting on load would mean every
  imported pack is world-readable by construction, which is the exact
  posture this slice exists to end.
- **Does not retrofit a named-graph grant onto the three medical
  namespaces.** They stay on `is_fixed_vocabulary_namespace`'s
  unconditional path — nothing in this codebase imports CUI/SNOMED/RxNorm
  data into a named graph the way a pack's own facts land in
  `graph:import:{source}`, so there is no `cx` for a policy to be scoped
  against; extending that admission would be solving a problem that does
  not exist yet.
