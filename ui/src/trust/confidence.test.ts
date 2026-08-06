import { describe, expect, it } from "vitest";
import {
  bandOf,
  describeCertification,
  describeConfidence,
  describeDerivation,
  describeProvenance,
} from "./confidence";

describe("bandOf", () => {
  it("bands 0.8 and above as assert", () => {
    expect(bandOf(0.8)).toBe("assert");
    expect(bandOf(0.95)).toBe("assert");
    expect(bandOf(1)).toBe("assert");
  });

  it("bands the half-open range [0.5, 0.8) as surface", () => {
    expect(bandOf(0.5)).toBe("surface");
    expect(bandOf(0.79)).toBe("surface");
  });

  it("bands anything below 0.5 as ignore", () => {
    expect(bandOf(0.49)).toBe("ignore");
    expect(bandOf(0)).toBe("ignore");
  });
});

describe("describeConfidence", () => {
  it("gives every band a distinct symbol so the distinction survives without colour", () => {
    const symbols = new Set(
      [0.9, 0.6, 0.1].map((c) => describeConfidence(c).symbol),
    );
    expect(symbols.size).toBe(3);
  });

  it("gives every band a distinct, non-empty label", () => {
    const labels = [0.9, 0.6, 0.1].map((c) => describeConfidence(c).label);
    expect(new Set(labels).size).toBe(3);
    for (const label of labels) expect(label.length).toBeGreaterThan(0);
  });

  it("carries the band the caller would otherwise recompute", () => {
    expect(describeConfidence(0.9).band).toBe("assert");
    expect(describeConfidence(0.6).band).toBe("surface");
    expect(describeConfidence(0.1).band).toBe("ignore");
  });

  it("gives each band its exact symbol, not merely a non-empty one", () => {
    expect(describeConfidence(0.9).symbol).toBe("●");
    expect(describeConfidence(0.6).symbol).toBe("◐");
    expect(describeConfidence(0.1).symbol).toBe("○");
  });
});

describe("describeDerivation", () => {
  it("labels an asserted fact exactly, not just distinctly from derived", () => {
    expect(describeDerivation("asserted")).toEqual({
      status: "asserted",
      label: "Asserted",
      symbol: "✓",
    });
  });

  it("labels a derived fact exactly, not just distinctly from asserted", () => {
    expect(describeDerivation("derived")).toEqual({
      status: "derived",
      label: "Derived",
      symbol: "∴",
    });
  });
});

describe("describeCertification", () => {
  it("says plainly when nothing is known, rather than a confident-looking blank", () => {
    expect(describeCertification({})).toEqual({
      state: "uncertified",
      label: "uncertified",
      symbol: "—",
    });
  });

  it("reports a certified asset exactly", () => {
    expect(describeCertification({ certified: true })).toEqual({
      state: "certified",
      label: "certified",
      symbol: "✓",
    });
  });

  it("reports a deprecated asset exactly", () => {
    expect(describeCertification({ deprecated: true })).toEqual({
      state: "deprecated",
      label: "deprecated",
      symbol: "⚠",
    });
  });

  it("deprecation takes priority over certification when both are somehow set", () => {
    const d = describeCertification({ certified: true, deprecated: true });
    expect(d.state).toBe("deprecated");
  });
});

describe("describeProvenance", () => {
  it("is honest about missing source, time, and ingestor rather than blank", () => {
    const p = describeProvenance({});
    expect(p.sourceLabel).toMatch(/not captured/);
    expect(p.transactionLabel).toMatch(/not captured/);
    expect(p.ingestedByLabel).toMatch(/not captured/);
  });

  it("renders what it is given instead of the not-captured fallback", () => {
    const p = describeProvenance({ source: "connector:postgres", t: 42, ingestedBy: "alice" });
    expect(p.sourceLabel).toBe("connector:postgres");
    expect(p.transactionLabel).toBe("as of t42");
    expect(p.ingestedByLabel).toBe("alice");
  });
});
