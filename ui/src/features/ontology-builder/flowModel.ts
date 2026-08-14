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
