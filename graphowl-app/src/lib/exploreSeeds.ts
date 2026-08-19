import type { Finding } from "./api";

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
