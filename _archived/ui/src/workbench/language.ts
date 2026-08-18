/** Everything decidable about *which* query language the workbench is in, and
 *  *when* the graph it asks about is — Plan 111 Slice B.
 *
 *  **Two gaps, one surface.** `POST /cypher` had no console caller at all, so
 *  a property-graph query language this product implements end to end was
 *  reachable only with curl; and `POST /sparql` has accepted `asOf` since
 *  Epic 4 while `api.sparql()` took one argument, so the surface where
 *  re-running a question against the past is the obvious thing to want could
 *  only ever ask about now.
 *
 *  Nothing here is domain-specific: a query language is a query language, and
 *  transaction time is a property of the store rather than of any pack. */

/** The two the server actually implements. Anything else — a saved URL from a
 *  future version, a typo — is not silently coerced to one of them. */
export const LANGUAGES = ["sparql", "cypher"] as const;

export type Language = (typeof LANGUAGES)[number];

export function isLanguage(value: string | null): value is Language {
  return value !== null && (LANGUAGES as readonly string[]).includes(value);
}

/** **A query that runs, not a blank editor.** The first thing anyone needs
 *  from a query surface is proof that it answers at all; an empty box makes
 *  "does this work" and "did I write it wrong" the same question. */
export function defaultQuery(language: Language): string {
  return language === "cypher"
    ? "MATCH (n)-[r]->(m)\nRETURN n, r, m\nLIMIT 50"
    : "SELECT ?s ?p ?o WHERE { ?s ?p ?o }\nLIMIT 50";
}

/** Per-language drafts, so toggling to look at the other language and back
 *  does not silently discard real work. */
export type Drafts = Partial<Record<Language, string>>;

export function keepDrafts(drafts: Drafts, language: Language, text: string): Drafts {
  return { ...drafts, [language]: text };
}

/** What to say above the results when the clock is not "now".
 *
 *  **A result from the past that looks like a result from now is the one
 *  failure this surface cannot afford**: historical data and stale data are
 *  indistinguishable on screen, and only a label tells them apart. `null`
 *  when there is nothing to say, so the caller renders nothing rather than an
 *  empty banner. */
export function pastTenseNote(asOf: string | null): string | null {
  if (asOf === null) return null;
  const at = new Date(asOf);
  const when = Number.isNaN(at.getTime()) ? asOf : at.toLocaleString();
  return `Answered against the graph as it stood at ${when}, not as it stands now.`;
}
