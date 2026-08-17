import { describe, expect, it } from "vitest";
import { INBOX_REJECT_REQUIRES_REASON, resolveInboxAction } from "./inboxActions";

describe("resolveInboxAction", () => {
  it("routes an agent-proposal approve to /proposals/{id}/accept with no body", () => {
    expect(resolveInboxAction("agent-proposal", "p1", "approve")).toEqual({
      method: "POST",
      path: "/proposals/p1/accept",
    });
  });

  it("routes an agent-proposal reject to /proposals/{id}/reject with no body — the one source with none", () => {
    expect(resolveInboxAction("agent-proposal", "p1", "reject")).toEqual({
      method: "POST",
      path: "/proposals/p1/reject",
    });
  });

  it("routes a change-proposal approve to /change-proposals/{id}/accept", () => {
    expect(resolveInboxAction("change-proposal", "cp1", "approve")).toEqual({
      method: "POST",
      path: "/change-proposals/cp1/accept",
    });
  });

  it("routes a change-proposal reject with the reason in the body", () => {
    expect(resolveInboxAction("change-proposal", "cp1", "reject", "stale value")).toEqual({
      method: "POST",
      path: "/change-proposals/cp1/reject",
      body: { reason: "stale value" },
    });
  });

  it("routes a resolution approve to /resolution/queue/{id}/confirm", () => {
    expect(resolveInboxAction("resolution", "r1", "approve")).toEqual({
      method: "POST",
      path: "/resolution/queue/r1/confirm",
    });
  });

  it("routes a resolution reject with the reason in the body", () => {
    expect(resolveInboxAction("resolution", "r1", "reject", "not the same entity")).toEqual({
      method: "POST",
      path: "/resolution/queue/r1/reject",
      body: { reason: "not the same entity" },
    });
  });

  it("routes a finding decision to the one tagged-status endpoint, both directions", () => {
    expect(resolveInboxAction("finding", "f1", "approve")).toEqual({
      method: "POST",
      path: "/findings/f1/decision",
      body: { status: "accepted", reason: null },
    });
    expect(resolveInboxAction("finding", "f1", "reject", "false positive")).toEqual({
      method: "POST",
      path: "/findings/f1/decision",
      body: { status: "rejected", reason: "false positive" },
    });
  });

  it("routes an extraction-claim decision to the tagged-outcome endpoint, both directions", () => {
    expect(resolveInboxAction("extraction-claim", "c1", "approve")).toEqual({
      method: "POST",
      path: "/extraction/claims/c1/decision",
      body: { outcome: "accept" },
    });
    expect(resolveInboxAction("extraction-claim", "c1", "reject", "wrong subject")).toEqual({
      method: "POST",
      path: "/extraction/claims/c1/decision",
      body: { outcome: "reject", reason: "wrong subject" },
    });
  });

  it("throws on an unrecognized source rather than silently routing nowhere", () => {
    expect(() => resolveInboxAction("mystery", "x", "approve")).toThrow(/unknown inbox source/);
  });

  /** The negative case a copy-pasted switch arm gets wrong: two sources
   *  reusing the same path template with only the id substituted still
   *  "work" for one of them. */
  it("every source with an approve/reject pair produces two distinct paths", () => {
    for (const source of ["agent-proposal", "change-proposal", "resolution"]) {
      const approve = resolveInboxAction(source, "same-id", "approve");
      const reject = resolveInboxAction(source, "same-id", "reject", "why");
      expect(approve.path).not.toBe(reject.path);
    }
  });
});

describe("INBOX_REJECT_REQUIRES_REASON", () => {
  it("does not include agent-proposal — its reject endpoint takes no body", () => {
    expect(INBOX_REJECT_REQUIRES_REASON.has("agent-proposal")).toBe(false);
  });

  it("includes every source whose reject endpoint validates a non-empty reason", () => {
    expect(INBOX_REJECT_REQUIRES_REASON.has("change-proposal")).toBe(true);
    expect(INBOX_REJECT_REQUIRES_REASON.has("resolution")).toBe(true);
    expect(INBOX_REJECT_REQUIRES_REASON.has("extraction-claim")).toBe(true);
  });
});
