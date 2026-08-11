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
import type { CSSProperties } from "react";
import {
  api,
  type EvidenceGraph,
  type EvidenceGraphEdge,
  type EvidenceGraphNode,
  type PackFinding,
} from "../../api";
import type { Picture } from "../../graph/cytoscape";
import { GraphCanvas } from "../../graph/GraphCanvas";
import { palette } from "../../theme";
import { readParam } from "../deepLink";
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
  sourcesLabel: "Sources",
  sourcesEmpty: "no source recorded",
  triplesLabel: "Triples",
  predicateLabel: "Predicate",
  objectLabel: "Object",
  derivedTag: "derived",
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

const triplesHeaderStyle: CSSProperties = {
  textAlign: "left",
  fontSize: 12,
  fontWeight: 600,
  padding: "4px 8px 4px 0",
  borderBottom: "1px solid rgba(0, 0, 0, 0.08)",
};

const triplesCellStyle: CSSProperties = {
  fontSize: 13,
  padding: "4px 8px 4px 0",
  borderBottom: "1px solid rgba(0, 0, 0, 0.04)",
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

/** The evidence graph, as the shared `GraphCanvas` wants it — the same
 *  Cytoscape canvas `Explore`'s asset graph already draws with, reused here
 *  rather than a second renderer for a second kind of graph.
 *
 *  **`EvidenceGraphEdge` already has the shape `GraphEdge` does** — `{from,
 *  to, relationship, derived?}` — so edges pass through unchanged; only the
 *  nodes need adapting, since a finding's subject is not a catalog asset
 *  and so has no `name`/`kind` the way `/assets/{id}/graph`'s nodes do.
 *
 *  **Every node counts as already expanded.** The evidence graph is fetched
 *  whole in one call, unlike the asset explorer's click-to-reveal picture —
 *  marking a node `expandable` here would draw a ring promising an
 *  interaction nothing behind it honours. */
/** A node's own resolved IRI, read the way a reviewer reads any other term
 *  in this file — falling back to the bare id when the namespace never
 *  resolved. Shared between the rendered picture and the sources list below
 *  it, so both name the same node the same way. */
function nodeLabel(node: EvidenceGraphNode): string {
  return node.iri ? displayTerm(node.iri) : node.id;
}

export function evidencePicture(finding: PackFinding, graph: EvidenceGraph): Picture {
  const seedId = displayTerm(finding.subject);
  const nodeIds = graph.nodes.map((node) => node.id);
  return {
    seedId,
    nodes: graph.nodes.map((node) => ({
      id: node.id,
      name: nodeLabel(node),
      kind: null,
    })),
    edges: graph.edges,
    expanded: nodeIds,
    // The API reports truncation for the walk as a whole, not per node; the
    // seed is where a reader would look to learn more is out there, so it
    // carries the mark.
    truncatedAt: graph.truncated ? [seedId] : [],
  };
}

/** Which source document(s) asserted each node's own flakes — Epic 105 P7's
 *  provenance half (`plans/105g-evidence-provenance-and-near-miss.md`). The
 *  API has already deduplicated per node; this only adds the same display
 *  name the canvas and the triples table already use, so a reviewer reads
 *  one name for one node everywhere on the page. */
export function evidenceNodeSources(
  graph: EvidenceGraph,
): { id: string; name: string; sources: readonly string[] }[] {
  return graph.nodes.map((node) => ({
    id: node.id,
    name: nodeLabel(node),
    sources: node.sources,
  }));
}

/** One row per edge — a finding's evidence graph, as the triples it actually
 *  is. `{from, relationship, to}` on the wire *is* `{subject, predicate,
 *  object}`; this just names it the way a reader who wants to see raw
 *  triples, not a rendered picture, expects to read it. */
export function evidenceTriples(
  graph: EvidenceGraph,
): { subject: string; predicate: string; object: string; derived: boolean }[] {
  return graph.edges.map((edge) => ({
    subject: displayTerm(edge.from),
    predicate: displayTerm(edge.relationship),
    object: displayTerm(edge.to),
    derived: edge.derived ?? false,
  }));
}

/** Light by default, matching `App.tsx`'s own `useTheme()` precedence
 *  (`?theme=` first, then the persisted choice) — `GraphCanvas` needs
 *  concrete colour values to hand Cytoscape, so it cannot read the CSS
 *  custom properties the rest of this file's markup relies on. Duplicated
 *  rather than imported: `useTheme` is a hook local to `App.tsx`, and this
 *  file is plain data-fetching functions, not a component that could call
 *  one. */
function currentGraphColors(): (typeof palette)["light"] {
  const stored =
    typeof window === "undefined" ? null : window.localStorage.getItem("graphowl.theme.v2");
  const dark = (readParam("theme") ?? stored) === "dark";
  return dark ? palette.dark : palette.light;
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
                  <div style={{ marginTop: 8 }}>
                    <GraphCanvas
                      picture={evidencePicture(finding, graph)}
                      colors={currentGraphColors()}
                      onExpand={() => {}}
                      label={`Evidence graph for ${displayTerm(finding.subject)}`}
                    />
                  </div>
                  {/* The canvas's own `role="img"`/`aria-label` names the
                   *  picture but not its content — this is the same
                   *  non-visual equivalent `00f` requires of any rendered
                   *  graph, not a duplicate of the canvas above it. */}
                  <div style={{ marginTop: 8 }}>
                    {graph.edges.map((edge, index) => (
                      <div key={`${edge.from}-${edge.relationship}-${edge.to}-${index}`}>
                        <Text type="secondary">{describeEvidenceEdge(edge)}</Text>
                      </div>
                    ))}
                  </div>
                  {graph.truncated && (
                    <div>
                      <Text type="secondary">{COPY.evidenceGraphTruncated}</Text>
                    </div>
                  )}
                  <div style={{ marginTop: 8 }}>
                    <Text strong>{COPY.sourcesLabel}</Text>
                    {evidenceNodeSources(graph).map((row) => (
                      <div key={row.id}>
                        <Text type="secondary">
                          {row.name}
                          {COPY.separator}
                          {row.sources.length > 0 ? (
                            row.sources.map((source, index) => (
                              <Tag key={source} style={{ marginLeft: index === 0 ? 6 : 0 }}>
                                {source}
                              </Tag>
                            ))
                          ) : (
                            <Text type="secondary">{COPY.sourcesEmpty}</Text>
                          )}
                        </Text>
                      </div>
                    ))}
                  </div>
                  <div style={{ marginTop: 8 }}>
                    <Text strong>{COPY.triplesLabel}</Text>
                    <table style={{ width: "100%", borderCollapse: "collapse", marginTop: 4 }}>
                      <thead>
                        <tr>
                          <th style={triplesHeaderStyle}>{COPY.subjectLabel}</th>
                          <th style={triplesHeaderStyle}>{COPY.predicateLabel}</th>
                          <th style={triplesHeaderStyle}>{COPY.objectLabel}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {evidenceTriples(graph).map((row, index) => (
                          <tr key={`${row.subject}-${row.predicate}-${row.object}-${index}`}>
                            <td style={triplesCellStyle}>{row.subject}</td>
                            <td style={triplesCellStyle}>
                              {row.predicate}
                              {row.derived && (
                                <Tag style={{ marginLeft: 6 }}>{COPY.derivedTag}</Tag>
                              )}
                            </td>
                            <td style={triplesCellStyle}>{row.object}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
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
