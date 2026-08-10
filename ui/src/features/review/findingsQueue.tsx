/** Reconciliation findings, as a `QueueConfig` — Epic 105 P5.
 *
 *  **One queue for every domain pack, and that is the whole point.** GST's
 *  input-tax-credit reconciliation and a hospitality duplicate-guest rule
 *  render through this same file; the difference between them lives in
 *  `packs/<id>/pack.toml` and in the findings the runtime wrote, never here.
 *  A `GstFindingsQueue.tsx` would be the first per-domain hardcoding in the
 *  console, and `plans/105-domain-neutrality.md` exists to prevent exactly
 *  that — the second one always follows.
 *
 *  **The citation is rendered first, above the evidence.** A finding is an
 *  accusation about somebody's data, and the rule it rests on is what makes
 *  it reviewable rather than an assertion; a reviewer who cannot see *why*
 *  the system concluded something can only accept it on trust, which is the
 *  failure mode this product is positioned against.
 *
 *  **Accept is `instant`, dismiss is `withText`.** The asymmetry matches the
 *  server's: accepting means "yes, this is real", which the finding already
 *  explains, while dismissing is what the next run must be able to tell apart
 *  from "nobody has looked yet". There is no defer — leaving it pending
 *  already *is* deferring, the same reasoning Epic 17's queue records. */

import { Space, Tag, Typography } from "antd";
import { api, type PackFinding } from "../../api";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text } = Typography;

const COPY = {
  label: "Findings",
  subtitle:
    "What a domain pack's rules concluded, with the rule each one rests on. One queue for every pack.",
  emptyPendingTitle: "Nothing to review",
  emptyPendingDescription:
    "No pack's rules have produced a finding that is still waiting on a decision.",
  emptyAcceptedTitle: "Nothing accepted yet",
  emptyAcceptedDescription: "No finding has been accepted yet.",
  emptyRejectedTitle: "Nothing dismissed yet",
  emptyRejectedDescription: "No finding has been dismissed yet.",
  governedByLabel: "Rule",
  subjectLabel: "Subject",
  evidenceLabel: "Evidence",
  acceptAction: "Accept",
  acceptConfirmTitle: "Accept this finding?",
  acceptConfirmBody:
    "This records that the finding is real. It stays on the record with your name against it.",
  rejectAction: "Dismiss",
  rejectModalTitle: "Dismiss this finding",
  rejectHint:
    "A dismissal has to say why. Without a reason the next reconciliation run cannot tell 'considered and dismissed' from 'nobody has looked yet' — and neither can the next reviewer.",
  rejectPlaceholder: "Why should this finding not stand?",
  separator: " · ",
};

/** The local name of a term, for a reviewer who does not want to read IRIs.
 *
 *  Falls back to the whole string rather than an empty one: a subject with no
 *  separator is unusual, but a blank row in a review queue is unusable. */
export function displayTerm(term: string): string {
  const cut = Math.max(term.lastIndexOf("#"), term.lastIndexOf("/"));
  const tail = cut >= 0 ? term.slice(cut + 1) : term;
  return tail.length > 0 ? tail : term;
}

export function toQueueEntry(finding: PackFinding): QueueEntry {
  return {
    id: finding.id,
    status: finding.status,
    summary: displayTerm(finding.label),
    detail: `${finding.pack}${COPY.separator}${displayTerm(finding.subject)}${COPY.separator}${finding.summary}`,
    decidedSummary:
      finding.status === "pending"
        ? undefined
        : `${finding.status}${finding.decidedBy ? ` by ${finding.decidedBy}` : ""}`,
    reason: finding.reason ?? undefined,
  };
}

export function findingsQueue(): QueueConfig {
  const raw = new Map<string, PackFinding>();

  return {
    key: "findings",
    label: COPY.label,
    subtitle: COPY.subtitle,
    openStatus: "pending",
    statuses: [
      { key: "pending", label: "Pending" },
      { key: "accepted", label: "Accepted" },
      { key: "rejected", label: "Dismissed" },
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
        doneMessage: "Dismissed.",
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
      const findings = await api.findings({ status });
      raw.clear();
      for (const finding of findings) raw.set(finding.id, finding);
      return findings.map(toQueueEntry);
    },
    async fetchDetail(entry) {
      const finding = raw.get(entry.id);
      if (!finding) return null;
      return (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <div>
            <Text strong>{COPY.governedByLabel}</Text>
            <div>
              <Tag>{finding.governedBy}</Tag>
            </div>
          </div>
          <div>
            <Text strong>{COPY.subjectLabel}</Text>
            <div>{finding.subject}</div>
          </div>
          <div>
            <Text strong>{COPY.evidenceLabel}</Text>
            {finding.evidence.map((fact, index) => (
              <div key={`${fact.predicate}-${index}`}>
                <Text type="secondary">{displayTerm(fact.predicate)}</Text>
                {COPY.separator}
                {fact.value}
              </div>
            ))}
          </div>
        </Space>
      );
    },
    async performAction(entry, actionKey, text) {
      return performAsAction(async () => {
        if (actionKey === "accept") return api.decideFinding(entry.id, "accepted");
        if (actionKey === "reject") return api.decideFinding(entry.id, "rejected", text ?? "");
      });
    },
  };
}
