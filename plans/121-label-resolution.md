# Plan 121 — generalized subject label resolution (Plan 120 Slice C)

**Branch**: main. **Status**: Draft, ready to execute. **Trigger**: Plan
120 §1.2 — `displayTerm()` is purely syntactic (strips an IRI to its local
name), so a GST Supplier subject renders as `supplier-27AAAFN2938K1Z2`
everywhere the console shows a bare subject: the findings queue, the
evidence-graph drawer, and the Explore/`SubjectExplorer` graph view.

## Grounding (read from the code, not assumed)

- `displayTerm()` (`ui/src/features/review/findingsQueue.tsx:95-100`) strips
  an IRI to its local name and never reads a literal. `nodeLabel()`
  (`:190-192`) is the one call site that already has the right shape to take
  a server-resolved label first: `node.iri ? displayTerm(node.iri) : node.id`.
- `finding_evidence_graph` (`graph-owl-server/src/lib.rs:9234-9286`) builds
  each node from `Catalog::node_sources` and `Catalog::node_semantic_type`
  (`graph-owl-api/src/lib.rs:4308-4332`) — the latter already resolves a
  subject's `rdf:type` local name (e.g. `"Supplier"`) by querying its own
  flakes with `TriplePattern { s: Some(sid.clone()), ..Default::default() }`
  and matching `rdf:type`. The same pattern, with `p` also set, is what a new
  literal-value lookup needs.
- `pack_install::read_console_config` (`graph-owl-server/src/pack_install.rs:218-249`)
  already reads a pack's whole `[console]` table verbatim as camelCased JSON,
  **at request time, off disk** — a `[console.labels]` addition needs zero
  Rust schema changes to become readable; `pack_console_config`
  (`graph-owl-server/src/lib.rs:8856-8863`, `GET /packs/{pack}/console`)
  already serves it.
- `Catalog::namespaces()` (`graph-owl-api/src/lib.rs:3050-3060`) returns
  `NamespaceDef { code, iri, declared_by }` (`graph-owl-engine/src/lib.rs:332-342`)
  — `declared_by` is `"pack:{id}"`. This is the missing link: a subject's own
  `namespace_code` resolves to the pack that owns it, which is what
  `[console.labels]` table to consult. No ambiguity, no merge-every-pack
  fallback needed.
- `connectors/python/graph_owl_packs/manifest.py:80` (`console: dict`) reads
  `[console]` as an untyped dict and nothing in Python validates or consumes
  it further — **no Python changes are needed for this slice**, only
  `pack.toml` edits and Rust/TS code.
- Confirmed live: a real GST supplier subject (`reco-books` upload) carries
  `a gst:Supplier ; gst:supplierGstin "..." ; gst:supplierName "Nimbus
  Freight Logistics"` (`ext-apps/Reco/backend/app/graphowl_client.py:266-270`,
  `CLASS_BY_KIND`). `node_semantic_type` on that subject returns
  `Some("Supplier")` today, already wired and already correct — only the
  *label* lookup is missing.
- Hospitality fixture (`packs/hospitality/fixtures/estate.ttl:10-26`) gives a
  second, unrelated domain to prove the mechanism against without inventing
  test data: `hosp:Property` (`hosp:name`) and `hosp:Guest`
  (`hosp:guestSurname`), and a real finding already declared over Guest
  identity (`pack.toml:72-75`, `hosp:DuplicateGuest`).

## Design

