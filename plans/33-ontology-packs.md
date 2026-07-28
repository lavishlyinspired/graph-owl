# Plan: Domain Ontology Packs (Epic 33)

**Branch**: feat/ontology-packs
**Status**: Not started
**Depends on**: Epic 24 (glossary and taxonomy model), Epic 9 (standards import)
**Crates**: `graph-owl-ontology` (OntologyPack, PackOverride, Licence) · `graph-owl-rdf-io` (SKOS/RDF pack import) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server` — no new crates

## Goal

Ship industry starter vocabularies so a bank does not hand-build a financial glossary. The difference between "here is an empty glossary" and "here are four thousand financial terms your analysts already use".

## Why content earns an epic

Low technical risk, high adoption value. Most of the gap between an installed catalog and a used one is that the installed one is empty. This is also the natural consumer of Epic 9's standards work — packs are distributed as standard vocabularies, not a bespoke format.

## Resolved decisions

1. **Packs are versioned artifacts, imported — not vendored into the repo.** Several of these vocabularies carry real licensing conditions; the repo ships mappings and manifests, not the content.
2. **Extend without fork.** An organization adds and overrides terms while still taking upstream pack updates. A pack that must be forked to customize will be forked once and never updated.
3. **Licence is tracked per pack and surfaced.** Some are freely redistributable, some require attribution, some require a licence to use. Importing must state which.
4. **Packs import as `Approved` terms in their own glossary**, not merged into an organization's glossary. Provenance stays visible and a pack is removable.
5. **SKOS is the interchange model** (Epic 24 decision 2), so pack import is a mapping rather than a translation.

## The financial pack, examined — because it is the one that will be asked for first

The demo estate is Indian retail and corporate banking, so the financial
vocabulary is the pack this project will be asked for before any other. Checked
against its repository on **28 July 2026** rather than assumed:

| | |
|---|---|
| Licence | **MIT** — freely redistributable, and unusually permissive for an industry vocabulary. Decision 1's "not vendored" rule stands anyway, on size and update-cadence grounds rather than licensing |
| Size | ~2,450 classes in the 2026/Q1 production release |
| Releases | Quarterly, with a **production/development split** — production is the reviewed subset |
| Authored in | OWL 2 **DL** |

**Three consequences that decide how it is used.**

**1. Take the production release, never development.** The split exists because
the development branch carries work in review. A catalog that imported it would
show analysts terms that may not survive the quarter, and a glossary that
changes under its readers is worse than a smaller one.

**2. It is authored in DL and this engine reasons in RL, so the honest claim is
the RL-expressible subset.** Most of what a catalog wants from it — class
hierarchy, property hierarchy, domain, range, inverse, transitivity — is inside
RL. What falls outside are the DL constructs that make full classification
expensive, and published experience is that RL is more useful than RDFS for
reasoning over this vocabulary in practice. **Import must therefore report what
it dropped**, per-axiom, rather than silently loading a subset and letting a
user believe the whole ontology is in force. A vocabulary that quietly means
less than it says is a worse foundation than one that states its own limits.

**3. It is a *business* ontology, not a metadata one — and conflating the two
is the mistake to avoid.** It describes financial instruments, legal entities
and contractual obligations. graph-owl describes tables, columns and schemas.
They are different layers, and the pack's job is to give a *column* something to
mean: "this column holds a monetary amount", "this table is about a legal
entity". That is Epic 24's glossary link, not a replacement for the `dsc:`
vocabulary.

**Where it earns the most, and it is not the glossary.** Its legal-entity
identifiers — LEI above all — are inverse-functional by definition, which makes
them the ready-made input to Epic 17's key-based identity (Slice A2) via Epic
95's `InverseFunctionalProperty` rule. Two records sharing an LEI are the same
entity, and that is a *derivation with an explanation* rather than a fuzzy
match. Of everything in this pack, that is the part that does work no
hand-written matcher does as well.

## Implementation reference

```rust
pub struct OntologyPack {
    pub envelope: EntityEnvelope,
    pub pack_id: String,                 // "fibo", "icd10", "gs1"
    pub version: String,
    pub licence: Licence,
    pub source_url: String,
    pub term_count: usize,
    pub imported_at: DateTime<Utc>,
}

pub enum Licence {
    Permissive { name: String },
    AttributionRequired { name: String, notice: String },
    LicenceRequired { name: String, contact: String },
}

