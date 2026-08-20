import { describe, expect, it } from "vitest";
import { toPathsConfig, type TraceConfig } from "./trace";

/** The plan's own RED description, narrowed: this was once "a
 *  config-driven test table drives all four surfaces through the same
 *  assertions" (Lineage, Paths, History, Evidence). Only Paths still
 *  reduces to this shape — the other three are Explore's own Entity tab
 *  now, with real per-entity data this generic config never carried. */
function assertsAsATraceConfig(config: TraceConfig) {
  expect(config.title.length).toBeGreaterThan(0);
  expect(config.kpis.length).toBeGreaterThan(0);
  expect(config.columns.length).toBeGreaterThan(0);
}

describe("paths — Plan 122a A4", () => {
  it("passes the shared config assertions", () => {
    const config = toPathsConfig({ paths: [{ nodes: ["a", "b", "c"], length: 2 }], truncated: false }, "a", "c");
    assertsAsATraceConfig(config);
  });

  it("counts the paths actually found, not a fixed number", () => {
    const config = toPathsConfig(
      { paths: [{ nodes: ["a", "b"], length: 1 }, { nodes: ["a", "c", "b"], length: 2 }], truncated: false },
      "a",
      "b",
    );
    expect(config.kpis.find((k) => k.label === "PATHS FOUND")?.value).toBe("2");
    expect(config.rows).toHaveLength(2);
  });

  it("shows every path found, including the weakest, ranked last rather than hidden", () => {
    const config = toPathsConfig(
      { paths: [{ nodes: ["a", "b"], length: 1 }, { nodes: ["a", "x", "y", "b"], length: 3 }], truncated: false },
      "a",
      "b",
    );
    expect(config.rows).toHaveLength(2);
    expect(config.rows[1]?.cells.some((c) => c.text.includes("x"))).toBe(true);
  });

  it("is empty, not broken, when no path connects the two entities", () => {
    const config = toPathsConfig({ paths: [], truncated: false }, "a", "z");
    expect(config.rows).toEqual([]);
  });
});
