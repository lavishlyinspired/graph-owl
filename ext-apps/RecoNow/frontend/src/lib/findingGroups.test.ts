import { describe, expect, it } from "vitest";
import { CATEGORY_META, groupFindings } from "./findingGroups";
import type { RegisterRow } from "./api";

const row = (id: string, code: string, category: string | null, exposure: number): RegisterRow =>
  ({
    id,
    invoice_no: `INV-${id}`,
    reason_code: code,
    category,
    exposure,
    supplier_name: "S",
    supplier_gstin: "27A",
    books_amount: exposure,
    portal_amount: null,
  }) as unknown as RegisterRow;

describe("groupFindings", () => {
  it("splits findings by who has to act, not by rule name", () => {
    // The note's own taxonomy. A flat exception list makes a reviewer decide
    // for each row whether it is their problem, the supplier's, or the law's —
    // twelve times a period, from a rule label.
    const groups = groupFindings([
      row("1", "gst:ITCNotAvailable", "compliance", 58300),
      row("2", "gst:AmountMismatch", "data", 500),
      row("3", "gst:SupplierNotFiled", "follow-up", 8640),
    ]);

    expect(groups.map((g) => g.category)).toEqual(["compliance", "data", "follow-up"]);
  });

  it("orders the categories by what a reviewer can least afford to miss", () => {
    // Compliance first: that credit is lost or must be reversed on a filed
    // return. Follow-up last: it is recoverable and depends on somebody else.
    const order = CATEGORY_META.map((m) => m.category);

    expect(order.indexOf("compliance")).toBeLessThan(order.indexOf("data"));
    expect(order.indexOf("data")).toBeLessThan(order.indexOf("follow-up"));
  });

  it("totals the exposure per category", () => {
    const groups = groupFindings([
      row("1", "gst:ITCNotAvailable", "compliance", 58300),
      row("2", "gst:PaymentOverdue", "compliance", 45000),
    ]);

    expect(groups[0]?.exposure).toBe(103300);
  });

  it("sub-groups by rule inside a category", () => {
    // "9 compliance issues" is a number; "2 blocked credit, 1 unpaid 180 days"
    // is a list of things to do.
    const groups = groupFindings([
      row("1", "gst:ITCNotAvailable", "compliance", 58300),
      row("2", "gst:ITCNotAvailable", "compliance", 31500),
      row("3", "gst:PaymentOverdue", "compliance", 45000),
    ]);

    expect(groups[0]?.rules).toHaveLength(2);
    expect(groups[0]?.rules[0]?.rows).toHaveLength(2);
  });

  it("puts the costliest rule first within a category", () => {
    const groups = groupFindings([
      row("1", "gst:PaymentOverdue", "compliance", 100),
      row("2", "gst:ITCNotAvailable", "compliance", 9000),
    ]);

    expect(groups[0]?.rules[0]?.reason_code).toBe("gst:ITCNotAvailable");
  });

  it("omits a category with no findings rather than showing an empty one", () => {
    // An empty "Compliance issues" heading reads as a claim that the law was
    // checked and found nothing, which is a different statement from "no rule
    // in that category fired".
    const groups = groupFindings([row("1", "gst:AmountMismatch", "data", 500)]);

    expect(groups.map((g) => g.category)).toEqual(["data"]);
  });

  it("puts a rule with no category into its own bucket rather than dropping it", () => {
    // A rule added tomorrow, or a pack with no category authored, must still
    // appear. Silently omitting a finding is the one outcome this screen
    // cannot have.
    const groups = groupFindings([row("1", "gst:BrandNew", null, 700)]);

    expect(groups).toHaveLength(1);
    expect(groups[0]?.category).toBe("uncategorised");
  });

  it("returns nothing for no findings", () => {
    expect(groupFindings([])).toEqual([]);
  });
});
