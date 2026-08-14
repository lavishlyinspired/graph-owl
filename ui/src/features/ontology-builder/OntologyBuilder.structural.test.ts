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

  it("exports and imports every format formats.ts supports, not just JSON", () => {
    expect(source).toMatch(/exportModel/);
    expect(source).toMatch(/importModel/);
    expect(source).toMatch(/FORMAT_LABELS/);
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

  it("shows layout, edge style, and format as Select lists, not segmented pills", () => {
    expect(source).not.toMatch(/Segmented/);
    expect(source).toMatch(/<Select\b/);
  });

  it("shows the stat row as plain stat tiles rather than badge-on-text", () => {
    expect(source).not.toMatch(/Badge/);
  });

  it("discovers installed packs and their loaded sources the same way the ontology editor does", () => {
    expect(source).toMatch(/installedPacks/);
    expect(source).toMatch(/api\s*\.\s*namespaces\s*\(\s*\)/s);
    expect(source).toMatch(/NAMED_GRAPHS_QUERY/);
    expect(source).toMatch(/loadedSourcesFromSparql/);
  });

  it("loads a selected pack's own ontology graph into the canvas", () => {
    expect(source).toMatch(/ontologySourceFor/);
    expect(source).toMatch(/triplesQuery/);
    expect(source).toMatch(/ntriplesFromRows/);
    expect(source).toMatch(/importNTriples/);
  });

  it("shows packs as a visual picker, not a bare Select", () => {
    expect(source).toMatch(/PackPicker/);
  });
});

describe("OntologyBuilder canvas control layout — Plan 117", () => {
  it("has no separate full-width toolbar row above the canvas", () => {
    // The toolbar row this replaces was its own top-level Flex, marked by
    // this comment. Removing it (not just relabelling it) is the point:
    // that row's height is exactly the space this slice gives back.
    expect(source).not.toMatch(/\{\/\*\s*Toolbar\s*\*\/\}/);
  });

  it("positions the canvas wrapper as the anchor for floating controls", () => {
    // Tailwind's `relative` utility class, not necessarily an inline style —
    // either mechanism satisfies "this element is a positioning context".
    expect(source).toMatch(/position:\s*"relative"|className="[^"]*\brelative\b/);
  });

  it("floats the create/filter controls over the canvas, not in outer chrome", () => {
    const overlay = source.match(/\{\/\*\s*Canvas overlay — create \+ filter\s*\*\/\}[\s\S]{0,400}/);
    expect(overlay).not.toBeNull();
    expect(overlay![0]).toMatch(/position:\s*"absolute"|className="[^"]*\babsolute\b/);
    expect(overlay![0]).toMatch(/AddEntityTypeDialog/);
    expect(overlay![0]).toMatch(/AddRelationshipDialog/);
  });

  it("floats layout, edges, and reset over the canvas, not in outer chrome", () => {
    const overlay = source.match(
      /\{\/\*\s*Canvas overlay — view controls\s*\*\/\}[\s\S]*?(?=\{model\.entityTypes\.length === 0)/,
    );
    expect(overlay).not.toBeNull();
    expect(overlay![0]).toMatch(/position:\s*"absolute"|className="[^"]*\babsolute\b/);
    expect(overlay![0]).toMatch(/LAYOUT_OPTIONS/);
    expect(overlay![0]).toMatch(/EDGE_OPTIONS/);
  });

  it("keeps create/filter controls reachable even before any entity type exists", () => {
    // The overlay must render unconditionally alongside the empty state, not
    // only inside the branch that requires entityTypes.length > 0 — otherwise
    // there is no way to add the very first entity type.
    const emptyBranch = source.match(/entityTypes\.length === 0[\s\S]*?Canvas overlay — create \+ filter/);
    expect(emptyBranch).toBeNull();
    expect(source).toMatch(/Canvas overlay — create \+ filter[\s\S]*?entityTypes\.length === 0/);
  });

  it("moves format/export/import into the header, not a canvas overlay", () => {
    // Up to the next top-level chrome section (the notice banner / Tabs),
    // not a fixed character budget — the stat tiles between the pack picker
    // and the format/export/import controls are legitimately verbose.
    const header = source.match(/\{\/\*\s*Header\s*\*\/\}[\s\S]*?(?=<\/div>\s*\n\s*(\{notice|<Tabs))/);
    expect(header).not.toBeNull();
    expect(header![0]).toMatch(/handleExport/);
    expect(header![0]).toMatch(/handleImport/);
  });
});
