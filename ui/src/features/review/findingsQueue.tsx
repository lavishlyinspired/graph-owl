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
import { api, type EvidenceGraph, type EvidenceGraphEdge, type PackFinding } from "../../api";
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
  evidenceGraphLabel: "Evidence graph",
  evidenceGraphSeedOnly: "Nothing else in the graph connects to this yet.",
  evidenceGraphTruncated: "Truncated — only part of the neighbourhood is shown.",
  evidenceGraphArrow: "—",
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

/** One edge, as a sentence a reviewer can follow without decoding IRIs —
 *  Epic 105 P7's console half. Both endpoints and the relationship itself
 *  are rendered through {@link displayTerm}: a derived edge's relationship
 *  can arrive as a full predicate IRI, and a reviewer reads it the same way
 *  they read a node. */
export function describeEvidenceEdge(edge: EvidenceGraphEdge): string {
  return `${displayTerm(edge.from)} ${COPY.evidenceGraphArrow}${displayTerm(edge.relationship)}→ ${displayTerm(edge.to)}`;
}

/** Whether the walk found nothing beyond the finding's own subject — an
 *  empty seed set (an unresolvable subject) counts the same as a lone one,
 *  since either way there is nothing for a reviewer to follow. */
export function evidenceGraphIsJustTheSeed(graph: EvidenceGraph): boolean {
  return graph.nodes.length <= 1 && graph.edges.length === 0;
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
      // A missing traversal engine or an unresolvable subject must not take
      // down the rest of the detail pane — the flat evidence list above
      // already carries the finding's citation, and this section is an
      // addition to it, not a dependency of it.
      const graph = await api.findingEvidenceGraph(entry.id).catch(() => null);
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
          {graph && (
            <div>
              <Text strong>{COPY.evidenceGraphLabel}</Text>
              {evidenceGraphIsJustTheSeed(graph) ? (
                <div>
                  <Text type="secondary">{COPY.evidenceGraphSeedOnly}</Text>
                </div>
              ) : (
                <>
                  {graph.edges.map((edge, index) => (
                    <div key={`${edge.from}-${edge.relationship}-${edge.to}-${index}`}>
                      {describeEvidenceEdge(edge)}
                    </div>
                  ))}
                  {graph.truncated && (
                    <div>
                      <Text type="secondary">{COPY.evidenceGraphTruncated}</Text>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
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
