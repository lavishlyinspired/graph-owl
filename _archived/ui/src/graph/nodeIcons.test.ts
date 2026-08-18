import { describe, expect, it } from "vitest";
import { iconCategoryFor, iconDataUri } from "./nodeIcons";

describe("iconCategoryFor", () => {
  it("recognises a person-shaped entity name", () => {
    expect(iconCategoryFor("Customer")).toBe("person");
    expect(iconCategoryFor("Employee")).toBe("person");
  });

  it("recognises an organization-shaped entity name", () => {
    expect(iconCategoryFor("Supplier")).toBe("organization");
    expect(iconCategoryFor("Vendor Account")).toBe("organization");
  });

  it("recognises a location-shaped entity name", () => {
    expect(iconCategoryFor("Warehouse")).toBe("location");
  });

  it("recognises a money-shaped entity name", () => {
    expect(iconCategoryFor("Invoice")).toBe("money");
    expect(iconCategoryFor("Tax Return")).toBe("money");
  });

  it("recognises an event-shaped entity name", () => {
    expect(iconCategoryFor("Transaction")).toBe("event");
  });

  it("falls back to generic for an unrecognised name", () => {
    expect(iconCategoryFor("Widget")).toBe("generic");
  });

  it("is case-insensitive", () => {
    expect(iconCategoryFor("CUSTOMER")).toBe("person");
  });
});

describe("iconDataUri", () => {
  it("produces an inline SVG data URI", () => {
    const uri = iconDataUri("person", "#2a78d6");
    expect(uri).toMatch(/^data:image\/svg\+xml,/);
  });

  it("produces a distinct glyph per category", () => {
    const person = iconDataUri("person", "#2a78d6");
    const organization = iconDataUri("organization", "#2a78d6");
    expect(person).not.toBe(organization);
  });

  it("bakes the given colour into the icon stroke", () => {
    const uri = iconDataUri("generic", "#ff0000");
    expect(decodeURIComponent(uri)).toContain("#ff0000");
  });

  it("changes output when the colour changes but the category does not", () => {
    const red = iconDataUri("generic", "#ff0000");
    const blue = iconDataUri("generic", "#0000ff");
    expect(red).not.toBe(blue);
  });

  it("gives the SVG an explicit width and height, not just a viewBox", () => {
    // Cytoscape's background-fit sizes from the image's own intrinsic
    // dimensions; a viewBox-only SVG gets an ambiguous one in some
    // browsers, which crops and off-centres the rendered icon.
    const decoded = decodeURIComponent(iconDataUri("generic", "#000000"));
    expect(decoded).toMatch(/width="24"/);
    expect(decoded).toMatch(/height="24"/);
  });
});
