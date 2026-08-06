import { describe, expect, it } from "vitest";
import { evaluateBudgets } from "./evaluate-budgets.mjs";

// `00f-ui-architecture.md`'s revised numbers: initial JS 350KB gzipped,
// each route chunk 100KB gzipped, dependency count 40. "What does not
// move: the route budget (30), the dependency budget (40), the route-chunk
// budget (100KB)" — only the initial figure was revised, from 250KB.

describe("evaluateBudgets", () => {
  it("passes a build comfortably inside every budget", () => {
    const result = evaluateBudgets({
      initialBytesGzip: 300 * 1024,
      routeChunksGzip: [{ name: "explorer", bytesGzip: 90 * 1024 }],
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
        { name: "workbench", bytesGzip: 50 * 1024 },
        { name: "explorer", bytesGzip: 101 * 1024 },
      ],
      dependencyCount: 11,
    });
    expect(result.ok).toBe(false);
    expect(result.violations).toContainEqual(
      expect.objectContaining({ budget: "route-chunk", detail: expect.stringContaining("explorer") }),
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
      routeChunksGzip: [{ name: "explorer", bytesGzip: 200 * 1024 }],
      dependencyCount: 50,
    });
    expect(result.violations).toHaveLength(3);
  });
});
