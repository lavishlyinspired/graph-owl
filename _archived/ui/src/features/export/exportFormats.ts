/** The export dialog's own decisions — Epic 42 / Phase 3 item 3.15.
 *
 *  `00f-ui-architecture.md` puts the part that can be wrong in a way somebody
 *  would act on in a pure function. For an export dialog that is: which URL a
 *  chosen format and filter pair actually downloads from — get this wrong and
 *  a reader downloads the wrong scope, or the wrong format's query string,
 *  with no way to notice before opening the file.
 */

/** One downloadable shape. Nine entries, not six: RDF's four wire formats
 *  share one route (`?format=`), and modelling each as its own entry keeps
 *  the dialog a flat list rather than a two-step "pick a family, then a
 *  sub-format" flow for a set this small. */
export interface ExportFormat {
  readonly key: string;
  readonly label: string;
  readonly path: string;
  /** Only RDF entries carry this — the `?format=` value the route itself
   *  requires (`export_rdf`'s own `RdfExportQuery`). */
  readonly rdfFormat?: string;
}

/** A non-empty tuple, not a plain array — `noUncheckedIndexedAccess` would
 *  otherwise type `EXPORT_FORMATS[0]` as possibly `undefined`, forcing a
 *  fallback for a case that cannot actually occur (this list is a fixed
 *  literal, never built up at runtime). */
export const EXPORT_FORMATS: readonly [ExportFormat, ...ExportFormat[]] = [
  { key: "graphml", label: "GraphML", path: "/graph/export/graphml" },
  { key: "bulk-csv", label: "Neo4j bulk CSV (.tar.zst)", path: "/graph/export/bulk-csv" },
  { key: "cypher", label: "Cypher script", path: "/graph/export/cypher" },
  { key: "jsonl", label: "JSON Lines", path: "/graph/export/jsonl" },
  { key: "json-graph", label: "JSON graph", path: "/graph/export/json-graph" },
  { key: "rdf-turtle", label: "RDF (Turtle)", path: "/graph/export/rdf", rdfFormat: "turtle" },
  { key: "rdf-jsonld", label: "RDF (JSON-LD)", path: "/graph/export/rdf", rdfFormat: "jsonld" },
  {
    key: "rdf-ntriples",
    label: "RDF (N-Triples)",
    path: "/graph/export/rdf",
    rdfFormat: "ntriples",
  },
  { key: "rdf-nquads", label: "RDF (N-Quads)", path: "/graph/export/rdf", rdfFormat: "nquads" },
];

/** `scope`/`asOf`, both optional — `null` means "not set", matching how
 *  every other optional filter in this console is represented (never an
 *  empty string doing double duty as "unset"). */
export interface ExportFilters {
  readonly scope: string | null;
  readonly asOf: string | null;
}

/** The query string a format + filter pair produces — `?format=` first
 *  when the format itself requires one (RDF), so the same three-parameter
 *  shape every export route accepts is built identically regardless of
 *  which format is chosen.
 */
export function exportQueryString(format: ExportFormat, filters: ExportFilters): string {
  const params = new URLSearchParams();
  if (format.rdfFormat) params.set("format", format.rdfFormat);
  if (filters.scope) params.set("scope", filters.scope);
  if (filters.asOf) params.set("asOf", filters.asOf);
  const query = params.toString();
  return query ? `?${query}` : "";
}

/** The full path (no host, no `BASE` prefix — the caller's job, since only
 *  the caller knows whether it is running against the dev proxy) a
 *  download link should point at.
 */
export function exportPath(format: ExportFormat, filters: ExportFilters): string {
  return `${format.path}${exportQueryString(format, filters)}`;
}

/** The preview route's own path — same filters, no format (the count is
 *  identical across every format, since all six read through the same
 *  authorized/scoped/as-of element set).
 */
export function previewPath(filters: ExportFilters): string {
  const params = new URLSearchParams();
  if (filters.scope) params.set("scope", filters.scope);
  if (filters.asOf) params.set("asOf", filters.asOf);
  const query = params.toString();
  return `/graph/export/preview${query ? `?${query}` : ""}`;
}
