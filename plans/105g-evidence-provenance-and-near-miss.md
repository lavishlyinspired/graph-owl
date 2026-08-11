# Plan: Evidence provenance and near-miss linking — the real form of P7's interpretation half

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Slice 1 shipped 11 August 2026 (`Catalog::node_sources`, `GET /findings/{id}/evidence-graph`'s `sources` field, console `Sources` section). Slice 2 not started — see its own section below. Scoped 11 August 2026, at the user's direction after `105e`'s traversal half shipped, redirected by a grill-me-shaped investigation (see "Why this instead of the original P7 design" below) and confirmed by the user via `AskUserQuestion`.
**Depends on**: `plans/105e-evidence-chain-walk.md` (shipped — `Catalog::finding_evidence_graph`, `GET /findings/{id}/evidence-graph`, the console's graph+triples rendering), `graph-owl-engine`'s existing `TripleStore::query_pattern` (no new Rust primitive needed for Slice 1).
**Crates**: `graph-owl-api` (orchestration), `graph-owl-server` (route), `ui/` (console). **No new crates, no Rust changes to `graph-owl-engine`/`graph-owl-engine-postgres`** — Slice 1 is a new query against an existing capability.

## Why this instead of the original P7 design

The platform doc's own words: "Rust: a bounded graph computation (candidate evidence retrieval, missing-hop determination)... Python interprets what a gap means under which assertion... using the pack's `findings.yaml`." Read literally, this calls for a generic pack-config table mapping finding labels to expected assertion chains.

Checked against GST's real six findings before writing any plan around that design (the same "prove the system doesn't already do this" discipline `plans/00l` and the platform doc's own decision 5 require), because a design built against a hypothetical is exactly what `CLAUDE.md` warns against ("Don't design for hypothetical future requirements"):

| Finding | Is there a walkable "missing hop"? |
|---|---|
| `gst:PotentialMismatch` | No — its own `OPTIONAL`+`!BOUND` SPARQL query already **is** the missing-hop detection, at finding-creation time. A forward walk from the invoice cannot discover "no `Gstr2bInvoice` exists" by traversing outward; there is nothing to walk to. |
| `gst:AmountMismatch` | No — both sides are present and matched. The gap is a **value delta**, not a topology gap. |
| `gst:ITCNotAvailable` | No — matched; statute blocks the credit regardless. |
| `gst:Reversed` | No — matched; reverse-charge flagged. |
| `gst:PaymentOverdue` | No, in the topological sense — span-based (elapsed days), already fully resolved by the rule's own date arithmetic. |
| `gst:GstinTransposition` | **Yes.** Two `gst:Supplier` subjects — one from the purchase register, one from GSTR-2B — that *should* be the same entity and are not linked, by the near-miss policy's own design (`gst:MatchingPolicy`: "surfaced, never auto-linked"). That absence is a genuine "expected an edge here, found none." |

Five of six findings' gaps are already fully computed by their own rule at finding-creation time; building a second, generic missing-hop mechanism for them would duplicate detection that already happened. Building the generic mechanism now, with only one real finding that needs it, means designing the general case from a sample size of one — the exact trap `plans/00l`'s spike discipline exists to catch before it is code rather than after.

**What is real and buildable, checked against the 9 findings already live on the demo server:**

1. The evidence graph shipped in `105e` does not surface **provenance** — which source document (`graph:import:gst-purchase-register` vs `graph:import:gst-gstr2b`) each node's flakes were asserted in. A reviewer sees `supplier-27AABCU9603R1ZM` with no indication of which side claims it. This is visible today, on real data, with no design invention required — `TripleStore::query_pattern` already returns each flake's own `cx`.
2. `GstinTransposition`'s evidence graph (traversal-only, from `105e`) currently shows only the seed side's own supplier — the traversal has no edge to walk to the other side's supplier, because the absence of that edge is the entire point of the finding. Making the second supplier and the missing link **visible** is the one case where "missing-hop determination" is a real, provable, non-hypothetical piece of work.

## Slices

### Slice 1 — Provenance on every evidence-graph node

**Value**: A reviewer looking at a finding's evidence graph can see which source document (purchase register vs GSTR-2B, or any future pack's own sources) asserted each fact, without leaving the finding.
**Path**: `GET /findings/{id}/evidence-graph` → `Catalog::finding_evidence_graph` (existing) → for each node in the resolved `Subgraph`, a new `Catalog::node_sources(sid) -> Vec<String>` query against `TripleStore::query_pattern` with `s: Some(sid)`, collecting distinct `cx` values → the route adds a `sources: string[]` field per node → the console's evidence-graph node list and triples table show it.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring` before any code.
**Acceptance criteria** (needs confirmation before code):
- [ ] A node whose flakes were all asserted by one import (e.g. `gst-purchase-register`) reports exactly that one source.
- [ ] A node asserted by more than one import (a `Supplier` referenced from both the purchase-register and GSTR-2B documents) reports both, deduplicated.
- [ ] A node with no flakes of its own beyond being an object of another subject's edge (should not happen given how `finding_evidence_graph` builds its node set, but named rather than assumed) reports an empty list, not an error.
- [ ] `GET /findings/{id}/evidence-graph`'s JSON response carries `sources` per node; `openapi.json` regenerated to match.
- [ ] Console: the evidence-graph node list / triples view shows each node's source(s), distinguishable when a node has more than one.
- [ ] Verified against the real GST pack: `pr-INV-1003`'s own node reports `gst-purchase-register`; `supplier-29AACCG0527D1Z8` (referenced from both the purchase register and GSTR-2B fixtures) reports both.
**RED**: `Catalog::node_sources` unit test (fake `TripleStore`/graph, or reuse the existing `finding_evidence_graph_tests` fixture pattern from `105e`) — a node with one source, a node with two, a node with none. Mutator watch: an `s.is_empty()` short-circuit that silently drops the case where a node has zero flakes of its own; a dedup step that a `Vec` → `HashSet` swap could silently break if tested only with already-unique inputs.
**GREEN**: minimum code — call `query_pattern` per node, collect+dedup `cx`, attach to the response.
**MUTATE**: `scripts/mutants.sh graph-owl-api --diff crates/graph-owl-api/src/lib.rs` (unit-test-covered, `--lib` applies).
**KILL MUTANTS**: address survivors; ask if ambiguous.
**REFACTOR**: assess only after mutation testing confirms test strength.
**Done when**: acceptance criteria met, mutation report reviewed, real-Postgres integration test added to `crates/graph-owl-server/tests/evidence_graph.rs` proving the two-source case against real GST data, human approves commit.

### Slice 2 — Near-miss linking for `GstinTransposition` (and any future pack's own near-miss finding)

**Value**: A reviewer looking at a `GstinTransposition` finding sees *both* candidate Supplier subjects and the fact that no link exists between them — not just the one side the traversal happened to start from.
**Path**: sketched, not yet slice-ready — genuinely needs its own confirmation round before RED, because the pack-config shape is a real open question this slice cannot skip.

**The open design question, named rather than hidden**: `finding_evidence_graph`'s traversal seed is a single subject (`finding.subject`). To show the *second* supplier, something has to resolve it — and that means looking up a `gst:Supplier` by a GSTIN *value* (`filedGstin`, already bound in the finding's own flat `evidence` list from `105b`'s rule engine), not by walking an edge. Two live options, neither chosen yet:

1. A narrow pack-config addition — smaller than the original `findings.yaml` design — naming which two evidence variables in a `[[findings]]` rule are a "near-miss pair" and which predicate to resolve the second one by (e.g. `[findings.near_miss_pair] left = "claimedGstin", right = "filedGstin", resolve_by = "gst:supplierGstin"`). Generic across any pack with a similarity band, not GST-specific — the same test `00h`/`00e` already apply to every config surface.
2. Reuse the finding's own `[findings.similarity]` band (already present for `GstinTransposition`) as the *same* declaration, since it already names `left`/`right` — extending its meaning rather than adding a sibling table.

Option 2 reuses an existing table rather than inventing a new one, which is the smaller change — but needs confirming that a similarity band and a near-miss-pair declaration are actually the same concept for every finding that has one, not just this one. That confirmation is Slice 2's own first step, not assumed here.

**Not started. Requires a confirmed acceptance-criteria round (per the `planning` skill's own protocol) before any RED test is written**, precisely because the design choice above is unresolved.

## What this deliberately does not do

- It does not build a generic `findings.yaml`-shaped assertion-mapping mechanism for every conceivable future gap. Five of GST's six findings never need one; inventing the general case from one real instance is designing for a hypothetical.
- It does not touch `graph-owl-engine`/`graph-owl-engine-postgres`. `query_pattern` already returns everything Slice 1 needs.
- It does not resolve Slice 2's pack-config question inside this document. That is Slice 2's own confirmation step.
