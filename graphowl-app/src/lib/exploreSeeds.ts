import type { Finding } from "./api";
import type { EntitySummary } from "./graph/entityList";

/** A starting point Explore can offer when nothing has been searched for. */
export interface ExploreSeed {
  readonly id: string;
  readonly label: string;
  readonly findings: number;
}

/** How many starting points to offer.
 *
 *  Capped because this is a seeding aid, not a findings screen — a list long
 *  enough to scroll competes with the screen that exists for exactly that, and
 *  the point here is to get someone *into* the graph. */
export const MAX_SEEDS = 12;

/** Starting points drawn from what the engine has actually flagged.
 *
 *  **Plan 123 §9**: "Explore needs a search term to show anything — blank
 *  screen with 'Search or open an entity'. With search broken, unreachable."
 *  Search is fixed; this removes the dead end that remained.
 *
 *  Findings rather than recent subjects, because a finding is a subject
 *  somebody has a reason to look at. Recency says only that something was
 *  written. */
export function seedsFromFindings(findings: readonly Finding[]): readonly ExploreSeed[] {
  const bySubject = new Map<string, { label: string; count: number }>();
  for (const finding of findings) {
    const existing = bySubject.get(finding.subject);
    // An invoice with three problems is one thing to explore, not three — a
    // list repeating it would bury every other subject.
    if (existing) existing.count += 1;
    else bySubject.set(finding.subject, { label: finding.label, count: 1 });
  }

  return [...bySubject.entries()]
    .map(([id, { label, count }]) => ({ id, label, findings: count }))
    // Most-flagged first — that is where to look. Ties broken by subject so
    // the list does not reshuffle between loads; one that reorders itself on
    // every refresh cannot be scanned.
    .sort((a, b) => b.findings - a.findings || a.id.localeCompare(b.id))
    .slice(0, MAX_SEEDS);
}

/** Every real entity the graph declares, as a picker option — not only
 *  the ones a rule flagged. A flagged subject keeps its real finding
 *  count (`seedsFromFindings` already computed it); everything else is
 *  labelled by its own semantic type, since no finding named it.
 *
 *  **Not capped, unlike `seedsFromFindings`/`MAX_SEEDS`.** That cap
 *  exists because a findings list is a seeding *aid* competing with the
 *  Findings screen itself; a complete entity picker has no such screen
 *  to defer to, and hiding real entities from it would just be a second,
 *  worse findings list. */
export function mergeEntityOptions(
  allEntities: readonly EntitySummary[],
  flagged: readonly ExploreSeed[],
): readonly ExploreSeed[] {
  const flaggedById = new Map(flagged.map((seed) => [seed.id, seed]));
  const merged = allEntities.map(
    (entity) => flaggedById.get(entity.iri) ?? { id: entity.iri, label: entity.type, findings: 0 },
  );
  const coveredIds = new Set(allEntities.map((entity) => entity.iri));
  for (const seed of flagged) {
    if (!coveredIds.has(seed.id)) merged.push(seed);
  }
  return merged.sort((a, b) => b.findings - a.findings || a.id.localeCompare(b.id));
}

/** Every finding recorded against one subject.
 *
 *  This is what makes the detail panel real. The design mock shows a
 *  confidence score, a derivation chain and a list of supporting and
 *  contradicting documents; the graph API returns none of those for an edge.
 *  What it *does* return, per subject, is the rules that fired on it and the
 *  exact bindings that satisfied them — which is the same question answered
 *  from the data that exists. */
export function findingsFor(
  findings: readonly Finding[],
  subject: string | undefined,
): readonly Finding[] {
  if (subject === undefined) return [];
  return findings.filter((finding) => finding.subject === subject);
}

/** One numbered step of "why GraphOWL believes this". */
export interface ReasoningStep {
  readonly text: string;
  readonly source: string;
}

/** A finding read back as the chain that produced it.
 *
 *  Three parts, all of them things the finding actually carries: what the
 *  subject is classified as, each binding the rule matched on, and the rule
 *  itself as the closing step. Nothing here is inferred about the reasoner's
 *  internals — it is the rule's own inputs and output, in order.
 *
 *  The classification step is skipped when the graph could not type the
 *  subject, rather than opening the chain with a step that says nothing. */
export function reasoningSteps(
  finding: Finding,
  semanticType: string | undefined,
): readonly ReasoningStep[] {
  const classification: readonly ReasoningStep[] =
    semanticType === undefined
      ? []
      : [{ text: `The subject is classified as ${semanticType}.`, source: "ontology · rdf:type" }];

  const bindings: readonly ReasoningStep[] = finding.evidence.map((item) => ({
    text: `${item.predicate} = ${item.value}`,
    source: item.var ? `bound to ?${item.var}` : finding.pack,
  }));

  return [
    ...classification,
    ...bindings,
    { text: `Rule ${finding.label}: ${finding.summary}`, source: finding.governedBy },
  ];
}
