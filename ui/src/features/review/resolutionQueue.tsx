/** Epic 17's resolution queue (merge adjudication) as a `QueueConfig` —
 *  Slice C's original `ReviewQueue.tsx` content, moved here unchanged in
 *  substance once the component itself went generic in Slice D. The
 *  side-by-side comparison table is this queue's own evidence renderer;
 *  no other config file has one shaped like it. */

import { Space, Typography } from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import { api, type Evidence, type ReviewQueueEntry, type ReviewStatus } from "../../api";
import { compareAssets } from "./reviewDiff";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text } = Typography;

const COPY = {
  label: "Merge review",
  subtitle: "Ambiguous entity matches the resolver could not decide on its own.",
  emptyPendingTitle: "Nothing to review",
  emptyPendingDescription: "No ambiguous matches are waiting for a decision.",
  emptyConfirmedTitle: "No merges yet",
  emptyConfirmedDescription: "No candidate has been confirmed as a merge yet.",
  emptyRejectedTitle: "No rejections yet",
  emptyRejectedDescription: "No candidate has been rejected yet.",
  matchSuffix: "% match",
  targetColumn: "Target",
  candidateColumn: "Candidate",
  confirmTitle: "Merge these two entities?",
  confirmBody: "This can be undone later by splitting the merge — it is not a silent, permanent change.",
  mergeAction: "Merge",
  rejectAction: "Reject",
  rejectModalTitle: "Reject this match",
  rejectHint:
    "A rejection has to say why — without a reason, nobody reviewing this later can tell why the resolver's suggestion was wrong.",
  rejectPlaceholder: "Why is this not a match?",
  deferAction: "Defer",
};

function evidenceLabel(evidence: Evidence): string {
  switch (evidence.kind) {
    case "exactFqn":
      return "exact fully-qualified name";
    case "normalizedFqn":
      return "normalized fully-qualified name";
    case "exactName":
      return `exact name within ${evidence.scope}`;
    case "nameSimilarity":
      return `name similarity (${evidence.metric} ${evidence.value.toFixed(2)})`;
    case "structuralOverlap":
      return `${evidence.sharedColumns}/${evidence.total} shared columns`;
    case "sameParent":
      return "same parent";
    case "sameSourceSystem":
      return "same source system";
  }
}

function deciderLabel(entry: ReviewQueueEntry): string {
  if (!entry.decidedBy) return "";
  if (entry.decidedBy.kind === "human") return `by ${entry.decidedBy.userId}`;
  if (entry.decidedBy.kind === "agent") return `by agent ${entry.decidedBy.agentId}`;
  return "automatically";
}

function toQueueEntry(entry: ReviewQueueEntry): QueueEntry {
  return {
    id: entry.id,
    status: entry.status,
    summary: `${Math.round(entry.score * 100)}${COPY.matchSuffix}`,
    detail: entry.evidence.map(evidenceLabel).join(", "),
    decidedSummary: entry.status === "pending" ? undefined : `${entry.status} ${deciderLabel(entry)}`.trim(),
    reason: entry.reason,
  };
}

export function resolutionQueue(): QueueConfig {
  // The raw entries from the last `fetchEntries` call, keyed by id, so
  // `fetchDetail`/`performAction` can recover the real `target`/`candidate`
  // ids a generic `QueueEntry` does not carry.
  const raw = new Map<string, ReviewQueueEntry>();

  return {
    key: "resolution",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [
      { key: "pending", label: "Pending" },
      { key: "confirmed", label: "Confirmed" },
      { key: "rejected", label: "Rejected" },
    ],
    actions: [
      {
        key: "merge",
        label: COPY.mergeAction,
        kind: "instant",
        confirmTitle: COPY.confirmTitle,
        confirmBody: COPY.confirmBody,
        doneMessage: "Merged.",
      },
      {
        key: "reject",
        label: COPY.rejectAction,
        kind: "withText",
        danger: true,
        textModalTitle: COPY.rejectModalTitle,
        textModalHint: COPY.rejectHint,
        textPlaceholder: COPY.rejectPlaceholder,
        doneMessage: "Rejected.",
      },
      { key: "defer", label: COPY.deferAction, kind: "clientOnly" },
    ],
    emptyTitle: (status) =>
      status === "confirmed"
        ? COPY.emptyConfirmedTitle
        : status === "rejected"
          ? COPY.emptyRejectedTitle
          : COPY.emptyPendingTitle,
    emptyDescription: (status) =>
      status === "confirmed"
        ? COPY.emptyConfirmedDescription
        : status === "rejected"
          ? COPY.emptyRejectedDescription
          : COPY.emptyPendingDescription,
    async fetchEntries(status) {
      const page = await api.reviewQueue({ status: status as ReviewStatus });
      raw.clear();
      for (const entry of page.data) raw.set(entry.id, entry);
      return page.data.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const source = raw.get(entry.id);
      if (!source) return null;
      const [target, candidate] = await Promise.all([api.asset(source.target), api.asset(source.candidate)]);
      const comparison = compareAssets(target, candidate);
      return (
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th style={{ textAlign: "left", padding: 8 }} />
              <th style={{ textAlign: "left", padding: 8 }}>{COPY.targetColumn}</th>
              <th style={{ textAlign: "left", padding: 8 }}>{COPY.candidateColumn}</th>
            </tr>
          </thead>
          <tbody>
            {comparison.map((field) => (
              <tr key={field.field} style={{ borderTop: "1px solid rgba(0,0,0,0.06)" }}>
                <td style={{ padding: 8 }}>
                  <Space size={4}>
                    {field.matches ? (
                      <CheckCircleOutlined style={{ color: "#52c41a" }} />
                    ) : (
                      <WarningOutlined style={{ color: "#faad14" }} />
                    )}
                    <Text strong>{field.label}</Text>
                  </Space>
                </td>
                <td style={{ padding: 8 }}>{field.targetValue}</td>
                <td style={{ padding: 8 }}>{field.candidateValue}</td>
              </tr>
            ))}
          </tbody>
        </table>
      );
    },
    async performAction(entry, actionKey, text) {
      return performAsAction(async () => {
        if (actionKey === "merge") return api.confirmReview(entry.id);
        if (actionKey === "reject") return api.rejectReview(entry.id, text ?? "");
      });
    },
  };
}
