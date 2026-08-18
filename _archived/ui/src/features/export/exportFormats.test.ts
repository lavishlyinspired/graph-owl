import { describe, expect, it } from "vitest";
import {
  EXPORT_FORMATS,
  exportPath,
  exportQueryString,
  previewPath,
  type ExportFormat,
} from "./exportFormats";

const noFilters = { scope: null, asOf: null };

function format(key: string): ExportFormat {
  const found = EXPORT_FORMATS.find((f) => f.key === key);
  if (!found) throw new Error(`no such format: ${key}`);
  return found;
}

describe("EXPORT_FORMATS", () => {
  it("names nine formats, one per RDF wire format plus five LPG shapes", () => {
    expect(EXPORT_FORMATS).toHaveLength(9);
  });

  it("every key is unique — a duplicate would silently shadow one option in a keyed list", () => {
    const keys = EXPORT_FORMATS.map((f) => f.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("every entry has a real key, a real label, and a real /graph/export/ path — none silently blank", () => {
    for (const entry of EXPORT_FORMATS) {
      expect(entry.key.length, `key for ${entry.label}`).toBeGreaterThan(0);
      expect(entry.label.length, `label for ${entry.key}`).toBeGreaterThan(0);
      expect(entry.path, `path for ${entry.key}`).toMatch(/^\/graph\/export\//);
    }
  });

  it("every RDF entry's own rdfFormat is a real, non-empty value", () => {
    for (const entry of EXPORT_FORMATS.filter((f) => f.rdfFormat !== undefined)) {
      expect(entry.rdfFormat?.length, `rdfFormat for ${entry.key}`).toBeGreaterThan(0);
    }
  });

  it("only the RDF entries carry an rdfFormat", () => {
    const withRdfFormat = EXPORT_FORMATS.filter((f) => f.rdfFormat !== undefined);
    expect(withRdfFormat).toHaveLength(4);
    expect(withRdfFormat.every((f) => f.path === "/graph/export/rdf")).toBe(true);
  });
});

describe("exportQueryString", () => {
  it("is empty when no format param and no filters are set", () => {
    expect(exportQueryString(format("graphml"), noFilters)).toBe("");
  });

  it("names the RDF sub-format for an RDF entry even with no other filters", () => {
    expect(exportQueryString(format("rdf-turtle"), noFilters)).toBe("?format=turtle");
  });

  it("includes scope when set", () => {
    expect(exportQueryString(format("graphml"), { scope: "public.", asOf: null })).toBe(
      "?scope=public.",
    );
  });

  it("includes asOf when set", () => {
    expect(
      exportQueryString(format("graphml"), { scope: null, asOf: "2026-01-01T00:00:00.000Z" }),
    ).toBe("?asOf=2026-01-01T00%3A00%3A00.000Z");
  });

  it("combines format, scope and asOf in one query string", () => {
    const qs = exportQueryString(format("rdf-jsonld"), {
      scope: "public.",
      asOf: "2026-01-01T00:00:00.000Z",
    });
    const params = new URLSearchParams(qs);
    expect(params.get("format")).toBe("jsonld");
    expect(params.get("scope")).toBe("public.");
    expect(params.get("asOf")).toBe("2026-01-01T00:00:00.000Z");
  });

  it("does not name a format param for a non-RDF entry even when RDF's own sub-format string would otherwise collide", () => {
    expect(exportQueryString(format("bulk-csv"), noFilters)).not.toContain("format=");
  });
});

describe("exportPath", () => {
  it("is the format's own route with no query string when nothing is set", () => {
    expect(exportPath(format("cypher"), noFilters)).toBe("/graph/export/cypher");
  });

  it("appends the query string built from format and filters", () => {
    expect(exportPath(format("rdf-ntriples"), { scope: "a.", asOf: null })).toBe(
      "/graph/export/rdf?format=ntriples&scope=a.",
    );
  });

  it("routes every RDF entry through the same /graph/export/rdf path, distinguished only by the query string", () => {
    const rdfPaths = EXPORT_FORMATS.filter((f) => f.rdfFormat).map((f) =>
      exportPath(f, noFilters),
    );
    expect(new Set(rdfPaths).size).toBe(4);
    expect(rdfPaths.every((p) => p.startsWith("/graph/export/rdf?format="))).toBe(true);
  });
});

describe("previewPath", () => {
  it("is the bare preview route with no filters", () => {
    expect(previewPath(noFilters)).toBe("/graph/export/preview");
  });

  it("never carries a format param — the count is identical across every format", () => {
    expect(previewPath({ scope: "public.", asOf: "2026-01-01T00:00:00.000Z" })).not.toContain(
      "format=",
    );
  });

  it("carries scope and asOf identically to exportPath, so a preview and its matching export agree on what they counted", () => {
    const filters = { scope: "public.", asOf: "2026-01-01T00:00:00.000Z" };
    const previewParams = new URLSearchParams(previewPath(filters).split("?")[1]);
    const exportParams = new URLSearchParams(exportPath(format("graphml"), filters).split("?")[1]);
    expect(previewParams.get("scope")).toBe(exportParams.get("scope"));
    expect(previewParams.get("asOf")).toBe(exportParams.get("asOf"));
  });
});
