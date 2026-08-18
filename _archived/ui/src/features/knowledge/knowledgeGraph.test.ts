import { describe, expect, it } from "vitest";
import { assetIri, describeLoss, inboundTriplesQuery, outboundTriplesQuery } from "./knowledgeGraph";
import type { LossyMapping } from "./knowledgeGraph";

describe("assetIri", () => {
  it("builds the DSC namespace IRI an asset's own subject uses in the graph", () => {
    expect(assetIri("3a75ce83-8f42-4902-b096-2fcd7637f298")).toBe(
      "https://graph-owl.dev/ns/catalog#3a75ce83-8f42-4902-b096-2fcd7637f298",
    );
  });
});

describe("outboundTriplesQuery / inboundTriplesQuery", () => {
  it("scopes the outbound query to this asset as the subject", () => {
    const query = outboundTriplesQuery("abc-123");
    expect(query).toContain("<https://graph-owl.dev/ns/catalog#abc-123>");
    expect(query).toMatch(/SELECT\s+\?p\s+\?o/);
  });

  it("scopes the inbound query to this asset as the object", () => {
    const query = inboundTriplesQuery("abc-123");
    expect(query).toContain("<https://graph-owl.dev/ns/catalog#abc-123>");
    expect(query).toMatch(/SELECT\s+\?s\s+\?p/);
  });
});

describe("describeLoss", () => {
  // The RED test this feature exists for: a view toggle that silently
  // drops what does not map teaches a reader the two models are
  // equivalent when they are not. Every `LossyMapping` variant must
  // produce a non-empty, specific description — never a blank string,
  // never a generic fallback that erases which kind of loss it was.
  it("names a reference flattened into a plain-text property", () => {
    const loss: LossyMapping = { kind: "refInProperty", subject: "s1", predicate: "parentService" };
    expect(describeLoss(loss)).toBe(
      'The reference in "parentService" was flattened to plain text — it no longer traverses as an edge.',
    );
  });

  it("names named graphs collapsed into one, listing how many", () => {
    const loss: LossyMapping = {
      kind: "namedGraphCollapse",
      subject: "s1",
      graphs: ["graph:a", "graph:b", "graph:c"],
    };
    expect(describeLoss(loss)).toBe("3 named graphs were merged into one — their separation is gone.");
  });

  it("singularizes named graph collapse when only one graph merged", () => {
    const loss: LossyMapping = { kind: "namedGraphCollapse", subject: "s1", graphs: ["graph:a"] };
    expect(describeLoss(loss)).toBe("1 named graph was merged into one — its separation is gone.");
  });

  it("names a type narrowed to a string, keeping the original type", () => {
    const loss: LossyMapping = {
      kind: "typeNarrowed",
      subject: "s1",
      predicate: "id",
      from: "uuid",
    };
    expect(describeLoss(loss)).toBe('"id" narrowed from uuid to plain text.');
  });

  it("distinguishes typeNarrowed's from value in the description", () => {
    const loss: LossyMapping = {
      kind: "typeNarrowed",
      subject: "s1",
      predicate: "properties",
      from: "json",
    };
    expect(describeLoss(loss)).toBe('"properties" narrowed from json to plain text.');
  });
});
