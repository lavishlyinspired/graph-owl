import { describe, expect, it } from "vitest";
import type { OntologyModel } from "./types";
import {
  EMPTY_MODEL,
  addAttribute,
  addEntityType,
  addInteraction,
  addReferenceDatum,
  addRelationship,
  importJson,
  nextEntityColor,
  removeAttribute,
  removeEntityType,
  removeInteraction,
  removeReferenceDatum,
  removeRelationship,
  setSources,
  updateAttribute,
  updateEntityType,
  updateRelationship,
} from "./state";

function modelWithEntity(name: string): OntologyModel {
  return addEntityType(EMPTY_MODEL, {
    name,
    displayName: name,
    description: "",
    color: "#000000",
  });
}

describe("entity type CRUD", () => {
  it("adds an entity type", () => {
    const next = modelWithEntity("Customer");
    expect(next.entityTypes).toHaveLength(1);
    expect(next.entityTypes[0]!.name).toBe("Customer");
    expect(next.entityTypes[0]!.attributes).toEqual([]);
  });

  it("updates an entity type without touching attributes", () => {
    const withEntity = modelWithEntity("Customer");
    const next = updateEntityType(withEntity, withEntity.entityTypes[0]!.id, {
      displayName: "Individual Customer",
    });
    expect(next.entityTypes[0]!.displayName).toBe("Individual Customer");
    expect(next.entityTypes[0]!.attributes).toEqual([]);
  });

  it("removes an entity type and any relationship touching it", () => {
    let model = modelWithEntity("Customer");
    model = addEntityType(model, {
      name: "Order",
      displayName: "Order",
      description: "",
      color: "#000000",
    });
    model = addRelationship(model, {
      fromEntityTypeId: model.entityTypes[0]!.id,
      toEntityTypeId: model.entityTypes[1]!.id,
      name: "places",
      displayName: "places",
      description: "",
      cardinality: "oneToMany",
    });
    const next = removeEntityType(model, model.entityTypes[0]!.id);
    expect(next.entityTypes).toHaveLength(1);
    expect(next.relationships).toHaveLength(0);
  });
});

describe("attribute CRUD", () => {
  it("adds, updates, and removes an attribute", () => {
    let model = modelWithEntity("Customer");
    const entityId = model.entityTypes[0]!.id;
    model = addAttribute(model, entityId, {
      name: "email",
      displayName: "Email",
      description: "",
      dataType: "string",
      required: true,
      referenceToId: null,
    });
    const attrId = model.entityTypes[0]!.attributes[0]!.id;

    model = updateAttribute(model, entityId, attrId, { required: false });
    expect(model.entityTypes[0]!.attributes[0]!.required).toBe(false);

    model = removeAttribute(model, entityId, attrId);
    expect(model.entityTypes[0]!.attributes).toHaveLength(0);
  });
});

describe("relationship CRUD", () => {
  it("adds, updates, and removes a relationship", () => {
    let model = modelWithEntity("Customer");
    model = addEntityType(model, {
      name: "Order",
      displayName: "Order",
      description: "",
      color: "#000000",
    });
    model = addRelationship(model, {
      fromEntityTypeId: model.entityTypes[0]!.id,
      toEntityTypeId: model.entityTypes[1]!.id,
      name: "places",
      displayName: "places",
      description: "",
      cardinality: "oneToMany",
    });
    const relId = model.relationships[0]!.id;

    model = updateRelationship(model, relId, { cardinality: "manyToMany" });
    expect(model.relationships[0]!.cardinality).toBe("manyToMany");

    model = removeRelationship(model, relId);
    expect(model.relationships).toHaveLength(0);
  });
});

describe("supporting vocabulary CRUD", () => {
  it("adds and removes interactions", () => {
    let model = addInteraction(EMPTY_MODEL, {
      name: "emailSent",
      displayName: "Email sent",
      description: "",
    });
    expect(model.interactions).toHaveLength(1);
    model = removeInteraction(model, model.interactions[0]!.id);
    expect(model.interactions).toHaveLength(0);
  });

  it("adds and removes reference data", () => {
    let model = addReferenceDatum(EMPTY_MODEL, {
      name: "Countries",
      displayName: "Countries",
      description: "",
    });
    expect(model.referenceData).toHaveLength(1);
    model = removeReferenceDatum(model, model.referenceData[0]!.id);
    expect(model.referenceData).toHaveLength(0);
  });

  it("replaces sources", () => {
    const model = setSources(EMPTY_MODEL, [
      { id: "s1", name: "CRM", displayName: "CRM" },
    ]);
    expect(model.sources).toHaveLength(1);
  });
});

describe("import/export", () => {
  it("round-trips an ontology through JSON", () => {
    let model = modelWithEntity("Customer");
    model = addEntityType(model, {
      name: "Order",
      displayName: "Order",
      description: "",
      color: "#000000",
    });
    const json = JSON.stringify(model);
    const imported = importJson(json);
    expect(imported.entityTypes).toHaveLength(2);
    expect(imported.entityTypes[0]!.name).toBe("Customer");
  });
});

describe("utilities", () => {
  it("cycles through a fixed palette for entity colours", () => {
    expect(nextEntityColor(0)).toBe("#2a78d6");
    expect(nextEntityColor(10)).toBe(nextEntityColor(0));
  });
});
