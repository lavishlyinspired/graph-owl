/** The gating: a pack surface exists only while its pack does. */

import { describe, expect, it } from "vitest";
import { installedPacks, packIdOf, surfacesFor } from "./packSurfaces";
import { invoicePeriod } from "./PackImportPanel";

describe("packIdOf", () => {
  it("recognises a pack-declared namespace", () => {
    expect(packIdOf("pack:gst")).toBe("gst");
  });

  it("does not treat a connector as a pack", () => {
    // `connector:erpnext` declares a namespace too, and must contribute no
    // console surface — it brings data, not a domain's UI.
    expect(packIdOf("connector:erpnext")).toBeNull();
    expect(packIdOf("system")).toBeNull();
  });
});

describe("surfacesFor", () => {
  it("renders nothing on a deployment with no packs", () => {
    // The empty case is the default, not an error state: most deployments
    // never install a pack and must not see a broken-looking section.
    expect(surfacesFor([])).toEqual([]);
    expect(surfacesFor(["system", "connector:erpnext"])).toEqual([]);
  });

  it("renders the GST surface only once the GST pack is installed", () => {
    expect(surfacesFor(["pack:hospitality"])).toEqual([]);

    const withGst = surfacesFor(["pack:hospitality", "pack:gst"]);
    expect(withGst.map((s) => s.packId)).toEqual(["gst"]);
    expect(withGst[0]!.imports.map((i) => i.key)).toEqual(["gstr2b"]);
  });

  it("converts an uploaded file through the surface's own converter", () => {
    const gst = surfacesFor(["pack:gst"])[0]!;
    const file = JSON.stringify({
      docdata: {
        b2b: [{ ctin: "27AABCU9603R1ZM", inv: [{ inum: "INV-1", dt: "09-07-2026", txval: 100, igst: 18, cgst: 0, sgst: 0, cess: 0, itcavl: "Y", rev: "N" }] }],
      },
    });

    const { turtle, count } = gst.imports[0]!.convert(file);

    expect(count).toBe(1);
    expect(turtle).toContain("gst:2b-INV-1 rdf:type gst:Gstr2bInvoice");
    expect(turtle).toContain('gst:invoiceDate   "2026-07-09"');
  });

  it("surfaces a bad file as an error the uploader can act on", () => {
    const gst = surfacesFor(["pack:gst"])[0]!;

    expect(() => gst.imports[0]!.convert('{"error":"unauthorized"}')).toThrow(/docdata/);
    expect(() => gst.imports[0]!.convert("not json at all")).toThrow();
  });
});

describe("installedPacks", () => {
  it("lists nothing on a deployment with no packs", () => {
    expect(installedPacks([])).toEqual([]);
    expect(installedPacks([{ code: 0, iri: "https://x/#", declaredBy: "system" }])).toEqual([]);
  });

  it("lists a pack that has no import surface, unlike surfacesFor", () => {
    // The whole reason this is a separate function from `surfacesFor`:
    // `surfacesFor` filters to packs with a registered upload surface, which
    // would silently hide an installed pack that has none (hospitality, at
    // the time of writing) — invisible to an admin trying to confirm it
    // loaded at all.
    const rows = [{ code: 1024, iri: "https://graph-owl.dev/packs/hospitality#", declaredBy: "pack:hospitality" }];

    expect(surfacesFor(["pack:hospitality"])).toEqual([]);
    expect(installedPacks(rows).map((p) => p.packId)).toEqual(["hospitality"]);
  });

  it("uses the registry's own label when a pack is registered there", () => {
    const rows = [{ code: 1024, iri: "https://graph-owl.dev/packs/gst#", declaredBy: "pack:gst" }];
    expect(installedPacks(rows)[0]).toEqual({
      packId: "gst",
      label: "GST",
      namespaceCode: 1024,
      iri: "https://graph-owl.dev/packs/gst#",
    });
  });

  it("falls back to a title-cased id for a pack the registry does not name", () => {
    const rows = [{ code: 1025, iri: "https://graph-owl.dev/packs/hospitality#", declaredBy: "pack:hospitality" }];
    expect(installedPacks(rows)[0]!.label).toBe("Hospitality");
  });

  it("does not list the same pack twice when it declares more than one namespace", () => {
    const rows = [
      { code: 1024, iri: "https://graph-owl.dev/packs/gst#", declaredBy: "pack:gst" },
      { code: 1025, iri: "https://graph-owl.dev/packs/gst/law#", declaredBy: "pack:gst" },
    ];
    expect(installedPacks(rows).map((p) => p.packId)).toEqual(["gst"]);
  });

  it("ignores a connector's own namespace, the same boundary packIdOf already draws", () => {
    const rows = [{ code: 2000, iri: "https://x/erpnext#", declaredBy: "connector:erpnext" }];
    expect(installedPacks(rows)).toEqual([]);
  });
});

describe("invoicePeriod", () => {
  it("reads the period out of the generated Turtle", () => {
    expect(invoicePeriod('gst:period        "2026-07" .')).toBe("2026-07");
  });

  it("scopes the import source so a real upload cannot collide with the pack's own bundled demo fixture", () => {
    // A sample and the pack's shipped `fixtures/gstr2b.ttl` can legitimately
    // share invoice numbers (both this project's own test data), and the
    // server's import is idempotent per subject *within one source name* — so
    // without period-scoping, a real upload whose invoice numbers happened to
    // match the demo fixture would silently skip as "already imported".
    // Found exactly this way: a real upload landed 4 of 7 invoices with no
    // visible explanation for the other 3.
    expect(invoicePeriod('gst:period        "2020-07" .')).toBe("2020-07");
  });

  it("returns null for Turtle with no period, falling back to the pack-wide source", () => {
    expect(invoicePeriod("gst:2b-X rdf:type gst:Gstr2bInvoice .")).toBeNull();
  });
});
