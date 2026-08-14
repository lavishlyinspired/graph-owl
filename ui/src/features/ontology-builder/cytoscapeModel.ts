/** Turn an ontology model into Cytoscape elements.
 *
 *  Following `graph/cytoscape.ts`, the model (nodes, edges, classes) is
 *  pure so tests can assert structure without rendering. */

import { canvasLabel } from "../../graph/bidiLabel";
import type { EntityType, OntologyModel, Relationship } from "./types";

export interface OntologyElement {
  readonly group: "nodes" | "edges";
  readonly data: {
    readonly id: string;
    readonly label?: string;
    readonly source?: string;
    readonly target?: string;
    readonly color?: string;
  };
  readonly classes: string;
}

function entityLabel(entity: EntityType): string {
  return canvasLabel(entity.displayName || entity.name);
}

export function toCytoscapeElements(model: OntologyModel): OntologyElement[] {
  const nodes: OntologyElement[] = model.entityTypes.map((entity) => ({
    group: "nodes",
    data: {
      id: entity.id,
      label: entityLabel(entity),
      color: entity.color,
    },
    classes: "entity-type",
  }));

  const edges: OntologyElement[] = model.relationships.map((rel) => ({
    group: "edges",
    data: {
      id: rel.id,
      source: rel.fromEntityTypeId,
      target: rel.toEntityTypeId,
      label: canvasLabel(rel.displayName || rel.name),
    },
    classes: rel.fromEntityTypeId === rel.toEntityTypeId ? "self-loop" : "relationship",
  }));

  return [...nodes, ...edges];
}

export function relationshipById(
  model: OntologyModel,
  id: string,
): Relationship | undefined {
  return model.relationships.find((r) => r.id === id);
}

export function entityById(model: OntologyModel, id: string): EntityType | undefined {
  return model.entityTypes.find((e) => e.id === id);
}
