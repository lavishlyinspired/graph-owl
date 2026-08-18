/** Epic 42's outbound webhook admin panel: pure decision logic.
 *
 *  **This module's own named RED test**: a successful delivery is *deleted*
 *  once acknowledged (`graph-owl-api`'s `OutboundWebhookSender::attempt`
 *  calls `delete_outbound_webhook_delivery` on success, never marks a
 *  "delivered" row) — so a row this panel ever sees is either still
 *  pending or has already given up. `describeDeliveryStatus` must never
 *  imply a third, "delivered" state for a row from this endpoint. */

import { describe, expect, it } from "vitest";
import type { OutboundWebhookDelivery } from "../../api";
import { describeDeliveryStatus } from "./outboundWebhooks";

function delivery(overrides: Partial<OutboundWebhookDelivery> = {}): OutboundWebhookDelivery {
  return {
    id: "d1",
    webhookId: "w1",
    payload: { kind: "created" },
    attempt: 0,
    nextAttemptAt: "2026-08-09T00:00:00Z",
    lastError: null,
    deadLettered: false,
    createdAt: "2026-08-09T00:00:00Z",
    ...overrides,
  };
}

describe("describeDeliveryStatus", () => {
  it("a never-attempted delivery is pending, not retrying", () => {
    const described = describeDeliveryStatus(delivery({ attempt: 0 }));

    expect(described.label).toBe("Pending");
    expect(described.tone).toBe("warning");
  });

  it("an attempted, still-queued delivery is retrying and names the attempt count", () => {
    const described = describeDeliveryStatus(
      delivery({ attempt: 3, lastError: "connection refused" }),
    );

    expect(described.label).toBe("Retrying");
    expect(described.tone).toBe("warning");
    expect(described.detail).toContain("3");
    expect(described.detail).toContain("connection refused");
  });

  /** The negative that makes the case above about the *error*, not just
   *  the attempt count: a retry with no recorded error yet must not
   *  fabricate one. */
  it("a retrying delivery with no recorded error yet does not invent one", () => {
    const described = describeDeliveryStatus(delivery({ attempt: 1, lastError: null }));

    expect(described.detail).not.toContain("null");
    expect(described.detail).not.toContain("undefined");
  });

  it("a dead-lettered delivery is reported as having given up, with the error that ended it", () => {
    const described = describeDeliveryStatus(
      delivery({ deadLettered: true, attempt: 5, lastError: "DNS resolution failed" }),
    );

    expect(described.label).toBe("Dead-lettered");
    expect(described.tone).toBe("error");
    expect(described.detail).toContain("5");
    expect(described.detail).toContain("DNS resolution failed");
  });

  /** And the negative for dead-lettered too: `admit_target` can dead-letter
   *  a delivery with no HTTP attempt at all (a refused target), so the
   *  error text must not depend on one having been recorded. */
  it("a dead-lettered delivery with no recorded error still reports having given up", () => {
    const described = describeDeliveryStatus(
      delivery({ deadLettered: true, attempt: 1, lastError: null }),
    );

    expect(described.label).toBe("Dead-lettered");
    expect(described.detail).not.toContain("null");
  });

  /** `deadLettered` takes priority over `attempt` — a dead-lettered
   *  delivery must never be reported as merely "retrying" regardless of
   *  what its attempt count says. */
  it("dead-lettered wins over attempt count when both are present", () => {
    const described = describeDeliveryStatus(delivery({ deadLettered: true, attempt: 0 }));

    expect(described.label).toBe("Dead-lettered");
  });
});
