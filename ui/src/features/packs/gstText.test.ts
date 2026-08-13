/** The normalizers every GST import surface shares.
 *
 *  `money`, `isoDate` and `returnPeriod` are covered through `gstr2b.test.ts`,
 *  which is pinned assertion-for-assertion against the Python twin. What is
 *  tested here is the part that has no Python twin yet and that a live upload
 *  proved was wrong. */

import { describe, expect, it } from "vitest";
import { subjectSuffix } from "./gstText";

describe("turning an invoice number into a subject", () => {
  /** **The bug a real upload found, and it is not an edge case.** Indian
   *  invoice numbers routinely look like `RST/2026/0455` — a slash is the
   *  normal separator, not an exotic one. Written straight into a prefixed
   *  name it produces `gst:pr-RST/2026/0455`, which is not valid Turtle, and
   *  the server rejected the whole import with "1 field failed validation".
   *  Every importer here had it, including the GSTR-2B one that shipped
   *  months ago and would have failed the same way on any real return. */
  it("percent-encodes a slash so the subject is a legal prefixed name", () => {
    expect(subjectSuffix("RST/2026/0455")).toBe("RST%2F2026%2F0455");
  });

  it("leaves an invoice number that is already safe completely alone", () => {
    // The pinned GSTR-2B fixtures use these, and they must not change.
    expect(subjectSuffix("INV-1001")).toBe("INV-1001");
    expect(subjectSuffix("PL-8834")).toBe("PL-8834");
    expect(subjectSuffix("SE_JUL_119")).toBe("SE_JUL_119");
  });

  /** **Encoding rather than replacing, because a collision here merges two
   *  invoices.** Mapping every unsafe character to `-` would make `INV/1` and
   *  `INV-1` the same subject — two different invoices silently becoming one
   *  in a tax reconciliation, which is far worse than a rejected import. */
  it("keeps two invoices distinct when a lossy substitution would merge them", () => {
    expect(subjectSuffix("INV/1")).not.toBe(subjectSuffix("INV-1"));
  });

  it("encodes the other separators a real invoice number carries", () => {
    expect(subjectSuffix("INV 1")).toBe("INV%201");
    expect(subjectSuffix("INV#1")).toBe("INV%231");
    expect(subjectSuffix("INV(1)")).toBe("INV%281%29");
  });

  /** A GSTIN is always alphanumeric, so a supplier subject is unaffected —
   *  worth pinning so the encoding is never "improved" into changing it. */
  it("leaves a GSTIN untouched", () => {
    expect(subjectSuffix("27AABCU9603R1ZM")).toBe("27AABCU9603R1ZM");
  });
});
