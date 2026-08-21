import { describe, expect, it } from "vitest";
import { findingsQueryString, packsIn, sortForReview, subjectDisplayLabel } from "./findingsQueue";
import type { Finding } from "./api";

const finding = (overrides?: Partial<Finding>): Finding => ({
  id: "f1",
  pack: "gst",
  label: "gst:TaxAmountMismatch",
  subject: "https://graph-owl.dev/packs/gst#inv-1",
  summary: "The tax differs between books and GSTR-2B",
  governedBy: "gst:Section16",
  evidence: [{ subject: "https://graph-owl.dev/packs/gst#inv-1", predicate: "gst:taxAmount", value: "18000.00" }],
  status: "pending",
  detectedAt: "2026-08-01T00:00:00Z",
  decidedBy: null,
  reason: null,
  priority: 1,
  subjectLabel: null,
  ...overrides,
});

describe("building the /findings query string", () => {
  it("asks for everything when no filter is set", () => {
    expect(findingsQueryString({})).toBe("");
  });

  it("narrows to one pack", () => {
    expect(findingsQueryString({ pack: "gst" })).toBe("?pack=gst");
  });

  it("narrows to one status", () => {
    expect(findingsQueryString({ status: "pending" })).toBe("?status=pending");
  });

  it("combines pack and status", () => {
    expect(findingsQueryString({ pack: "gst", status: "pending" })).toBe("?pack=gst&status=pending");
  });

  // A business admin filtering "gst" mismatches must not also see "hospitality"
  // findings that happen to share the same status letter-for-letter — a query
  // string that silently ignored the pack would show the whole estate instead
  // of the one pack the admin asked for.
  it("does not fall back to every pack when a specific one is requested", () => {
    expect(findingsQueryString({ pack: "hosp" })).not.toContain("gst");
  });
});

describe("which packs a loaded queue spans", () => {
  it("lists each pack once, sorted, so a filter dropdown is stable across loads", () => {
    const findings = [finding({ pack: "hosp" }), finding({ pack: "gst" }), finding({ pack: "gst" })];
    expect(packsIn(findings)).toEqual(["gst", "hosp"]);
  });

  it("is empty when nothing has loaded yet", () => {
    expect(packsIn([])).toEqual([]);
  });
});

describe("ordering a queue for review", () => {
  // The whole point of a queue: an admin working top-down should meet the
  // still-open questions before the ones already decided.
  it("puts pending findings ahead of accepted or rejected ones", () => {
    const decided = finding({ id: "decided", status: "accepted" });
    const open = finding({ id: "open", status: "pending" });

    const ordered = sortForReview([decided, open]);

    expect(ordered.map((f) => f.id)).toEqual(["open", "decided"]);
  });

  // Finding.priority: "Lower ranks more actionable" (graph-owl-core/src/finding.rs).
  // A queue that ignored it would bury the rule pack marked most urgent under
  // whatever happened to be inserted first.
  it("within the same status, ranks the lower (more actionable) priority first", () => {
    const low = finding({ id: "low-priority", priority: 2 });
    const high = finding({ id: "high-priority", priority: 1 });

    const ordered = sortForReview([low, high]);

    expect(ordered.map((f) => f.id)).toEqual(["high-priority", "low-priority"]);
  });

  it("treats a finding with no declared priority as least actionable, not most", () => {
    // A naive numeric sort (`undefined` coerced to 0, or sorting before any
    // number) would put an unranked finding ahead of a rule that explicitly
    // declared itself urgent — the opposite of what "no priority stated" means.
    const unranked = finding({ id: "unranked", priority: undefined });
    const ranked = finding({ id: "ranked", priority: 3 });

    const ordered = sortForReview([unranked, ranked]);

    expect(ordered.map((f) => f.id)).toEqual(["ranked", "unranked"]);
  });

  it("breaks a full tie by newest first, so today's reconciliation run leads", () => {
    const older = finding({ id: "older", detectedAt: "2026-08-01T00:00:00Z" });
    const newer = finding({ id: "newer", detectedAt: "2026-08-20T00:00:00Z" });

    const ordered = sortForReview([older, newer]);

    expect(ordered.map((f) => f.id)).toEqual(["newer", "older"]);
  });

  it("does not mutate the array it was given", () => {
    const findings = [finding({ id: "a", priority: 2 }), finding({ id: "b", priority: 1 })];
    const original = [...findings];

    sortForReview(findings);

    expect(findings).toEqual(original);
  });
});

describe("showing a finding's subject to a reviewer who cannot read raw IRIs", () => {
  it("prefers the server-resolved label when one exists", () => {
    expect(subjectDisplayLabel(finding({ subjectLabel: "Invoice INV-MAR-011" }))).toBe("Invoice INV-MAR-011");
  });

  // Verified against the real running deployment: every GST finding today
  // carries `subjectLabel: null` (no `[console.labels]` entry configured for
  // that pack yet), so falling back to the bare IRI is not a rare edge case —
  // it is the common one, and a raw `https://graph-owl.dev/packs/gst#...`
  // string is not something a business admin can read.
  it("falls back to a shortened prefix:local-name form when no label was resolved", () => {
    expect(subjectDisplayLabel(finding({ subjectLabel: null, subject: "https://graph-owl.dev/packs/gst#inv-1" }))).toBe(
      "gst:inv-1",
    );
  });

  it("falls back the same way when the label is present but blank", () => {
    expect(subjectDisplayLabel(finding({ subjectLabel: "  ", subject: "https://graph-owl.dev/packs/gst#inv-2" }))).toBe(
      "gst:inv-2",
    );
  });

  it("shortens an IRI with no vocabulary segment to its bare local name", () => {
    expect(subjectDisplayLabel(finding({ subjectLabel: null, subject: "https://example.org/inv-9" }))).toBe("inv-9");
  });
});
