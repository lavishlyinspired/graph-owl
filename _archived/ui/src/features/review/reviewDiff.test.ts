import { describe, expect, it } from "vitest";
import { compareAssets } from "./reviewDiff";
import type { Asset } from "../../api";

function asset(overrides: Partial<Asset> = {}): Asset {
  return {
    id: "11111111-1111-1111-1111-111111111111",
    kind: "table",
    name: "orders",
    fullyQualifiedName: "warehouse.public.orders",
    parentId: "22222222-2222-2222-2222-222222222222",
    description: "Customer orders.",
    properties: { rowCount: 1000 },
    version: { major: 1, minor: 0 },
    updatedBy: "system",
    deleted: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("compareAssets", () => {
  it("marks every field as matching when target and candidate agree on all of them", () => {
    const target = asset();
    const candidate = asset();

    const comparison = compareAssets(target, candidate);

    expect(comparison.every((field) => field.matches)).toBe(true);
  });

  it("labels every compared field for display, in a stable order", () => {
    const comparison = compareAssets(asset(), asset());

    expect(comparison.map((field) => ({ field: field.field, label: field.label }))).toEqual([
      { field: "name", label: "Name" },
      { field: "fullyQualifiedName", label: "Fully qualified name" },
      { field: "kind", label: "Kind" },
      { field: "parentId", label: "Parent" },
      { field: "description", label: "Description" },
      { field: "properties", label: "Properties" },
    ]);
  });

  it("flags a field whose values differ between target and candidate, and only that field", () => {
    const target = asset({ name: "orders" });
    const candidate = asset({ name: "orders_v2" });

    const comparison = compareAssets(target, candidate);

    const nameField = comparison.find((field) => field.field === "name");
    expect(nameField?.matches).toBe(false);
    expect(nameField?.targetValue).toBe("orders");
    expect(nameField?.candidateValue).toBe("orders_v2");
    expect(comparison.filter((field) => !field.matches)).toHaveLength(1);
  });

  it("flags fullyQualifiedName, kind and parentId independently of one another", () => {
    const target = asset({
      fullyQualifiedName: "warehouse.public.orders",
      kind: "table",
      parentId: "22222222-2222-2222-2222-222222222222",
    });
    const candidate = asset({
      fullyQualifiedName: "warehouse.staging.orders",
      kind: "column",
      parentId: "33333333-3333-3333-3333-333333333333",
    });

    const comparison = compareAssets(target, candidate);

    expect(comparison.find((field) => field.field === "fullyQualifiedName")?.matches).toBe(false);
    expect(comparison.find((field) => field.field === "kind")?.matches).toBe(false);
    expect(comparison.find((field) => field.field === "parentId")?.matches).toBe(false);
  });

  it("treats two null descriptions as matching, not as a conflict", () => {
    const target = asset({ description: null });
    const candidate = asset({ description: null });

    const comparison = compareAssets(target, candidate);

    expect(comparison.find((field) => field.field === "description")?.matches).toBe(true);
  });

  it("flags a null description against a present one", () => {
    const target = asset({ description: null });
    const candidate = asset({ description: "Customer orders." });

    const comparison = compareAssets(target, candidate);

    const field = comparison.find((field) => field.field === "description");
    expect(field?.matches).toBe(false);
    expect(field?.targetValue).toBe("");
    expect(field?.candidateValue).toBe("Customer orders.");
  });

  it("treats equal properties as matching regardless of key order", () => {
    const target = asset({ properties: { a: 1, b: 2 } });
    const candidate = asset({ properties: { b: 2, a: 1 } });

    const comparison = compareAssets(target, candidate);

    expect(comparison.find((field) => field.field === "properties")?.matches).toBe(true);
  });

  it("flags properties that genuinely differ", () => {
    const target = asset({ properties: { rowCount: 1000 } });
    const candidate = asset({ properties: { rowCount: 2000 } });

    const comparison = compareAssets(target, candidate);

    expect(comparison.find((field) => field.field === "properties")?.matches).toBe(false);
  });

  it("treats absent properties as matching an empty object, not as a conflict", () => {
    const target = asset({ properties: undefined });
    const candidate = asset({ properties: {} });

    const comparison = compareAssets(target, candidate);

    expect(comparison.find((field) => field.field === "properties")?.matches).toBe(true);
  });
});
