# Plan 110 — Capability built, capability unreachable: closing the console's gap

**Status**: **all five slices shipped.** **Branch**: main.

## What shipped, and what is deliberately still unreachable

| Slice | State |
|---|---|
| 0 — console neutrality check | shipped; proven red on a planted violation and green on the tree |
| 1 — reasoning: profile, classify, explain | shipped |
| 2 — data-quality tests and contracts | shipped, read-only |
| 3 — governance queues | shipped, and the suggestion queue is actionable (confirm/reject) |
| 4 — business metrics and custom properties | shipped, read-only |

**34 routes remain unreached, and most of them should.** Re-measured after the
work rather than assumed:

- **Operator actions, not console features** — `/ingest`, `/ingest/batch`,
  `/usage/prune`, `/test-results/prune`, `/lineage/reconcile`,
  `/validation/shapes/seed`, `POST /predicates`, `/webhooks/receive/{path}`.
  `00g-operations.md` is their home. A button that prunes retention from a web
  page is a footgun, and seeding shapes is an install-time act the pack loader
  owns.
- **Write twins of things now visible** — `/contradictions/reviews`,
  `/drift/reports`, `/validation/assignments`, `/certifications/{targetFqn}`,
  `/extraction/runs`. Their read sides are reachable; the writes need an
  authoring surface, not a button.
- **Genuinely worth doing next, in order**: `/webhooks/*` (seven routes —
  endpoints, mappings, dead-letters, replay; `OutboundWebhooksPanel.tsx` covers
  only part), `/business-metrics/search` and `/glossary-terms/search` (search
  over lists this plan just made visible), `/certification-types`,
  `/connectors/configs`.

**The read-only decision is the one most worth revisiting.** Slices 2 and 4
show tests, contracts and metrics and cannot create them, because authoring
needs an assertion-language editor, a cadence picker and a formula builder.
That is a real feature and it is the honest next plan — but a deployment can
now *see* what it has, which is what stops tests accumulating unnoticed.

**Trigger**: a direct observation that the console reaches only part of what the
server offers, and that the reconciliation page had been built GST-shaped. Both
are the same defect in different clothes: **a capability that only one caller
can reach is not a capability, it is a private function with an HTTP route.**

## The audit, measured rather than estimated

`crates/graph-owl-server/src/lib.rs` registers **221 routes**. A crude
prefix-grep against `ui/src/api.ts` reported 88 unreached, and **that number was
wrong** — exports, proposals, webhooks, labels, certifications and Cypher are
all reached through dynamically-built URLs the grep could not see. Re-checked by
searching the whole `ui/src` tree for each route family, the genuinely
unreachable set is much smaller and much more interesting:

| Family | Routes | What the server can do that no human can ask for |
|---|---|---|
| `reasoning/el/*` | 2 | **Classify an ontology under OWL EL, and explain why an entailment holds** |
| `ontology/profile` | 1 | Which OWL profile the loaded ontology actually fits |
| `validation/shapes/seed`, `validation/assignments` | 3 | Seed SHACL shapes; assign shapes to targets |
| `contracts/*` | 5 | Data contracts, their SLAs, their breaches, their status |
| `test-definitions`, `test-cases`, `test-suites`, `test-results` | 6 | Data-quality tests with a cadence and a result history |
| `business-metrics/*` | 4 | Governed metric definitions and their sources |
| `custom-properties/*` | 2 | Per-deployment extension properties |
| `drift/reports`, `contradictions/reviews` | 2 | Drift detection; contradiction adjudication |
| `glossary-terms/search` | 1 | Search the glossary |
| `label-suggestions`, `recertification-queue` | 2 | Suggested labels; what is due for recertification |
| `extraction/runs` | 2 | Start/cancel an extraction run |
| `connectors/configs` | 2 | Stored connector configuration |
| `lineage/reconcile`, `graph/reconcile` | 2 | Reconcile projections |
| `usage/prune`, `test-results/prune` | 2 | Retention |

**~30 routes across 14 families**, not 88. The correction matters: an
overstated gap produces a scattershot plan, and three of these families are
worth far more than the other eleven put together.

## The rule this plan is built on

