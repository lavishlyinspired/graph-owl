import { describe, expect, it } from "vitest";
import { mergeEntityOptions, seedsFromFindings, findingsFor, reasoningSteps } from "./exploreSeeds";
import type { EntitySummary } from "./graph/entityList";
import type { Finding } from "./api";

const finding = (id: string, subject: string, label: string): Finding =>
  ({
    id,
    pack: "gst",
    label,
    subject,
    summary: `${label} on ${subject}`,
    governedBy: "gst:Section16",
    evidence: [],
    status: "pending",
  }) as unknown as Finding;

describe("seedsFromFindings", () => {
  it("offers a starting point per distinct subject", () => {
    // Plan 123 §9: "Explore needs a search term to show anything. Blank screen
    // with 'Search or open an entity'. With search broken, unreachable."
    // Search is fixed; this removes the dead end that remained.
    const seeds = seedsFromFindings([
      finding("1", "gst:invoice-a", "gst:AmountMismatch"),
      finding("2", "gst:invoice-b", "gst:SupplierNotFiled"),
    ]);

    expect(seeds.map((s) => s.id)).toEqual(["gst:invoice-a", "gst:invoice-b"]);
  });

  it("collapses several findings on one subject into one starting point", () => {
    // An invoice with three problems is one thing to explore, not three. A
    // list repeating it would bury the other subjects.
    const seeds = seedsFromFindings([
      finding("1", "gst:invoice-a", "gst:AmountMismatch"),
      finding("2", "gst:invoice-a", "gst:TaxHeadMismatch"),
      finding("3", "gst:invoice-b", "gst:SupplierNotFiled"),
    ]);

    expect(seeds).toHaveLength(2);
  });

  it("says how many findings a subject carries, so the list ranks itself", () => {
    const seeds = seedsFromFindings([
      finding("1", "gst:invoice-a", "gst:AmountMismatch"),
      finding("2", "gst:invoice-a", "gst:TaxHeadMismatch"),
      finding("3", "gst:invoice-b", "gst:SupplierNotFiled"),
    ]);

    expect(seeds[0]).toMatchObject({ id: "gst:invoice-a", findings: 2 });
  });

  it("puts the most-flagged subject first, because that is where to look", () => {
    const seeds = seedsFromFindings([
      finding("1", "gst:quiet", "gst:AmountMismatch"),
      finding("2", "gst:busy", "gst:AmountMismatch"),
      finding("3", "gst:busy", "gst:TaxHeadMismatch"),
    ]);

    expect(seeds[0]?.id).toBe("gst:busy");
  });

  it("breaks ties by subject so the list does not reshuffle between loads", () => {
    // A list that reorders itself on every refresh cannot be scanned.
    const seeds = seedsFromFindings([
      finding("1", "gst:b", "gst:AmountMismatch"),
      finding("2", "gst:a", "gst:AmountMismatch"),
    ]);

    expect(seeds.map((s) => s.id)).toEqual(["gst:a", "gst:b"]);
  });

  it("names one of the labels, so a row says what is wrong with it", () => {
    const seeds = seedsFromFindings([finding("1", "gst:invoice-a", "gst:AmountMismatch")]);

    expect(seeds[0]?.label).toBe("gst:AmountMismatch");
  });

  it("caps the list, because a seeding aid is not a findings screen", () => {
    const many = Array.from({ length: 40 }, (_, i) =>
      finding(String(i), `gst:invoice-${i}`, "gst:AmountMismatch"),
    );

    expect(seedsFromFindings(many).length).toBeLessThanOrEqual(12);
  });

  it("returns nothing when there are no findings, rather than inventing a seed", () => {
    expect(seedsFromFindings([])).toEqual([]);
  });
});

