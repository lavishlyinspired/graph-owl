import { describe, expect, it } from "vitest";
import { seedsFromFindings } from "./exploreSeeds";
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
