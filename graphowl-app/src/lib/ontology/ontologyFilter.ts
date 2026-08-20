/** One filter for the classes/relationships/properties browser (Slice 2 of
 *  `plans/ontology-graph.md`) — a blank query matches everything, so the
 *  browser's default state is simply "show it all", not an empty list
 *  waiting for input. */
export function matchesOntologyFilter(name: string, query: string): boolean {
  const trimmed = query.trim().toLowerCase();
  if (trimmed === "") return true;
  return name.toLowerCase().includes(trimmed);
}
