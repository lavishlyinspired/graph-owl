/** Turn an ontology model into React Flow elements.
 *
 *  Replaces `cytoscapeModel.ts` — 00f-ui-architecture.md's 14 Aug 2026
 *  revision moves this canvas from Cytoscape to React Flow. The model
 *  (nodes, edges, colour, icon) stays pure and renderer-shaped-but-not
 *  renderer-coupled, so tests assert structure without mounting a canvas,
 *  same discipline `graph/cytoscape.ts` established. Positions are not
 *  computed here — `layout.ts` does that, kept separate so a layout
 *  algorithm change never has to touch what a node or edge *is*. */

import { canvasLabel } from "../../graph/bidiLabel";
import { iconCategoryFor, iconDataUri } from "../../graph/nodeIcons";
import type { EntityType, OntologyModel, Relationship } from "./types";

export interface FlowNodeData {
  readonly id: string;
  readonly label: string;
  readonly color: string;
  readonly icon: string;
  // React Flow's `Node<T>` constrains `T extends Record<string, unknown>` —
  // this satisfies that without weakening the four named fields above,
  // which stay the actual contract every caller and test reads.
  readonly [key: string]: unknown;
}

export interface FlowEdgeData {
  readonly id: string;
  readonly source: string;
  readonly target: string;
  readonly label: string;
  readonly selfLoop: boolean;
}

export interface FlowElements {
  readonly nodes: readonly FlowNodeData[];
  readonly edges: readonly FlowEdgeData[];
}

function entityLabel(entity: EntityType): string {
  return canvasLabel(entity.displayName || entity.name);
}

export function toFlowElements(model: OntologyModel): FlowElements {
  const nodes: FlowNodeData[] = model.entityTypes.map((entity) => ({
    id: entity.id,
    label: entityLabel(entity),
    color: entity.color,
    icon: iconDataUri(iconCategoryFor(entity.name || entity.displayName), entity.color),
  }));

  const edges: FlowEdgeData[] = model.relationships.map((rel) => ({
    id: rel.id,
    source: rel.fromEntityTypeId,
    target: rel.toEntityTypeId,
    label: canvasLabel(rel.displayName || rel.name),
    selfLoop: rel.fromEntityTypeId === rel.toEntityTypeId,
  }));

  return { nodes, edges };
}

export function relationshipById(model: OntologyModel, id: string): Relationship | undefined {
  return model.relationships.find((r) => r.id === id);
}

export function entityById(model: OntologyModel, id: string): EntityType | undefined {
  return model.entityTypes.find((e) => e.id === id);
}

/** An IRI's own namespace prefix — the last `#`, else the last `/`, else
 *  the whole string when neither separator exists. Plan 120 Slice B: the
 *  identical algorithm the deleted `ontologyDocument.ts`'s `namespaceOf`
 *  used, restored rather than reinvented. */
export function namespaceOf(iri: string): string {
  const hash = iri.lastIndexOf("#");
  if (hash !== -1) return iri.slice(0, hash + 1);
  const slash = iri.lastIndexOf("/");
  return slash !== -1 ? iri.slice(0, slash + 1) : iri;
}

/** Every distinct namespace an entity in this model was declared under,
 *  sorted. A manually added entity (`namespace: null`) contributes nothing
 *  — it has no namespace to offer the filter a choice of. */
export function namespacesIn(model: OntologyModel): readonly string[] {
  const namespaces = new Set<string>();
  for (const entity of model.entityTypes) {
    if (entity.namespace !== null) namespaces.add(entity.namespace);
  }
  return [...namespaces].sort();
}

/** The model narrowed to one namespace's own entities, for the graph
 *  view's namespace filter — Plan 120 Slice B. `null` means "all
 *  namespaces" and returns the model unchanged, matching the filter's own
 *  "no selection" default.
 *
 *  **A relationship survives only when both endpoints do.** The old,
 *  triple-level filter (`ontologyDocument.ts`, deleted) kept an edge when
 *  either its subject or its predicate matched; entities and relationships
 *  are separate collections now, and a relationship has no namespace of
 *  its own to test — an edge pointing at a node the filter just hid would
 *  be a dangling reference the canvas cannot draw correctly, so both ends
 *  must stay for the edge to stay. */
export function filterModelByNamespace(
  model: OntologyModel,
  namespace: string | null,
): OntologyModel {
  if (namespace === null) return model;
  const keptIds = new Set(
    model.entityTypes.filter((e) => e.namespace === namespace).map((e) => e.id),
  );
  return {
    ...model,
    entityTypes: model.entityTypes.filter((e) => keptIds.has(e.id)),
    relationships: model.relationships.filter(
      (r) => keptIds.has(r.fromEntityTypeId) && keptIds.has(r.toEntityTypeId),
    ),
  };
}