Every item below must work **for any pack** — GST, healthcare, banking,
automotive — or it is not built here. The reconciliation page's failure is the
worked example: its sources, measures and guidance were TypeScript constants, so
a second domain would have had its data rendered under GST's headings. The fix
was `[console.reconciliation]` in `pack.toml` plus `GET /packs/{pack}/console`,
and **that route is now the pattern**: a pack declares, the console renders,
and a pack that declares nothing gets an honest empty state rather than another
pack's vocabulary.

Concretely, three tests for every slice here:

1. **Would this work if the only installed pack were hospitality?** If it
   renders GST's words, it is wrong.
2. **Does the pack declare it, or does the console assume it?** Anything that
   names a domain belongs in `pack.toml`.
3. **What does a deployment with no pack at all see?** An empty state that says
   so — never an error, never a blank.

## Sequenced by value, not by route count

### Slice 1 — Reasoning: classify, and explain why ★

`POST /reasoning/el/classify` and `GET /reasoning/el/explain` are the two most
valuable unreachable routes in the list, and it is not close. `graph-owl-reasoning`
is 3,491 lines; the console cannot invoke any of it.

**Why it matters more than the other twelve families.** Every finding this
product surfaces is currently the result of a *query* — something asserted, or
something absent. An entailment is different in kind: it is a fact **nobody
wrote down** that follows necessarily from the ones that were. That is the
difference between a reconciliation tool and a knowledge graph, and right now
the product ships the engine for it with no way to press the button.

`explain` is the half that makes it defensible: an entailment a reviewer cannot
interrogate is worse than none, because it looks authoritative and cannot be
checked — the same argument `governed_by` already makes for findings.

**Domain-neutral by construction**: classification runs over whatever ontology
is loaded. GST's `gst:Class`/`gst:Property` hierarchy, hospitality's, or a
healthcare pack's — the engine does not know or care.

**Where it goes**: the Governance section already has "Run validation" and "Run
reasoning" buttons. `Run reasoning` today reports "0 derived, 0 replaced" for
GST, which is correct (the pack registers no entailment rules) but reads as
broken. Classification plus explanation is what would make that section worth
opening.

### Slice 2 — Data-quality tests, contracts and SLAs

Eleven routes, one coherent product surface: *define an expectation, run it on a
cadence, see the result history, and treat a breach as a contract event.*

Domain-neutral: a test is a named assertion over a query. GST's would be "no
invoice in the register lacks a supplier"; a healthcare pack's would be "no
claim lacks a member id".

**This is deliberately second, not first.** It is more routes and more screens
than Slice 1 and less differentiating — plenty of tools do data-quality tests,
almost none explain an entailment.

### Slice 3 — Governance actions already half-built

`label-suggestions`, `recertification-queue`, `contradictions/reviews` and
`drift/reports` are each one route away from a queue the console already knows
how to render: `ReviewSection` has a generic queue abstraction with five
implementations. These are cheap, and cheap is the reason to do them together
rather than to skip them.

### Slice 4 — `business-metrics` and `custom-properties`

Genuine capability, no current consumer, and no evidence anybody wants them yet.
Recorded so they are not rediscovered as "missing", not scheduled on the
suspicion that they might matter.

### Not scheduled, and why

- `usage/prune`, `test-results/prune`, `graph/reconcile`, `lineage/reconcile` —
  operator actions, not console actions. `00g-operations.md` is their home; a
  button that prunes retention from a web page is a footgun.
- `ingest`, `connectors/configs`, `extraction/runs`, `webhooks/*` — reachable
  or partly reachable already, or operator-shaped.
- `validation/shapes/seed` — seeding shapes is an install-time action the pack
  loader should own, not a console button.

## The gap this plan does not close

**The console's own domain-neutrality is now enforced by convention, not by a
check.** `packSurfaces.ts`, `statement.ts` and `ReconciliationWorkspace.tsx`
have each held GST constants at some point, and each was found by hand.
`scripts/check-namespace-neutrality.py` already exists and evidently does not
cover this. Extending it to fail the build when a domain term appears outside
`packs/` is worth more than any single slice above, because it is the thing that
stops the defect recurring — and it should probably come first.
