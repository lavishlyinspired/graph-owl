import { describe, expect, it } from "vitest";
import { toEvidenceConfig, toHistoryConfig, toLineageConfig, toPathsConfig, type TraceConfig } from "./trace";
import type { AssetVersion, Finding, LineageGraph } from "./api";

/** The plan's own RED description: "a config-driven test table drives all
 *  four surfaces through the same assertions." Each screen's config is
 *  structurally the same shape (`TraceConfig`), so the same three
 *  assertions apply to all four — this is the test that would catch a
 *  screen silently collapsing into an empty or malformed config, the
 *  failure mode the plan names explicitly. */
function assertsAsATraceConfig(config: TraceConfig) {
  expect(config.title.length).toBeGreaterThan(0);
  expect(config.kpis.length).toBeGreaterThan(0);
  expect(config.columns.length).toBeGreaterThan(0);
}

describe("lineage — Plan 122a A4", () => {
  const graph: LineageGraph = {
    rootId: "root",
    nodes: [
      { id: "root", name: "supplier.gstin", kind: "column", fullyQualifiedName: "supplier.gstin", deleted: false },
      { id: "up1", name: "raw.gst_returns", kind: "table", fullyQualifiedName: "raw.gst_returns", deleted: false },
      { id: "down1", name: "reco.matching_key", kind: "table", fullyQualifiedName: "reco.matching_key", deleted: false },
    ],
    edges: [
      {
        id: "e1",
        fromAssetId: "up1",
        toAssetId: "root",
        relationship: "feeds",
        source: "connector",
        createdAt: "2026-08-01T00:00:00Z",
        createdBy: "connector:postgres",
      },
      {
        id: "e2",
        fromAssetId: "root",
        toAssetId: "down1",
        relationship: "feeds",
        source: "manual",
        createdAt: "2026-08-10T00:00:00Z",
        createdBy: "asha",
      },
    ],
    truncated: false,
  };

  it("passes the shared config assertions", () => {
    assertsAsATraceConfig(toLineageConfig(graph, "supplier.gstin"));
  });

  it("counts upstream and downstream edges separately, from the root's own perspective", () => {
    const config = toLineageConfig(graph, "supplier.gstin");
    expect(config.kpis.find((k) => k.label === "UPSTREAM")?.value).toBe("1");
    expect(config.kpis.find((k) => k.label === "DOWNSTREAM")?.value).toBe("1");
  });

  /** Mutator: a config that reported total edge count for both KPIs, rather
   *  than genuine hop depth from the root, would still show "1" for
   *  upstream here even though the chain is two hops deep — this passes
   *  only when the KPI is derived from the same walk that draws the
   *  breadcrumb below it, so the two never contradict each other on
   *  screen. */
  it("does not report the same count for both directions when their hop depth genuinely differs", () => {
    const asymmetric: LineageGraph = {
      rootId: "root",
      nodes: [
        ...graph.nodes,
        { id: "up0", name: "gst.portal_api", kind: "service", fullyQualifiedName: "gst.portal_api", deleted: false },
      ],
      edges: [
        ...graph.edges,
        {
          id: "e0",
          fromAssetId: "up0",
          toAssetId: "up1",
          relationship: "feeds",
          source: "connector",
          createdAt: "2026-08-01T00:00:00Z",
          createdBy: "connector:postgres",
        },
      ],
      truncated: false,
    };
    const config = toLineageConfig(asymmetric, "supplier.gstin");
    const upstream = config.kpis.find((k) => k.label === "UPSTREAM")?.value;
    const downstream = config.kpis.find((k) => k.label === "DOWNSTREAM")?.value;
    expect(upstream).toBe("2");
    expect(downstream).toBe("1");
    expect(upstream).not.toBe(downstream);
  });

  it("resolves each edge's endpoints to real names, not bare ids", () => {
    const config = toLineageConfig(graph, "supplier.gstin");
    const row = config.rows.find((r) => r.key === "e1");
    expect(row?.cells.map((c) => c.text)).toContain("raw.gst_returns");
    expect(row?.cells.map((c) => c.text)).toContain("supplier.gstin");
  });

  it("is empty, not broken, for an entity with no lineage edges", () => {
    const config = toLineageConfig({ ...graph, edges: [] }, "supplier.gstin");
    expect(config.rows).toEqual([]);
    expect(config.emptyMessage.length).toBeGreaterThan(0);
  });

  it("builds the upstream chain furthest-first ending at the root, and the downstream chain starting at the root", () => {
    const deep: LineageGraph = {
      rootId: "root",
      nodes: [
        ...graph.nodes,
        { id: "up0", name: "gst.portal_api", kind: "service", fullyQualifiedName: "gst.portal_api", deleted: false },
        { id: "down2", name: "supplier.risk_score", kind: "table", fullyQualifiedName: "supplier.risk_score", deleted: false },
      ],
      edges: [
        ...graph.edges,
        {
          id: "e0",
          fromAssetId: "up0",
          toAssetId: "up1",
          relationship: "feeds",
          source: "connector",
          createdAt: "2026-08-01T00:00:00Z",
          createdBy: "connector:postgres",
        },
        {
          id: "e3",
          fromAssetId: "down1",
          toAssetId: "down2",
          relationship: "feeds",
          source: "manual",
          createdAt: "2026-08-10T00:00:00Z",
          createdBy: "asha",
        },
      ],
      truncated: false,
    };
    const config = toLineageConfig(deep, "supplier.gstin");
    expect(config.chain?.upstream.map((n) => n.id)).toEqual(["up0", "up1", "root"]);
    expect(config.chain?.downstream.map((n) => n.id)).toEqual(["root", "down1", "down2"]);
  });

  it("marks the root node as current at the boundary of both chains", () => {
    const config = toLineageConfig(graph, "supplier.gstin");
    expect(config.chain?.upstream.at(-1)).toMatchObject({ id: "root", current: true });
    expect(config.chain?.downstream[0]).toMatchObject({ id: "root", current: true });
    expect(config.chain?.upstream[0]?.current).toBe(false);
  });

  it("labels each chain link with the real asset kind, not a fabricated description", () => {
    const config = toLineageConfig(graph, "supplier.gstin");
    const up1Node = config.chain?.upstream.find((n) => n.id === "up1");
    expect(up1Node?.sub).toBe("table");
  });

  it("degrades to a single-node chain, not a crash, for an entity with no lineage edges", () => {
    const config = toLineageConfig({ ...graph, edges: [] }, "supplier.gstin");
    expect(config.chain?.upstream).toEqual([
      { id: "root", label: "supplier.gstin", sub: "column", current: true },
    ]);
    expect(config.chain?.downstream).toEqual([
      { id: "root", label: "supplier.gstin", sub: "column", current: true },
    ]);
  });

  /** Mutator: a walk that stops at the first inbound edge it finds (rather
   *  than exploring every branch and keeping the longest) would pick
   *  `shortBranch` here, since it is discovered first in edge order. */
  it("chooses the deepest branch when the upstream graph forks, not just the first one found", () => {
    const forked: LineageGraph = {
      rootId: "root",
      nodes: [
        { id: "root", name: "supplier.gstin", kind: "column", fullyQualifiedName: "supplier.gstin", deleted: false },
        { id: "shortBranch", name: "raw.other", kind: "table", fullyQualifiedName: "raw.other", deleted: false },
        {
          id: "longBranchA",
          name: "raw.gst_returns",
          kind: "table",
          fullyQualifiedName: "raw.gst_returns",
          deleted: false,
        },
        {
          id: "longBranchB",
          name: "gst.portal_api",
          kind: "service",
          fullyQualifiedName: "gst.portal_api",
          deleted: false,
        },
      ],
      edges: [
        {
          id: "eShort",
          fromAssetId: "shortBranch",
          toAssetId: "root",
          relationship: "feeds",
          source: "manual",
          createdAt: "2026-08-01T00:00:00Z",
          createdBy: "asha",
        },
        {
          id: "eLong1",
          fromAssetId: "longBranchA",
          toAssetId: "root",
          relationship: "feeds",
          source: "connector",
          createdAt: "2026-08-01T00:00:00Z",
          createdBy: "connector:postgres",
        },
        {
          id: "eLong2",
          fromAssetId: "longBranchB",
          toAssetId: "longBranchA",
          relationship: "feeds",
          source: "connector",
          createdAt: "2026-08-01T00:00:00Z",
          createdBy: "connector:postgres",
        },
      ],
      truncated: false,
    };
    const config = toLineageConfig(forked, "supplier.gstin");
    expect(config.chain?.upstream.map((n) => n.id)).toEqual(["longBranchB", "longBranchA", "root"]);
  });
});

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

