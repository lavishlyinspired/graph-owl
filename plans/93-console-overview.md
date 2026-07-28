# Plan: Console Overview (Epic 93)

**Status**: **In progress** — Overview shipped (Demo 3)
**Depends on**: Epic 2 (hierarchy), Epic 3 (envelope), Epic 4 (graph), Epic 13 (authorization)
**Crates**: `graph-owl-server` (one endpoint), `ui/`

## Goal

A landing page that answers "what is in here, and how much of it can I trust"
without the reader clicking anything.

## Why this is an epic and not a screen

Because the temptation is to build the picture first and find the data later,
and that produces a dashboard of plausible-looking numbers nobody can act on.
Every tile here is a number the system already knows and can defend. The
constraint is deliberate and it is the whole design:

> **A tile that cannot be computed from real data does not ship, however good
> the mock looks.**

The reference mock (local only, not committed — see `.gitignore`) shows Data Products,
Ontologies, Policies, Data Quality, Insights and Agents. Those are Epics 22–38.
Rendering them now over invented numbers would make the console lie about the
maturity of the product it fronts, and would be discovered by the first person
who clicked one.

## Resolved decisions

1. **Authorization-filtered, like every other count.** The overview is computed
   through the same predicate as list and search. A total that ignored policy
   would leak the size of what the reader may not see — the exact leak Demo 2
   exists to demonstrate is closed.
2. **One request, not six.** A dashboard that fans out to six endpoints renders
   in six stages and shows a different partial truth in each. One endpoint, one
   answer, one paint.
3. **Documentation coverage is the headline governance number.** Not because it
   is the most sophisticated signal but because it is the one that is *true
   today*: certification, quality and lineage tiles all need epics that do not
   exist, and coverage needs only a `description` column. It is also the number
   most likely to make someone act.
4. **The graph gets a size, not a score.** Flake count, node count and edge
   count are facts. "Graph health" would be a number with no definition behind
   it.
5. **Empty states say what to do.** A new deployment sees zeros; the tile says
   how to make them non-zero rather than rendering a sad chart.

## Implementation reference

```
GET /overview  →
{
  "assets":        { "total": 124, "byKind": [{ "kind": "table", "count": 15 }, …] },
  "documentation": { "described": 12, "total": 124 },
  "graph":         { "flakes": 1234, "nodes": 124, "edges": 123 },
  "recentlyChanged": [ Asset, … ]        // newest first, capped
}
```

Every field is derived from a table that exists. `graph` reads the flake
projection, so it is also the honest way to surface projection lag: if the node
count trails the asset count, the graph view is behind and the number says so
rather than a log line nobody reads.

## Acceptance criteria

- [ ] One request populates the whole page.
- [ ] Every number is authorization-filtered — two principals see different
      overviews, asserted.
- [ ] Documentation coverage counts non-empty descriptions, not non-null ones.
- [ ] A brand-new deployment renders zeros with an action, not a broken chart.
- [ ] Node count trailing asset count is visible rather than hidden.
- [ ] The page is reachable by keyboard and readable without colour.

## Slices

### Slice A: `GET /overview`, authorization-filtered

**RED**: two principals, one endpoint, different totals — the same property
Demo 2 proves for search, now for the landing page. A description of `"   "`
must not count as documented.
**Done when**: criteria met, mutation report reviewed.

### Slice B: The page

**RED**: renders zeros with an action on an empty catalog rather than an empty
chart; the kind breakdown is readable without relying on colour.
**Done when**: criteria met, verified in both themes.

## Explicitly not in this epic

- Data Products, Ontologies, Policies, Data Quality, Insights, Agents tiles →
  Epics 22–38. They need the epics, not the markup.
- Time-series growth charts → needs a metrics store; Epic 10.
- Quality score donut → Epic 30.
