/** Pure state transformations for the ontology builder.
 *
 *  The prototype keeps the model in component state backed by `localStorage`.
 *  These functions are extracted and tested so the component only wires
 *  them to the UI. */

import type {
  Attribute,
  Cardinality,
  DataType,
  EntityType,
  InteractionType,
  OntologyModel,
  ReferenceDatum,
  Relationship,
  SourceSystem,
} from "./types";

const STORAGE_KEY = "graph-owl.ontology-builder.v1";

function nextId(): string {
  return crypto.randomUUID();
}

export const EMPTY_MODEL: OntologyModel = {
  entityTypes: [],
  relationships: [],
  interactions: [],
  referenceData: [],
  sources: [],
};

export function loadModel(): OntologyModel {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return EMPTY_MODEL;
    const parsed = JSON.parse(raw) as OntologyModel;
    return {
      entityTypes: parsed.entityTypes ?? [],
      relationships: parsed.relationships ?? [],
      interactions: parsed.interactions ?? [],
      referenceData: parsed.referenceData ?? [],
      sources: parsed.sources ?? [],
    };
  } catch {
    return EMPTY_MODEL;
  }
}

export function saveModel(model: OntologyModel): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(model));
}

export function exportJson(model: OntologyModel): string {
  return JSON.stringify(model, null, 2);
}

export function importJson(text: string): OntologyModel {
  const parsed = JSON.parse(text) as OntologyModel;
  return {
    entityTypes: parsed.entityTypes ?? [],
    relationships: parsed.relationships ?? [],
    interactions: parsed.interactions ?? [],
    referenceData: parsed.referenceData ?? [],
    sources: parsed.sources ?? [],
  };
}

export function addEntityType(
  model: OntologyModel,
  draft: Omit<EntityType, "id" | "attributes">,
): OntologyModel {
  const entity: EntityType = { ...draft, id: nextId(), attributes: [] };
  return { ...model, entityTypes: [...model.entityTypes, entity] };
}

export function updateEntityType(
  model: OntologyModel,
  id: string,
  patch: Partial<Omit<EntityType, "id" | "attributes">>,
): OntologyModel {
  return {
    ...model,
    entityTypes: model.entityTypes.map((et) => (et.id === id ? { ...et, ...patch } : et)),
  };
}

export function removeEntityType(model: OntologyModel, id: string): OntologyModel {
  return {
    ...model,
    entityTypes: model.entityTypes.filter((et) => et.id !== id),
    relationships: model.relationships.filter(
      (r) => r.fromEntityTypeId !== id && r.toEntityTypeId !== id,
    ),
  };
}

export function addAttribute(
  model: OntologyModel,
  entityTypeId: string,
  draft: Omit<Attribute, "id">,
): OntologyModel {
  const attribute: Attribute = { ...draft, id: nextId() };
  return {
    ...model,
    entityTypes: model.entityTypes.map((et) =>
      et.id === entityTypeId ? { ...et, attributes: [...et.attributes, attribute] } : et,
    ),
  };
}

export function updateAttribute(
  model: OntologyModel,
  entityTypeId: string,
  attributeId: string,
  patch: Partial<Omit<Attribute, "id">>,
): OntologyModel {
  return {
    ...model,
    entityTypes: model.entityTypes.map((et) =>
      et.id === entityTypeId
        ? {
            ...et,
            attributes: et.attributes.map((a) =>
              a.id === attributeId ? { ...a, ...patch } : a,
            ),
          }
        : et,
    ),
  };
}

export function removeAttribute(
  model: OntologyModel,
  entityTypeId: string,
  attributeId: string,
): OntologyModel {
  return {
    ...model,
    entityTypes: model.entityTypes.map((et) =>
      et.id === entityTypeId
        ? { ...et, attributes: et.attributes.filter((a) => a.id !== attributeId) }
        : et,
    ),
  };
}

export function addRelationship(
  model: OntologyModel,
  draft: Omit<Relationship, "id">,
): OntologyModel {
  const relationship: Relationship = { ...draft, id: nextId() };
  return { ...model, relationships: [...model.relationships, relationship] };
}

export function updateRelationship(
  model: OntologyModel,
  id: string,
  patch: Partial<Omit<Relationship, "id">>,
): OntologyModel {
  return {
    ...model,
    relationships: model.relationships.map((r) => (r.id === id ? { ...r, ...patch } : r)),
  };
}

export function removeRelationship(model: OntologyModel, id: string): OntologyModel {
  return { ...model, relationships: model.relationships.filter((r) => r.id !== id) };
}

export function addInteraction(
  model: OntologyModel,
  draft: Omit<InteractionType, "id">,
): OntologyModel {
  const interaction: InteractionType = { ...draft, id: nextId() };
  return { ...model, interactions: [...model.interactions, interaction] };
}

export function removeInteraction(model: OntologyModel, id: string): OntologyModel {
  return { ...model, interactions: model.interactions.filter((i) => i.id !== id) };
}

export function addReferenceDatum(
  model: OntologyModel,
  draft: Omit<ReferenceDatum, "id">,
): OntologyModel {
  const datum: ReferenceDatum = { ...draft, id: nextId() };
  return { ...model, referenceData: [...model.referenceData, datum] };
}

export function removeReferenceDatum(model: OntologyModel, id: string): OntologyModel {
  return { ...model, referenceData: model.referenceData.filter((d) => d.id !== id) };
}

export function setSources(
  model: OntologyModel,
  sources: readonly SourceSystem[],
): OntologyModel {
  return { ...model, sources };
}

/** A deterministic palette for entity-type nodes. Reuses the same
 *  validated categorical slots the Explorer uses for semantic types, so
 *  colour is consistent across graph surfaces. */
const NODE_COLORS = [
  "#2a78d6",
  "#eb6834",
  "#1baf7a",
  "#eda100",
  "#e87ba4",
  "#008300",
  "#4a3aa7",
  "#e34948",
  "#0FAAB5",
  "#6C74D8",
] as const;

export function nextEntityColor(index: number): string {
  return NODE_COLORS[index % NODE_COLORS.length]!;
}

export function makeDefaultRelationshipName(from: string, to: string): string {
  return `relatesTo${from}${to}`;
}

export const CARDINALITY_LABELS: Record<Cardinality, string> = {
  oneToOne: "1:1",
  oneToMany: "1:N",
  manyToOne: "N:1",
  manyToMany: "N:M",
};

export const DATA_TYPE_LABELS: Record<DataType, string> = {
  string: "String",
  integer: "Integer",
  float: "Float",
  boolean: "Boolean",
  date: "Date",
  reference: "Reference",
  json: "JSON",
};