describe("history — Plan 122a A4", () => {
  const versions: readonly AssetVersion[] = [
    {
      version: { major: 0, minor: 2 },
      snapshot: {} as AssetVersion["snapshot"],
      changeDescription: { summary: "renamed" },
      updatedBy: "asha",
      updatedAt: "2026-08-14T00:00:00Z",
    },
    {
      version: { major: 0, minor: 1 },
      snapshot: {} as AssetVersion["snapshot"],
      updatedBy: "system",
      updatedAt: "2026-08-01T00:00:00Z",
    },
  ];

  it("passes the shared config assertions", () => {
    assertsAsATraceConfig(toHistoryConfig(versions, "orders-service"));
  });

  it("counts every version, not just the latest", () => {
    const config = toHistoryConfig(versions, "orders-service");
    expect(config.kpis.find((k) => k.label === "VERSIONS")?.value).toBe("2");
    expect(config.rows).toHaveLength(2);
  });

  /** Mutator: a config that always reported the *first* version regardless
   *  of array order would still pass a test with only one version. */
  it("reports the latest version from the first row, not a hardcoded one", () => {
    const config = toHistoryConfig(versions, "orders-service");
    expect(config.kpis.find((k) => k.label === "LATEST")?.value).toBe("0.2");
  });

  it("falls back to a real placeholder rather than blank text when a version has no recorded summary", () => {
    const config = toHistoryConfig(versions, "orders-service");
    const undocumented = config.rows.find((r) => r.key.startsWith("0.1"));
    expect(undocumented?.cells.some((c) => c.text.length > 0)).toBe(true);
  });

  it("is empty, not broken, for an entity with no version history", () => {
    const config = toHistoryConfig([], "orders-service");
    expect(config.rows).toEqual([]);
    expect(config.kpis.find((k) => k.label === "LATEST")?.value).toBe("—");
  });
});

