/** Domain model for the visual ontology builder prototype.
 *
 *  This is a standalone client-side model: the server has no matching
 *  `/entity-types` or `/relationship-types` collection today, so the
 *  prototype persists locally and exports/import JSON that a future API
 *  can accept.
 *
 *  Entity types are the nodes; relationships are the edges. Attributes,
 *  interactions, reference data, and sources are the supporting vocabulary
 *  shown in the counts and detail panels. */

export type DataType =
  | "string"
  | "integer"
  | "float"
  | "boolean"
  | "date"
  | "reference"
  | "json";

export type Cardinality = "oneToOne" | "oneToMany" | "manyToOne" | "manyToMany";

export interface Attribute {
  readonly id: string;
  readonly name: string;
  readonly displayName: string;
  readonly description: string;
  readonly dataType: DataType;
  readonly required: boolean;
  /** When `dataType` is `reference`, the target entity type id. */
  readonly referenceToId: string | null;
}

export interface EntityType {
  readonly id: string;
  readonly name: string;
  readonly displayName: string;
  readonly description: string;
  /** A colour hex for the diagram node. */
  readonly color: string;
  readonly attributes: readonly Attribute[];
  /** The IRI prefix this class was declared under, when it came from an
   *  imported document (`namespaceOf` in `flowModel.ts`) — `null` for an
   *  entity added by hand in the builder, which has no source document to
   *  take one from. Plan 120 Slice B: what the namespace filter narrows
   *  the graph view by. */
  readonly namespace: string | null;
}

export interface Relationship {
  readonly id: string;
  readonly fromEntityTypeId: string;
  readonly toEntityTypeId: string;
  readonly name: string;
  readonly displayName: string;
  readonly description: string;
  readonly cardinality: Cardinality;
}

export interface InteractionType {
  readonly id: string;
  readonly name: string;
  readonly displayName: string;
  readonly description: string;
}

export interface ReferenceDatum {
  readonly id: string;
  readonly name: string;
  readonly displayName: string;
  readonly description: string;
}

export interface SourceSystem {
  readonly id: string;
  readonly name: string;
  readonly displayName: string;
}

export interface OntologyModel {
  readonly entityTypes: readonly EntityType[];
  readonly relationships: readonly Relationship[];
  readonly interactions: readonly InteractionType[];
  readonly referenceData: readonly ReferenceDatum[];
  readonly sources: readonly SourceSystem[];
}

export interface OntologyPackOption {
  readonly id: string;
  readonly name: string;
  readonly description: string | null;
}
