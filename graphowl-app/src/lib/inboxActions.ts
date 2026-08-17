/** Maps an inbox item's `{source, decision}` onto the real endpoint that
 *  owns it — five different queues, five different accept/reject shapes
 *  (`crates/graph-owl-server/src/lib.rs`: `/proposals/{id}/accept`,
 *  `/change-proposals/{id}/reject` with a required reason,
 *  `/resolution/queue/{id}/confirm`, `/findings/{id}/decision` with a
 *  tagged status, `/extraction/claims/{id}/decision` with a tagged
 *  `outcome`). A pure function so the mapping — the part that silently
 *  routes an approve to the wrong queue if a source string is mistyped —
 *  is unit-testable without a network call. */

export type InboxDecision = "approve" | "reject";

export interface InboxAction {
  readonly method: "POST";
  readonly path: string;
  readonly body?: Record<string, unknown>;
}

/** Sources whose reject endpoint requires a non-empty reason — the "a
 *  decision with no reason teaches nothing" rule this project applies
 *  everywhere a queue entry is dismissed. `agent-proposal`'s reject takes
 *  none at all, which is why it is not in this set. */
export const INBOX_REJECT_REQUIRES_REASON: ReadonlySet<string> = new Set([
  "change-proposal",
  "resolution",
  "extraction-claim",
]);

export function resolveInboxAction(source: string, id: string, decision: InboxDecision, reason?: string): InboxAction {
  switch (source) {
    case "agent-proposal":
      return { method: "POST", path: `/proposals/${id}/${decision === "approve" ? "accept" : "reject"}` };

    case "change-proposal":
      return decision === "approve"
        ? { method: "POST", path: `/change-proposals/${id}/accept` }
        : { method: "POST", path: `/change-proposals/${id}/reject`, body: { reason: reason ?? "" } };

    case "resolution":
      return decision === "approve"
        ? { method: "POST", path: `/resolution/queue/${id}/confirm` }
        : { method: "POST", path: `/resolution/queue/${id}/reject`, body: { reason: reason ?? "" } };

    case "finding":
      return {
        method: "POST",
        path: `/findings/${id}/decision`,
        body: { status: decision === "approve" ? "accepted" : "rejected", reason: reason ?? null },
      };

    case "extraction-claim":
      return decision === "approve"
        ? { method: "POST", path: `/extraction/claims/${id}/decision`, body: { outcome: "accept" } }
        : { method: "POST", path: `/extraction/claims/${id}/decision`, body: { outcome: "reject", reason: reason ?? "" } };

    default:
      throw new Error(`unknown inbox source "${source}"`);
  }
}
