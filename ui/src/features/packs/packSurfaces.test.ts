/** The gating: a pack surface exists only while its pack does. */

import { describe, expect, it } from "vitest";
import { installedPacks, packIdOf, surfacesFor } from "./packSurfaces";
import { invoicePeriod } from "./PackAdminPanel";

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

  /** **Looked up by key, never by index.** The three surfaces are ordered the
   *  way the reconciliation is actually done — books, then what the suppliers
   *  declared, then what the authority made available — and that order is
   *  content a product decision can change. A positional index turns any
   *  reordering into a test failure that says nothing about behaviour, and it
   *  did exactly that when the two new surfaces landed. */
  function surface(key: string) {
    const gst = surfacesFor(["pack:gst"])[0]!;
    const found = gst.imports.find((i) => i.key === key);
    if (!found) throw new Error(`no '${key}' import surface; found ${gst.imports.map((i) => i.key).join(", ")}`);
    return found;
  }

  it("renders the GST surface only once the GST pack is installed", () => {
    expect(surfacesFor(["pack:hospitality"])).toEqual([]);

    const withGst = surfacesFor(["pack:hospitality", "pack:gst"]);
    expect(withGst.map((s) => s.packId)).toEqual(["gst"]);
    // All three sources the three-way reconciliation needs, in workflow order.
    expect(withGst[0]!.imports.map((i) => i.key)).toEqual(["books", "gstr1", "gstr2b"]);
  });

  it("converts an uploaded GSTR-2B through the surface's own converter", () => {
    const file = JSON.stringify({
      docdata: {
        b2b: [{ ctin: "27AABCU9603R1ZM", supprd: "072026", inv: [{ inum: "INV-1", dt: "09-07-2026", txval: 100, igst: 18, cgst: 0, sgst: 0, cess: 0, itcavl: "Y", rev: "N" }] }],
      },
    });

    const { turtle, count } = surface("gstr2b").convert(file);

    expect(count).toBe(1);
    expect(turtle).toContain("gst:2b-INV-1 rdf:type gst:Gstr2bInvoice");
    expect(turtle).toContain('gst:invoiceDate   "2026-07-09"');
  });

  it("converts an uploaded purchase register", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value,IGST\n27AABCU9603R1ZM,INV-1,09-07-2026,100,18";

    const { turtle, count } = surface("books").convert(csv);

    expect(count).toBe(1);
    expect(turtle).toContain("gst:pr-INV-1 rdf:type gst:PurchaseInvoice");
  });

  it("converts an uploaded GSTR-2A", () => {
    const file = JSON.stringify({
      fp: "072026",
      b2b: [
        {
          ctin: "27AABCU9603R1ZM",
          fldtr1: "11-08-2026",
          inv: [{ inum: "INV-1", idt: "09-07-2026", itms: [{ itm_det: { txval: 100, iamt: 18 } }] }],
        },
      ],
    });

    const { turtle, count } = surface("gstr1").convert(file);

    expect(count).toBe(1);
    expect(turtle).toContain("gst:g1-INV-1 rdf:type gst:Gstr1Invoice");
    expect(turtle).toContain('gst:filedDate     "2026-08-11"');
  });

  it("surfaces a bad file as an error the uploader can act on", () => {
    expect(() => surface("gstr2b").convert('{"error":"unauthorized"}')).toThrow(/docdata/);
    expect(() => surface("gstr2b").convert("not json at all")).toThrow();
    expect(() => surface("gstr1").convert('{"error":"unauthorized"}')).toThrow(/not a GSTR-1/);
    // The books surface's own version of the same guarantee: it names the
    // column it wanted rather than reporting an empty register.
    expect(() => surface("books").convert("a,b\n1,2")).toThrow(/GSTIN/);
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
