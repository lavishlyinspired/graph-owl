import { describe, expect, it } from "vitest";
import { formatRupees } from "./format";

describe("formatRupees", () => {
  it("groups in lakhs and crores, not thousands", () => {
    // The whole reason this function exists: `toLocaleString()` with no
    // locale renders 1,234,567 — western grouping, wrong for a GST product.
    expect(formatRupees(1234567)).toBe("₹12,34,567");
  });

  it("groups a lakh at the Indian boundary", () => {
    expect(formatRupees(100000)).toBe("₹1,00,000");
  });

  it("leaves amounts below a thousand ungrouped", () => {
    expect(formatRupees(999)).toBe("₹999");
  });

  it("distinguishes an absent amount from zero", () => {
    // Zero rupees at risk and an unknown amount are opposite claims. A dash
    // is not a number, and rendering "₹0" for unknown asserts something the
    // data never said.
    expect(formatRupees(null)).toBe("—");
    expect(formatRupees(undefined)).toBe("—");
    expect(formatRupees(0)).toBe("₹0");
  });

  it("rounds to whole rupees rather than truncating", () => {
    expect(formatRupees(1234.6)).toBe("₹1,235");
    expect(formatRupees(1234.4)).toBe("₹1,234");
  });

  it("keeps a negative amount negative", () => {
    // A credit note reverses a value; showing it unsigned would read as a
    // charge.
    expect(formatRupees(-50000)).toBe("₹-50,000");
  });
});
