import { describe, expect, it } from "vitest";
import { evaluateBudgets } from "./evaluate-budgets.mjs";

// `00f-ui-architecture.md`'s revised numbers: initial JS 350KB gzipped,
// each route chunk 100KB gzipped, dependency count 40 — except a G6-backed
// route chunk (Plan 122a A3), which gets its own measured, documented
// budget instead. See `evaluate-budgets.mjs`'s own doc comment for why.

describe("evaluateBudgets", () => {
  it("passes a build comfortably inside every budget", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 300 * 1024,
      routeChunksGzip: [{ name: "workbench-abc123.js", bytesGzip: 90 * 1024 }],
      dependencyCount: 11,
    });
    expect(result.ok).toBe(true);
    expect(result.violations).toEqual([]);
  });

  it("fails the build on a deliberately oversized initial bundle", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 351 * 1024,
      routeChunksGzip: [],
      dependencyCount: 11,
    });
    expect(result.ok).toBe(false);
    expect(result.violations).toContainEqual(
      expect.objectContaining({ budget: "initial-bundle" }),
    );
  });

  it("is exact at the boundary — 350KB itself passes, 350KB + 1 byte fails", () => {
    const atBudget = evaluateBudgets({
      initialBytesGzip: 350 * 1024,
      routeChunksGzip: [],
      dependencyCount: 11,
    });
    expect(atBudget.ok).toBe(true);

    const overBudget = evaluateBudgets({
      initialBytesGzip: 350 * 1024 + 1,
      routeChunksGzip: [],
      dependencyCount: 11,
    });
    expect(overBudget.ok).toBe(false);
  });

  it("fails on an oversized route chunk and names which one", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 100 * 1024,
      routeChunksGzip: [
        { name: "workbench-abc123.js", bytesGzip: 50 * 1024 },
        { name: "packs-def456.js", bytesGzip: 101 * 1024 },
      ],
      dependencyCount: 11,
    });
    expect(result.ok).toBe(false);
    expect(result.violations).toContainEqual(
      expect.objectContaining({ budget: "route-chunk", detail: expect.stringContaining("packs-def456.js") }),
    );
  });

  it("fails when dependency count exceeds 40", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 100 * 1024,
      routeChunksGzip: [],
      dependencyCount: 41,
    });
    expect(result.ok).toBe(false);
    expect(result.violations).toContainEqual(expect.objectContaining({ budget: "dependency-count" }));
  });

  it("reports every violation at once rather than stopping at the first", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 400 * 1024,
      routeChunksGzip: [{ name: "packs-abc123.js", bytesGzip: 200 * 1024 }],
      dependencyCount: 50,
    });
    expect(result.violations).toHaveLength(3);
  });

  describe("the G6-backed route chunk exception (Plan 122a A3)", () => {
    it("passes an explore chunk that would fail the general 100KB budget", () => {
      const result = evaluateBudgets({
        initialBytesGzip: 100 * 1024,
        routeChunksGzip: [{ name: "explore-CH8ERkL2.js", bytesGzip: 412 * 1024 }],
        dependencyCount: 11,
      });
      expect(result.ok).toBe(true);
    });

    it("still fails an explore chunk that exceeds its own, larger budget", () => {
      const result = evaluateBudgets({
        initialBytesGzip: 100 * 1024,
        routeChunksGzip: [{ name: "explore-CH8ERkL2.js", bytesGzip: 451 * 1024 }],
        dependencyCount: 11,
      });
      expect(result.ok).toBe(false);
      expect(result.violations).toContainEqual(
        expect.objectContaining({ budget: "route-chunk", detail: expect.stringContaining("explore-CH8ERkL2.js") }),
      );
    });

    /** The exception is scoped by chunk name, not applied blanket. A
     *  route that happens to be large for an unrelated reason must still
     *  be caught by the general budget — otherwise the exception is a
     *  hole any future route could grow into unnoticed. */
    it("does not extend the larger budget to an unrelated oversized chunk", () => {
      const result = evaluateBudgets({
        initialBytesGzip: 100 * 1024,
        routeChunksGzip: [{ name: "workbench-abc123.js", bytesGzip: 200 * 1024 }],
        dependencyCount: 11,
      });
      expect(result.ok).toBe(false);
      expect(result.violations).toContainEqual(
        expect.objectContaining({ detail: expect.stringContaining("budget is 100KB") }),
      );
    });

    /** A chunk name is matched by its stable route prefix before the
     *  content hash, not by substring — a route that merely *contains*
     *  "explore" somewhere in its name must not accidentally inherit the
     *  larger budget. */
    it("matches by route prefix, not by substring anywhere in the name", () => {
      const result = evaluateBudgets({
        initialBytesGzip: 100 * 1024,
        routeChunksGzip: [{ name: "unexplored-abc123.js", bytesGzip: 200 * 1024 }],
        dependencyCount: 11,
      });
      expect(result.ok).toBe(false);
    });
  });
});