describe("the findings that explain a subject", () => {
  const finding = (subject: string, over?: Partial<Finding>): Finding => ({
    id: `f-${subject}`,
    pack: "gst",
    label: "gst:ImsNotActioned",
    subject,
    summary: "An IMS record left unactioned will be deemed accepted at the cut-off",
    governedBy: "gst:ImsPolicy",
    evidence: [
      { subject, predicate: "gst:taxAmount", value: "32400.0", var: "taxAmount" },
      { subject, predicate: "gst:imsStatus", value: "No Action", var: "imsStatus" },
    ],
    status: "pending",
    detectedAt: "2026-08-18T23:47:45Z",
    ...over,
  });

  it("finds every finding recorded against the subject", () => {
    const all = [finding("a"), finding("b"), finding("a", { id: "second-a" })];
    expect(findingsFor(all, "a").map((f) => f.id)).toEqual(["f-a", "second-a"]);
  });

  it("finds nothing for a subject nothing was flagged against", () => {
    expect(findingsFor([finding("a")], "b")).toEqual([]);
  });

  it("finds nothing when there is no subject selected", () => {
    expect(findingsFor([finding("a")], undefined)).toEqual([]);
  });

  /** The mock's numbered "why GraphOWL believes this" chain, built from the
   *  parts a finding actually carries: what the subject is, each binding the
   *  rule matched on, and the rule itself as the closing step. */
  it("reads the rule's own bindings back as numbered steps", () => {
    const steps = reasoningSteps(finding("a"), "gst:PurchaseInvoice");
    expect(steps[0]?.text).toContain("gst:PurchaseInvoice");
    expect(steps.map((s) => s.text)).toContain("gst:taxAmount = 32400.0");
    expect(steps[steps.length - 1]?.source).toBe("gst:ImsPolicy");
  });

  it("still explains a subject whose type the graph could not resolve", () => {
    expect(reasoningSteps(finding("a"), undefined).length).toBeGreaterThan(0);
  });

  it("closes on the rule, so the chain ends with what concluded it", () => {
    const steps = reasoningSteps(finding("a"), "gst:PurchaseInvoice");
    expect(steps[steps.length - 1]?.text).toContain(
      "An IMS record left unactioned will be deemed accepted",
    );
  });
});

describe("mergeEntityOptions", () => {
  const entity = (iri: string, type: string): EntitySummary => ({ id: iri.split("#")[1]!, iri, type });

  it("includes every real entity, not only the ones a rule flagged", () => {
    const merged = mergeEntityOptions(
      [entity("gst#invoice-a", "PurchaseInvoice"), entity("gst#invoice-b", "PurchaseInvoice")],
      [],
    );
    expect(merged.map((m) => m.id).sort()).toEqual(["gst#invoice-a", "gst#invoice-b"]);
  });

  it("carries a flagged entity's real finding count through the merge", () => {
    const merged = mergeEntityOptions(
      [entity("gst#invoice-a", "PurchaseInvoice")],
      [{ id: "gst#invoice-a", label: "gst:AmountMismatch", findings: 2 }],
    );
    expect(merged.find((m) => m.id === "gst#invoice-a")).toMatchObject({ findings: 2 });
  });

  it("labels an unflagged entity by its own type, since no finding named it", () => {
    const merged = mergeEntityOptions([entity("gst#supplier-1", "Supplier")], []);
    expect(merged[0]).toMatchObject({ label: "Supplier", findings: 0 });
  });

  it("puts the most-flagged entities first, same ranking seedsFromFindings already uses", () => {
    const merged = mergeEntityOptions(
      [entity("gst#quiet", "PurchaseInvoice"), entity("gst#busy", "PurchaseInvoice")],
      [{ id: "gst#busy", label: "gst:AmountMismatch", findings: 3 }],
    );
    expect(merged[0]?.id).toBe("gst#busy");
  });

  it("keeps a flagged subject even when the all-entities scan somehow missed it", () => {
    const merged = mergeEntityOptions([], [{ id: "gst#orphaned-finding", label: "gst:Reversed", findings: 1 }]);
    expect(merged.map((m) => m.id)).toEqual(["gst#orphaned-finding"]);
  });
});