pub struct PackOverride {                // extend-without-fork
    pub pack: EntityReference,
    pub term_path: String,
    pub kind: OverrideKind,              // Redefine | Hide | AddSynonym | AddRelation
    pub payload: serde_json::Value,
}
```

### Candidate packs

| Domain | Vocabularies |
|---|---|
| Finance | FIBO, ISO 20022, XBRL |
| Healthcare | LOINC, ICD-10, UMLS |
| Manufacturing | ECLASS, IEC CDD, asset-administration-shell patterns |
| Retail & supply chain | GS1 |
| General | schema.org, Dublin Core, SKOS core, ORG, Time |

General-purpose packs ship first: permissively licensed, universally useful, and they prove the mechanism without licence complexity.

### A pack vocabulary describes what data *means*, never how it *flows*

A fourth distinction to add to the three in `CLAUDE.md` that keep getting conflated, because this one has now been proposed once and is the most plausible of the lot.

**Business vocabulary and lineage vocabulary are different layers, and a pack only ever supplies the first.** FIBO is the clearest case: it is, in the EDM Council's own framing, a *business conceptual model* — as distinct from descriptions of data or IT implementations. It says what a loan, a counterparty, or a principal amount **is**. It says nothing about a table feeding a dashboard, and it should not, because that is plumbing rather than finance.

So this is right, and is what a pack is for:

```turtle
:loan_portfolio.principal_amount  dsc:meaning  fibo-loan:PrincipalAmount .
```

And this is a layer error:

```turtle
:rel_001  rdf:reifies << :loan_portfolio  fibo:isDataFor  :risk_report >> .
```

`fibo:isDataFor` was invented for the example; FIBO defines no such property and would not. Mapping `dsc:feeds` onto a business ontology takes a lineage fact — asset A supplies asset B — and dresses it in a vocabulary that cannot express it. If a lineage predicate ever needs a standard name on the wire, the vocabularies that model provenance are the place to look (PROV-O's `prov:wasDerivedFrom` is the obvious candidate), and that is a decision for `29-lineage.md` and `09-engine-rdf-io.md`, not for a pack.

The rule this protects is already stated above: **packs supply vocabulary, not schema** — and lineage is schema. A pack that could redefine what `dsc:feeds` means on export would let a vocabulary change the shape of the graph, which is exactly the coupling the override model exists to prevent.

## Acceptance criteria

- [ ] A pack imports from a standard serialization into its own glossary.
- [ ] Imported terms are `Approved` and attributable to the pack.
- [ ] Licence is recorded and surfaced; a licence-required pack warns before import.
- [ ] Overrides extend a pack without preventing an upstream update.
- [ ] A pack upgrade preserves overrides and reports conflicts.
- [ ] A pack is removable; removal reports affected asset attachments.
- [ ] Pack terms are searchable and attachable like any term.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Pack import

**Acceptance criteria**: import a SKOS/RDF vocabulary via Epic 9's parser into a pack-owned glossary; terms land `Approved` with `broader`/`narrower` from the source; a malformed pack fails before anything lands; import is idempotent — re-importing the same version is a no-op; term count and version recorded; a 4,000-term pack imports within a stated time bound.
**RED**: Idempotency test asserting a re-import creates nothing. A hierarchy-fidelity test asserting `broader` depth matches the source at depth 3 — a flattened import loses the vocabulary's structure, which is most of its value. Mutator watch: flattening the hierarchy must fail the depth test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Licence handling

**Acceptance criteria**: licence recorded per pack from its manifest; `LicenceRequired` packs refuse import without an explicit acknowledgement flag; `AttributionRequired` packs surface the notice wherever their terms are displayed; licence is included in Epic 37b exports; a pack with no licence metadata refuses import rather than defaulting to permissive.
**RED**: The unknown-licence test asserting refusal rather than a permissive default — defaulting to permissive on missing metadata is how licensing violations happen. An attribution-surfacing test. Mutator watch: defaulting to permissive must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Extend without fork

**Acceptance criteria**: `PackOverride` redefines a term's description, hides a term, adds synonyms, or adds relations; overrides are stored separately from pack content; reading a pack term applies overrides transparently, with `overridden: true` visible; removing an override restores the pack value; overrides survive a pack upgrade.
**RED**: The upgrade-survival test: apply an override, upgrade the pack, assert the override still applies. Without it, decision 2 fails and every customization is lost on update. Mutator watch: storing overrides inside pack content must fail the upgrade test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Upgrade and conflict reporting

**Acceptance criteria**: upgrading to a new pack version adds new terms, updates changed ones, and marks removed ones `Deprecated` rather than deleting; a term removed upstream but attached to assets is reported, not silently deprecated out from under them; an override targeting a term removed upstream is reported as an orphaned override; upgrade is dry-runnable; the report names added, changed, removed, and conflicting counts.
**RED**: The attached-term test: a term removed upstream but in use must be reported prominently and remain attached. A dry-run test asserting no mutation. Mutator watch: deleting an in-use term must fail; silent deprecation must fail the reporting assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Removal

**Acceptance criteria**: removing a pack reports how many assets have its terms attached, by type; `?force=true` removes the pack and its attachments transactionally, bumping affected asset versions; overrides for the pack are removed with it; removal is refused while another pack references its terms via `exactMatch`; removal is auditable.
**RED**: A cross-pack reference test asserting removal is refused when another pack points at it — removing a referenced vocabulary would break the referring pack silently. Mutator watch: ignoring cross-pack references must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Authoring packs inside graph-owl** → packs are imported artifacts; authoring is Epic 24's glossary.
- **Automatic term mapping** (matching org terms to pack terms) → Epic 17's resolution machinery could do it; needs a human gate and is a separate capability.
- **Pack marketplace / registry** → a distribution concern, not a product feature.
- **Domain-specific entity types from packs** (FIBO classes as catalog entity types) → a much larger change to the fixed entity model; packs supply vocabulary, not schema.
- **Ontology reasoning over pack axioms** → Epic 6's OWL 2 RL applies if a pack ships axioms; not a goal of this epic.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **No pack content committed to the repo** — verified by a repo-size and file-type check.
5. Licence refusal on missing metadata verified (Slice B).
6. Override survival across upgrade verified (Slice C).
