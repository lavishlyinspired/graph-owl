/** Epic 104's cross-vocabulary alignment review band, over HTTP, as a 5th
 *  `QueueConfig` — Phase 3 item 3.14, unblocked by decision 4.7 ("design
 *  the DTO now, as part of the build").
 *
 *  **This queue has no server-tracked status transitions**, unlike every
 *  other one here. `GET /alignments/review` always returns exactly the
 *  entries currently sitting in decision 4's `0.5..0.8` confidence band —
 *  there is no "confirmed"/"rejected" history to page through, because
 *  neither action here writes a status field; both just re-`POST` the same
 *  alignment through `Catalog::upsert_alignment`'s existing confidence
 *  gate. `statuses` is therefore a single `"pending"` entry, and every
 *  fetched entry reports that status — never anything else.
 *
 *  **Confirm** re-posts with `source.kind: "human"` and `confidence: 1`.
 *  1, not the entry's own (usually sub-0.8) computed score, because a human
 *  looking at this and vouching for it is a stronger claim than the
 *  original guess — and only a value `>= Disposition::ASSERT_THRESHOLD`
 *  (0.8, `graph_owl_core::extraction`) actually clears `alignment_to_
 *  flakes_gated` into writing the direct triple. Re-posting the unchanged
 *  0.6-ish score would leave the entry sitting in the exact same review
 *  band it started in — a "Confirm" that confirmed nothing.
 *
 *  **Reject has no dedicated backend route — none exists, and adding one
 *  was out of scope for this item.** Posting the same alignment with
 *  `confidence: 0` produces the same effect through a route that already
 *  ships: `Disposition::for_confidence(0)` is `Ignore`, so `alignment_to_
 *  flakes_gated` writes nothing new, while `Catalog::upsert_alignment`'s
 *  retract-then-assert step still retracts the *existing* Surface-band
 *  metadata (it always retracts whatever is currently stored at that
 *  subject before deciding what to write next). Net effect: the entry
 *  disappears from the review queue and leaves no graph edge — dismissed,
 *  not merely hidden client-side. There is deliberately no rejection
 *  reason field: `UpsertAlignmentRequest` has nowhere to store one, and a
 *  text box that silently discarded what was typed into it would be worse
 *  than not asking. */

import { Space, Typography } from "antd";
import { api, type AlignmentReviewEntry, type UpsertAlignmentRequest } from "../../api";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text } = Typography;

const COPY = {
  label: "Alignments",
  subtitle: "Cross-vocabulary matches confident enough to record, not confident enough to become a graph edge on their own.",
  emptyPendingTitle: "Nothing to review",
  emptyPendingDescription: "No computed alignment is currently sitting in the review band.",
  confidenceSuffix: "% confidence",
  leftLabel: "Left",
  rightLabel: "Right",
  predicateLabel: "Predicate",
  sourceLabel: "Source",
  lossyReverseLabel: "Reversing this loses information",
  confirmAction: "Confirm",
  confirmConfirmTitle: "Confirm this alignment?",
  confirmConfirmBody: "This writes the direct triple into the live graph, attributed to you as a human confirmation.",
  rejectAction: "Reject",
  rejectConfirmTitle: "Reject this alignment?",
  rejectConfirmBody: "This removes the alignment from the graph entirely — there is no direct triple to retract, only this review-band metadata.",
  unknown: "(unknown)",
  none: "(none)",
};

function displayTerm(term: string | null): string {
  return term ?? COPY.unknown;
}

function toQueueEntry(entry: AlignmentReviewEntry): QueueEntry {
  const confidencePct = entry.confidence === null ? "?" : Math.round(entry.confidence * 100);
  return {
    id: entry.subject,
    status: "pending",
    summary: `${confidencePct}${COPY.confidenceSuffix}`,
    detail: `${displayTerm(entry.left)} ${entry.predicate ?? "?"} ${displayTerm(entry.right)}`,
  };
}

/** Both actions need the same request shape, differing only in `source`
 *  and `confidence` — this builds the part they share, including the
 *  `predicate`-as-`kind`-discriminator decoding `AlignmentReviewEntry`'s
 *  own doc comment names (`graph-owl-api`'s `AlignmentReviewEntry.
 *  predicate` field: no separate `kind` was added because
 *  `Alignment::parts()` already writes the literal string
 *  `"equivalentClass"` for that variant). Throws if the entry is missing a
 *  field every real review entry carries (`left`/`right`/`predicate` are
 *  always written together by `metadata_flakes`) — a defensive read, not
 *  an expected path.
 */
function baseRequest(entry: AlignmentReviewEntry): Omit<UpsertAlignmentRequest, "source" | "confidence"> {
  if (!entry.left || !entry.right || !entry.predicate) {
    throw new Error("this review entry is missing left, right, or predicate — it cannot be acted on");
  }
  const kind = entry.predicate === "equivalentClass" ? "equivalentClass" : "match";
  return {
    kind,
    left: entry.left,
    right: entry.right,
    predicate: kind === "match" ? entry.predicate : undefined,
    lossyReverse: entry.lossyReverse ?? false,
  };
}

export function alignmentQueue(): QueueConfig {
  const raw = new Map<string, AlignmentReviewEntry>();

  return {
    key: "alignment",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [{ key: "pending", label: "Pending" }],
    actions: [
      {
        key: "confirm",
        label: COPY.confirmAction,
        kind: "instant",
        confirmTitle: COPY.confirmConfirmTitle,
        confirmBody: COPY.confirmConfirmBody,
        doneMessage: "Confirmed.",
      },
      {
        key: "reject",
        label: COPY.rejectAction,
        kind: "instant",
        danger: true,
        confirmTitle: COPY.rejectConfirmTitle,
        confirmBody: COPY.rejectConfirmBody,
        doneMessage: "Rejected.",
      },
    ],
    emptyTitle: () => COPY.emptyPendingTitle,
    emptyDescription: () => COPY.emptyPendingDescription,
    async fetchEntries() {
      const page = await api.alignmentReviewQueue();
      raw.clear();
      for (const entry of page) raw.set(entry.subject, entry);
      return page.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const source = raw.get(entry.id);
      if (!source) return null;
      return (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <div>
            <Text strong>{COPY.leftLabel}</Text>
            <div>{displayTerm(source.left)}</div>
          </div>
          <div>
            <Text strong>{COPY.rightLabel}</Text>
            <div>{displayTerm(source.right)}</div>
          </div>
          <div>
            <Text strong>{COPY.predicateLabel}</Text>
            <div>{source.predicate ?? COPY.unknown}</div>
          </div>
          <div>
            <Text strong>{COPY.sourceLabel}</Text>
            <div>
              {source.sourceKind ?? COPY.unknown}
              {source.sourceDetail ? ` — ${source.sourceDetail}` : ""}
            </div>
          </div>
          {source.lossyReverse && <Text type="warning">{COPY.lossyReverseLabel}</Text>}
        </Space>
      );
    },
    async performAction(entry, actionKey) {
      const source = raw.get(entry.id);
      if (!source) throw new Error("this entry is no longer available");
      return performAsAction(async () => {
        if (actionKey === "confirm") {
          const who = await api.whoAmI();
          return api.upsertAlignment({
            ...baseRequest(source),
            source: { kind: "human", detail: who.name },
            confidence: 1,
          });
        }
        if (actionKey === "reject") {
          return api.upsertAlignment({
            ...baseRequest(source),
            source: {
              kind: source.sourceKind ?? "human",
              detail: source.sourceDetail ?? "",
            },
            confidence: 0,
          });
        }
      });
    },
  };
}
