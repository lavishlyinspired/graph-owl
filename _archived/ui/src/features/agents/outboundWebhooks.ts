/** Epic 42's outbound webhook admin panel: pure decision logic.
 *
 *  A successful delivery is *deleted* once acknowledged
 *  (`OutboundWebhookSender::attempt` calls `delete_outbound_webhook_delivery`
 *  on success rather than marking a row "delivered") — so every row this
 *  panel ever sees is either still pending/retrying or has already given
 *  up. There is no "delivered" state to describe, and this module must
 *  never invent one. */

import type { OutboundWebhookDelivery } from "../../api";

export interface DeliveryStatus {
  readonly label: string;
  readonly tone: "warning" | "error";
  readonly detail: string;
}

export function describeDeliveryStatus(delivery: OutboundWebhookDelivery): DeliveryStatus {
  if (delivery.deadLettered) {
    return {
      label: "Dead-lettered",
      tone: "error",
      detail: delivery.lastError
        ? `Gave up after ${delivery.attempt} attempt(s): ${delivery.lastError}`
        : `Gave up after ${delivery.attempt} attempt(s).`,
    };
  }
  if (delivery.attempt === 0) {
    return { label: "Pending", tone: "warning", detail: "Not yet attempted." };
  }
  return {
    label: "Retrying",
    tone: "warning",
    detail: delivery.lastError
      ? `Attempt ${delivery.attempt} failed: ${delivery.lastError}`
      : `Attempt ${delivery.attempt} — next retry pending.`,
  };
}
