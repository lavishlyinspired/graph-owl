/** Structural tests for the ontology builder: wiring, not rendering.
 *
 *  The component touches `localStorage` and Cytoscape, so unit tests here
 *  assert static wiring (which API it calls, which persistence key it uses)
 *  rather than mounting the component. */

import { describe, expect, it } from "vitest";
import source from "./OntologyBuilder.tsx?raw";
import stateSource from "./state.ts?raw";

describe("OntologyBuilder wiring", () => {
  it("loads installed ontology packs from the existing API", () => {
    expect(source).toMatch(/api\s*\.\s*ontologyPacks\s*\(\s*\)/s);
  });

  it("persists the model to localStorage with a versioned key", () => {
    expect(stateSource).toMatch(/graph-owl\.ontology-builder\.v1/);
    expect(stateSource).toMatch(/localStorage\.setItem/);
    expect(stateSource).toMatch(/localStorage\.getItem/);
  });

  it("exports and imports JSON for save/share", () => {
    expect(source).toMatch(/exportJson/);
    expect(source).toMatch(/importJson/);
  });

  it("offers the three layouts from the screenshots", () => {
    expect(source).toMatch(/radial/);
    expect(source).toMatch(/tree/);
    expect(source).toMatch(/force/);
  });

  it("offers polyline and orthogonal edge styles", () => {
    expect(source).toMatch(/polyline/);
    expect(source).toMatch(/orthogonal/);
  });
});
