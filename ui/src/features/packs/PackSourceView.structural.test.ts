/** The source view opened in Explore's content pane — Plan 115 Slice B2.
 *
 *  Structural, like `PackDataExplorer.structural.test.ts`: the view renders
 *  only against a real graph (`/sparql`), and what it *displays* is pure
 *  parsing already unit-tested in `packData.test.ts`. This pins the wiring —
 *  that the view reads the source's own graph by the IRI the wire reported,
 *  drills into a subject's neighbourhood in place, and keeps the
 *  reconciliation one explicit action away. */

import { describe, expect, it } from "vitest";
import source from "./PackSourceView.tsx?raw";

describe("the source view in Explore's content pane", () => {
  it("reads the source's own graph, scoped to the IRI the wire reported", () => {
    expect(source).toMatch(/subjectsQuery\(source\.iri\)/);
    expect(source).toMatch(/typesQuery\(source\.iri\)/);
    expect(source).toMatch(/subjectsFromSparql/);
  });

  it("opens a subject's neighbourhood in place via SubjectExplorer", () => {
    expect(source).toMatch(/SubjectExplorer/);
    expect(source).toMatch(/seed=\{open\.iri\}/);
  });

  it("keeps the reconciliation one explicit action away", () => {
    expect(source).toMatch(/onReconcile/);
  });
});
