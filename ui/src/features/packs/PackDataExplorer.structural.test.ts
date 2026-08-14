/** The Explore "Pack data" block — Plan 115 Slice B1.
 *
 *  Structural, like `ReconciliationWorkspace.structural.test.ts`: the block
 *  renders only against a real graph (`/namespaces` + a named-graph query),
 *  and what it *displays* is pure parsing already unit-tested in
 *  `packData.test.ts`. This pins the wiring — that the block reads installed
 *  packs and the graph's own named import graphs, and that a source opens the
 *  reconciliation. */

import { describe, expect, it } from "vitest";
import source from "./PackDataExplorer.tsx?raw";

describe("the Pack data block in the Explore sider", () => {
  it("reads installed packs and the graph's own import graphs", () => {
    expect(source).toMatch(/api\.namespaces\(\)/);
    expect(source).toMatch(/installedPacks/);
    expect(source).toMatch(/loadedSourcesFromSparql/);
    expect(source).toMatch(/NAMED_GRAPHS_QUERY/);
  });

  it("files each loaded source under its own pack", () => {
    expect(source).toMatch(/sourcesForPack/);
    expect(source).toMatch(/pack\.packId/);
  });

  it("makes a source open the reconciliation", () => {
    expect(source).toMatch(/onOpen/);
  });
});
