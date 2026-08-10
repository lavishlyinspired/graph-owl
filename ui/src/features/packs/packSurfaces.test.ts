/** The gating: a pack surface exists only while its pack does. */

import { describe, expect, it } from "vitest";
import { packIdOf, surfacesFor } from "./packSurfaces";

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
