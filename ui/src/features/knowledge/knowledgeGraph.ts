/** Epic 42 Slice E: the triples ⇄ property-graph toggle on the Knowledge
 *  tab (decision 6 — a toggle on the existing tab, not a new screen). The
 *  triples side reuses `POST /sparql` (already generic) scoped to one
 *  subject client-side; the property-graph side is `GET
 *  /assets/{id}/lpg-node`, which carries the asset's own `MappingReport`.
 *
 *  **`describeLoss` is this slice's own named RED test**: a toggle that
 *  silently drops what does not map teaches a reader the two models are
 *  equivalent when Epic 7c's whole `MappingReport` exists to say they are
 *  not. Every `LossyMapping` variant must produce a specific description,
 *  never a blank string or a generic fallback that erases which kind of
 *  loss it was. */

import type { LossyMapping } from "../../api";

export type { LossyMapping };

const CATALOG_NAMESPACE = "https://graph-owl.dev/ns/catalog#";

export function assetIri(id: string): string {
  return `${CATALOG_NAMESPACE}${id}`;
}

export function outboundTriplesQuery(id: string): string {
  return `SELECT ?p ?o WHERE { <${assetIri(id)}> ?p ?o }`;
}

export function inboundTriplesQuery(id: string): string {
  return `SELECT ?s ?p WHERE { ?s ?p <${assetIri(id)}> }`;
}

function fromTypeLabel(from: "uuid" | "json"): string {
  return from === "uuid" ? "uuid" : "json";
}

export function describeLoss(loss: LossyMapping): string {
  switch (loss.kind) {
    case "refInProperty":
      return `The reference in "${loss.predicate}" was flattened to plain text — it no longer traverses as an edge.`;
    case "namedGraphCollapse": {
      const count = loss.graphs.length;
      const noun = count === 1 ? "named graph was" : "named graphs were";
      return `${count} ${noun} merged into one — ${count === 1 ? "its" : "their"} separation is gone.`;
    }
    case "typeNarrowed":
      return `"${loss.predicate}" narrowed from ${fromTypeLabel(loss.from)} to plain text.`;
  }
}
