import { describe, expect, it } from "vitest";
import { packCards } from "./packCards";
import type { InstalledPack } from "../packs/packSurfaces";
import type { LoadedSource } from "../packs/packData";

function pack(packId: string, label: string): InstalledPack {
  return { packId, label, namespaceCode: 1024, iri: `https://graph-owl.dev/packs/${packId}#` };
}

function source(name: string, packId: string): LoadedSource {
  return {
    name,
    packId,
    iri: `https://graph-owl.dev/ns/catalog#graph:import:${name}`,
    triples: 12,
  };
}

describe("packCards", () => {
  it("marks a pack as having an ontology when its {packId}-ontology source is loaded", () => {
    const cards = packCards([pack("gst", "GST")], [source("gst-ontology", "gst")]);
    expect(cards).toEqual([{ packId: "gst", label: "GST", hasOntology: true }]);
  });

  it("marks a pack as not having an ontology when no ontology source is loaded", () => {
    const cards = packCards([pack("hospitality", "Hospitality")], []);
    expect(cards).toEqual([{ packId: "hospitality", label: "Hospitality", hasOntology: false }]);
  });

  it("does not credit one pack's ontology to another pack", () => {
    const cards = packCards([pack("gst", "GST")], [source("hospitality-ontology", "hospitality")]);
    expect(cards[0]!.hasOntology).toBe(false);
  });

  it("does not credit a non-ontology data import as an ontology", () => {
    const cards = packCards([pack("gst", "GST")], [source("gst-gstr2b-2025-07", "gst")]);
    expect(cards[0]!.hasOntology).toBe(false);
  });

  it("renders one card per installed pack, in the order given", () => {
    const cards = packCards([pack("gst", "GST"), pack("hospitality", "Hospitality")], []);
    expect(cards.map((c) => c.packId)).toEqual(["gst", "hospitality"]);
  });
});
