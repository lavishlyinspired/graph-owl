/** The loaded import sources, for the Explore "Pack data" block — Plan 115
 *  Slice B1.
 *
 *  **One query answers "what has actually been imported".** Every pack import
 *  lands in `graph:import:{source}` (see `importFile.ts`'s `importThroughSurface`
 *  and the server's `/graph/import/rdf` handler), so a single
 *  `SELECT ?g (COUNT(?s) AS ?n) ... GROUP BY ?g` over the whole graph lists
 *  every loaded source and how many triples it holds — the same source name a
 *  successful upload prints in its toast (C1), so the user can match the two.
 */

import type { Solution } from "../../workbench/results";
import { lexical } from "../../workbench/results";

/** One named import graph that actually holds data. */
export interface LoadedSource {
  /** The import source name, e.g. `gst-gstr2b-2025-07` — the same name the
   *  upload toast prints. */
  readonly name: string;
  /** The pack it belongs to, from the source-name prefix (`gst-...`). */
  readonly packId: string;
  /** Triples the named graph holds. */
  readonly triples: number;
}

const IMPORT_GRAPH_PREFIX = "graph:import:";

/** `graph:import:gst-gstr2b-2025-07` → `gst-gstr2b-2025-07`. Anything that is
 *  not an import graph is `null` — a vocabulary graph is a real graph and
 *  must not be offered as "data you imported". */
export function importSourceOf(graphIri: string): string | null {
  return graphIri.startsWith(IMPORT_GRAPH_PREFIX) && graphIri.length > IMPORT_GRAPH_PREFIX.length
    ? graphIri.slice(IMPORT_GRAPH_PREFIX.length)
    : null;
}

/** The pack id a source name belongs to — the part before the first `-`,
 *  which is exactly how `importFile.ts` composes the name (`{pack}-{key}-{period}`).
 *  A name with no `-` keeps itself, so a malformed graph never crashes the
 *  listing. */
function packIdOfName(source: string): string {
  const dash = source.indexOf("-");
  return dash <= 0 ? source : source.slice(0, dash);
}

/** The loaded import sources out of one named-graph listing.
 *
 *  Values pass through `lexical`, because `POST /sparql` returns N-Triples:
 *  a graph name arrives as `<graph:import:…>` and a count as
 *  `"42"^^<…integer>`, and both must be unwrapped before they mean anything. */
export function loadedSourcesFromSparql(rows: readonly Solution[]): readonly LoadedSource[] {
  const out: LoadedSource[] = [];
  for (const row of rows) {
    const name = importSourceOf(lexical(row.g ?? ""));
    if (name === null) continue;
    out.push({ name, packId: packIdOfName(name), triples: Number(lexical(row.n ?? "") || 0) });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** The loaded sources that belong to one installed pack. Matching on the
 *  source-name prefix keeps connector imports (`connector:erpnext` brings
 *  data, not a pack) out of a pack's listing without any of them disappearing
 *  from the graph. */
export function sourcesForPack(
  sources: readonly LoadedSource[],
  packId: string,
): readonly LoadedSource[] {
  return sources.filter((source) => source.packId === packId);
}