describe("evidence — Plan 122a A4", () => {
  const findings: readonly Finding[] = [
    {
      id: "f1",
      pack: "gst",
      label: "gst:MissingInGstr2b",
      subject: "gst:Supplier/1",
      summary: "ITC claimed without a matching GSTR-2B row",
      governedBy: "gst:s16",
      evidence: [
        { subject: "gst:Supplier/1", predicate: "gst:supplierGstin", value: "27AABCU9603R1ZM", var: "claimedGstin" },
        { subject: "gst:Invoice/9", predicate: "gst:igst", value: "37500" },
      ],
      status: "pending",
      detectedAt: "2026-08-01T00:00:00Z",
    },
    {
      id: "f2",
      pack: "hosp",
      label: "hosp:DuplicateGuest",
      subject: "hosp:Guest/1",
      summary: "two guest records share a phone number",
      governedBy: "hosp:policy-3",
      evidence: [{ subject: "hosp:Guest/1", predicate: "hosp:phone", value: "9999999999" }],
      status: "accepted",
      detectedAt: "2026-08-02T00:00:00Z",
    },
  ];

  it("passes the shared config assertions", () => {
    assertsAsATraceConfig(toEvidenceConfig(findings));
  });

  it("flattens every finding's evidence into its own row, not one row per finding", () => {
    const config = toEvidenceConfig(findings);
    // f1 has 2 evidence entries, f2 has 1 — 3 rows, not 2.
    expect(config.rows).toHaveLength(3);
  });

  it("counts distinct packs, not evidence rows, for the packs KPI", () => {
    const config = toEvidenceConfig(findings);
    expect(config.kpis.find((k) => k.label === "PACKS INVOLVED")?.value).toBe("2");
  });

  it("counts only pending findings for the pending KPI, not every finding", () => {
    const config = toEvidenceConfig(findings);
    expect(config.kpis.find((k) => k.label === "PENDING REVIEW")?.value).toBe("1");
  });

  it("is empty, not broken, when no findings exist", () => {
    const config = toEvidenceConfig([]);
    expect(config.rows).toEqual([]);
    expect(config.kpis.find((k) => k.label === "PACKS INVOLVED")?.value).toBe("0");
  });
});