**One predicate per class, pack-declared, resolved server-side, consumed
identically wherever a bare subject renders today.** Deliberately the
simplest version the parent plan names — no multi-predicate composition, no
per-subject override; add richness only if a real need appears later
(Plan 120's own Parking Lot).

**Schema** — `[console.labels]` in `pack.toml`, keyed by the bare class local
name `node_semantic_type` already returns, valued by the predicate's own
local name in the same namespace:

```toml
[console.labels]
Supplier = "supplierName"
```

(GST) and

```toml
[console.labels]
Guest = "guestSurname"
Property = "name"
```

(hospitality). Both reuse predicates the packs already declare — no new
`[[predicates]]` entries.

**Resolution path** (new, in `graph-owl-server`, near `finding_evidence_graph`):

1. For a node with `semantic_type: Some(class)`, find the `NamespaceDef`
   whose `code == sid.namespace_code` (from `catalog.namespaces()`, fetched
   once per request, not per node).
2. Strip `"pack:"` off `declared_by` to get the pack id.
3. `pack_install::read_console_config(base_dir, pack_id)` (cached per pack id
   within the request — most evidence graphs are single-pack), read
   `["labels"][class]` as a string — the predicate's local name.
4. New `Catalog` method resolves the literal: given `sid` and
   `Sid::new(sid.namespace_code, predicate_local)`, query
   `TriplePattern { s: Some(sid.clone()), p: Some(predicate_sid), ..Default }`
   and return the first `FlakeValue::String` found, `None` otherwise — same
   shape and same "first wins, no persisted second concept" posture as
   `node_semantic_type`.
5. Any step failing (no namespace entry, no `[console.labels]`, no matching
   class, no literal on the subject) degrades to `None` — never an error,
   matching `sources`/`semanticType`'s existing degrade-not-fail posture in
   the same handler.

**Wire order** (four small, independently observable slices — see below).
Stage 1 proves the mechanism on the narrowest real screen; Stages 2–3 reuse
it on the two other places a bare subject renders today; Stage 4 proves it
isn't GST-shaped.

## Slices

Every slice: load `tdd`, `testing`, `mutation-testing`, `refactoring` before
code; RED-GREEN-MUTATE-KILL MUTANTS-REFACTOR; this project's standing
process commits directly to `main` without a separate approval pause
(`CLAUDE.md` — supersedes the generic planning skill's per-commit approval
step for this repo).

### Slice 1: A finding's evidence-graph node shows its declared label, not its bare id

**Value**: a reviewer opening a GST finding's evidence panel sees "Nimbus
Freight Logistics", not `supplier-27AAAFN2938K1Z2`.
**Path**: `GET /findings/{id}/evidence-graph` → `finding_evidence_graph` →
new label-resolution helper → `EvidenceGraphNode.label` → `nodeLabel()` in
the console.
**Acceptance criteria**:
- `[console.labels] Supplier = "supplierName"` added to `packs/gst/pack.toml`.
- A new `Catalog` method resolves a subject+predicate to its first literal
  value, unit-tested in isolation (`--lib`).
- `finding_evidence_graph`'s per-node JSON gains `"label": string | null`,
  resolved via the namespace→pack→`[console.labels]`→literal path above.
- `EvidenceGraphNode` (`ui/src/api.ts`) gains `readonly label: string | null`.
- `nodeLabel()` prefers `node.label`, falls back to today's
  `displayTerm(iri)`/`id` behavior when `null`.
- Integration test: a real Supplier subject with `gst:supplierName` set
  resolves to that literal through the live HTTP route; a subject with no
  declared label predicate (or no `[console.labels]` entry for its class)
  falls back to `null` and the console still renders the old way.
**RED**: HTTP test against `test_app()` — seed a `gst:Supplier` subject with
`gst:supplierName "Test Supplier"`, hit its finding's evidence-graph route,
assert the node's `label` is `"Test Supplier"`. Also assert a subject whose
class has no `[console.labels]` entry gets `label: null` (the negative case
— this project's mutation history says a missing negative test is the
default way a survivor gets through, per `CLAUDE.md`'s own recorded lesson).
**GREEN**: implement the resolution helper and wire it in.
**MUTATE**: `scripts/mutants.sh graph-owl-server --diff <this change>` (and
the new `Catalog` method under `--lib`, scoped to `graph-owl-api`).
**KILL MUTANTS**: address survivors; the likely one is the class-name match
(`==` weakened) or the `None`-degrade path silently swallowed.
**REFACTOR**: only if it adds value.
**Done when**: criteria met, mutation report reviewed, verified live against
the real reco-now upload (Supplier evidence-graph node shows the real name).

### Slice 2: The Explore graph view (`SubjectExplorer`) shows every node's label, not just the seed

**Value**: a reviewer expanding a subject's neighbourhood in Explore reads
names throughout the picture, not just at the node they clicked.
**Path**: `POST /graph/context` → `assemble_graph_context` (Plan 113's
shared walk, already split from `walk_authorized` so this reuses it) → same
label-resolution helper Slice 1 built → `SubjectExplorer.tsx`'s label map.
**Acceptance criteria**:
- `assemble_graph_context`'s node-building gains the same `label` field via
  the Slice 1 helper (shared function, not a second implementation).
- `SubjectExplorer.tsx:92`'s `new Map([[seed, label]])` becomes a map built
  from every node's resolved label, falling back per-node the same way
  `nodeLabel()` does.
**RED**: HTTP test against `/graph/context` — a walk that reaches a Supplier
neighbour two hops out asserts that neighbour's `label` is its
`supplierName`, not `null` (proves the shared helper generalizes past the
finding-subject-only case Slice 1 covered).
**GREEN**: extract Slice 1's per-node resolution into a function both
handlers call; wire into `assemble_graph_context`.
**MUTATE/KILL MUTANTS/REFACTOR**: as Slice 1.
**Done when**: criteria met, verified live — expand a real Supplier's
neighbourhood in Explore and confirm every labeled node reads a name.

### Slice 3: The findings queue's own Subject column shows the resolved label

**Value**: a reviewer scanning the queue (before opening any finding) reads
"Nimbus Freight Logistics — SupplierNotFiled", not a raw id.
**Path**: whichever endpoint backs the findings queue list → attach each
finding's own subject's resolved label → `findingsQueue.tsx`'s subject
rendering (`:425`, `:341` `detail` string, `:195` `evidencePicture` seed
label).
**Acceptance criteria**:
- The findings-list response carries a resolved `subjectLabel` (or
  equivalent) per finding, using the same helper, keyed off
  `Catalog::finding_subject`'s already-existing `(Sid, pack)` resolution.
- The queue row's subject text and the evidence panel's seed label both
  prefer it, falling back to `displayTerm(finding.subject)` when absent.
**RED**: HTTP test on the findings-list route — a finding whose subject
resolves to a real Supplier asserts the response carries the resolved name.
**GREEN/MUTATE/KILL MUTANTS/REFACTOR**: as above.
**Done when**: criteria met, verified live in the findings queue.

### Slice 4: Prove it against hospitality — the domain-neutrality acceptance example, verbatim

**Value**: closes Plan 120's own acceptance example directly: "a hospitality
subject shows its own declared label the same way, proving the mechanism
isn't GST-specific."
**Path**: `packs/hospitality/pack.toml` gains `[console.labels]`; the
already-declared `hosp:DuplicateGuest` finding's evidence graph is exercised
end to end.
**Acceptance criteria**:
- `[console.labels] Guest = "guestSurname"` and `Property = "name"` added to
  `packs/hospitality/pack.toml`.
- No Rust or TS code changes in this slice — if any are needed, the
  mechanism from Slices 1–3 was not actually generic and that is itself the
  finding to fix, not a reason to add a hospitality-specific branch anywhere.
**RED**: HTTP test — install packs/hospitality (already scriptable per
`scripts/verify-pack-load.sh`'s pattern), raise/seed a `hosp:DuplicateGuest`
finding over `hosp:guest-1`/`hosp:guest-3`, assert the evidence graph's guest
nodes carry `label: "Smith"` / `label: "Smyth"`, not `guest-1`/`guest-3`.
**GREEN**: the pack.toml addition alone should be sufficient, by
construction, since the mechanism is class-name-and-pack-generic.
**Done when**: criterion met, verified live against a real hospitality pack
install — this is the test the whole slice exists to pass.

## Pre-PR Quality Gate (once, after Slice 4)

1. `scripts/mutants.sh` scoped runs already done per-slice — no repeat needed.
2. `cargo fmt` / `cargo clippy -p graph-owl-server -p graph-owl-api` clean.
3. `openapi.json` diff reviewed (a new response field is a real, deliberate
   surface change — regenerate and diff, per Plan 120's own Warning about
   Slice D's contract change applying equally here).
4. TS: `tsc`/lint clean on touched files.

---
*Delete this file when Slice 4 ships and Plan 120's own tracker entry for
Slice C is recorded — knowledge moves to `plans/DEMOS.md` and `CLAUDE.md`,
matching this project's standing convention.*
