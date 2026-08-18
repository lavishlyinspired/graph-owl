import { describe, expect, it } from "vitest";
import { visibleRows } from "./rows";
import type { Bucket, ReconRow } from "./api";

const row = (invoice: string, bucket: Bucket, labels: readonly string[]): ReconRow =>
  ({
    invoice_no: invoice,
    supplier_name: "Supplier",
    supplier_gstin: "27AABCS1429B1Z8",
    books_total: 1000,
    portal_total: 1000,
    difference: 0,
    bucket,
    labels,
  }) as unknown as ReconRow;

const ROWS: readonly ReconRow[] = [
  row("INV-1", "matched", []),
  row("INV-2", "review", ["gst:AmountMismatch", "gst:TaxHeadMismatch"]),
  row("INV-3", "review", ["gst:TaxHeadMismatch"]),
  row("INV-4", "only_books", ["gst:SupplierNotFiled"]),
];

describe("visibleRows", () => {
  it("shows every invoice when nothing is chosen", () => {
    expect(visibleRows(ROWS, null, null)).toHaveLength(4);
  });

  it("narrows to the invoices carrying a rule's label, so a finding count leads somewhere", () => {
    const shown = visibleRows(ROWS, null, "gst:AmountMismatch");

    expect(shown.map((r) => r.invoice_no)).toEqual(["INV-2"]);
  });

  it("keeps an invoice that carries the label alongside others", () => {
    const shown = visibleRows(ROWS, null, "gst:TaxHeadMismatch");

    expect(shown.map((r) => r.invoice_no)).toEqual(["INV-2", "INV-3"]);
  });

  it("does not match a label no invoice carries", () => {
    expect(visibleRows(ROWS, null, "gst:PaymentOverdue")).toEqual([]);
  });

  it("narrows to a bucket on its own", () => {
    expect(visibleRows(ROWS, "review", null).map((r) => r.invoice_no)).toEqual([
      "INV-2",
      "INV-3",
    ]);
  });

  it("applies bucket and rule together, keeping only invoices satisfying both", () => {
    expect(visibleRows(ROWS, "review", "gst:SupplierNotFiled")).toEqual([]);
    expect(visibleRows(ROWS, "only_books", "gst:SupplierNotFiled").map((r) => r.invoice_no)).toEqual(
      ["INV-4"],
    );
  });
});
