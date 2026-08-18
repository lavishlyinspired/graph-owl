/** Epic 35's change proposals, over HTTP, as a `QueueConfig` — Phase 3
 *  item 3.2. Accept and reject mirror the resolution queue's shape: accept
 *  is `instant` (attribution goes to the proposer, not the accepter — the
 *  server's own decision 3), reject is `withText` and requires a reason,
 *  the same "a decision with no reason teaches nothing" rule drift and
 *  merge review already enforce.
 */

import { Space, Typography } from "./../../components/ui/antd-compat";
import { api, type ChangeProposal, type ChangeProposalStatus } from "../../api";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text } = Typography;

const COPY = {
  label: "Proposals",
  subtitle: "Field changes anyone proposed, waiting on an owner's decision.",
  emptyPendingTitle: "Nothing proposed",
  emptyPendingDescription: "No open proposal is waiting on a decision.",
  emptyAcceptedTitle: "No proposal accepted yet",
  emptyAcceptedDescription: "No proposal has been accepted yet.",
  emptyRejectedTitle: "No proposal rejected yet",
  emptyRejectedDescription: "No proposal has been rejected yet.",
  fieldLabel: "Field",
  currentLabel: "Current value",
  proposedLabel: "Proposed value",
  rationaleLabel: "Rationale",
  acceptAction: "Accept",
  acceptConfirmTitle: "Accept this proposal?",
  acceptConfirmBody: "This writes the proposed value into the live catalog, attributed to whoever proposed it.",
  rejectAction: "Reject",
  rejectModalTitle: "Reject this proposal",
  rejectHint: "A rejection needs a reason — without one, nobody reviewing this later can tell why it didn't land.",
  rejectPlaceholder: "Why doesn't this proposal stand?",
};

function displayValue(value: string | null): string {
  return value === null ? "(none)" : value;
}

function toQueueEntry(proposal: ChangeProposal): QueueEntry {
  return {
    id: proposal.id,
    status: proposal.status,
    summary: proposal.field,
    detail: `${proposal.proposedBy} proposes ${displayValue(proposal.proposedValue)}`,
    decidedSummary:
      proposal.status === "pending" ? undefined : `${proposal.status}${proposal.decidedBy ? ` by ${proposal.decidedBy}` : ""}`,
    reason: proposal.decisionReason ?? undefined,
  };
}

export function proposalsQueue(): QueueConfig {
  const raw = new Map<string, ChangeProposal>();

  return {
    key: "proposals",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [
      { key: "pending", label: "Pending" },
      { key: "accepted", label: "Accepted" },
      { key: "rejected", label: "Rejected" },
    ],
    actions: [
      {
        key: "accept",
        label: COPY.acceptAction,
        kind: "instant",
        confirmTitle: COPY.acceptConfirmTitle,
        confirmBody: COPY.acceptConfirmBody,
        doneMessage: "Accepted.",
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
    ],
    emptyTitle: (status) =>
      status === "accepted"
        ? COPY.emptyAcceptedTitle
        : status === "rejected"
          ? COPY.emptyRejectedTitle
          : COPY.emptyPendingTitle,
    emptyDescription: (status) =>
      status === "accepted"
        ? COPY.emptyAcceptedDescription
        : status === "rejected"
          ? COPY.emptyRejectedDescription
          : COPY.emptyPendingDescription,
    async fetchEntries(status) {
      const page = await api.changeProposals({ status: status as ChangeProposalStatus });
      raw.clear();
      for (const proposal of page.data) raw.set(proposal.id, proposal);
      return page.data.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const proposal = raw.get(entry.id);
      if (!proposal) return null;
      return (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <div>
            <Text strong>{COPY.fieldLabel}</Text>
            <div>{proposal.field}</div>
          </div>
          <div>
            <Text strong>{COPY.currentLabel}</Text>
            <div>{displayValue(proposal.currentValue)}</div>
          </div>
          <div>
            <Text strong>{COPY.proposedLabel}</Text>
            <div>{displayValue(proposal.proposedValue)}</div>
          </div>
          <div>
            <Text strong>{COPY.rationaleLabel}</Text>
            <div>{proposal.rationale}</div>
          </div>
        </Space>
      );
    },
    async performAction(entry, actionKey, text) {
      return performAsAction(async () => {
        if (actionKey === "accept") return api.acceptChangeProposal(entry.id);
        if (actionKey === "reject") return api.rejectChangeProposal(entry.id, text ?? "");
      });
    },
  };
}
