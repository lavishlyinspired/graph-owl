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

import { Card, Descriptions, Flex, Space, Table, Tag, Typography } from "./../../components/ui/antd-compat";
import {
  api,
  type EvidenceGraph,
  type EvidenceGraphEdge,
  type EvidenceGraphNode,
  type PackConsoleConfig,
  type PackFinding,
} from "../../api";
import type { Picture } from "../../graph/graphModel";
import { GraphCanvas } from "../../graph/GraphCanvas";
import { ClickableSubject } from "../../graph/ClickableSubject";
import { palette } from "../../theme";
import { readParam } from "../deepLink";
import { performAsAction } from "./apiAction";
import type { QueueConfig, QueueEntry } from "./queues";

const { Text, Title } = Typography;

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
  nearMissLabel: "Possible match — not linked",
  nearMissHint:
    "A near-identical candidate this rule found by value, not by a graph edge. No link exists between it and the finding's own subject — that absence is the finding.",
  candidatesLabel: "Might be the same record",
  candidatesHint:
    "Records this pack's own matching rules say are worth comparing with this one. A key collision is not a match — it is a reason to look, and the strategy that agreed is shown so you can weigh it.",
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
  edgesLabel: "Edges",
};

/** The local name of a term, for a reviewer who does not want to read IRIs.
 *
 *  The `:` cut is tried last, never first: an IRI's own scheme separator is a
 *  colon, and cutting there would turn every IRI into `//graph-owl.dev/…`.
 *  Curies (`gst:Section16`) are cut the same way a namespace-cut IRI is, so
 *  the Rule and Subject fields render the name a human decodes, not the term
 *  the registry emitted.
 *
 *  Falls back to the whole string rather than an empty one: a subject with no
 *  separator is unusual, but a blank row in a review queue is unusable. */
export function displayTerm(term: string): string {
  const slash = Math.max(term.lastIndexOf("#"), term.lastIndexOf("/"));
  const cut = slash >= 0 ? slash : term.lastIndexOf(":");
  const tail = cut >= 0 ? term.slice(cut + 1) : term;
  return tail.length > 0 ? tail : term;
}

/** The local name as a sentence a reviewer can scan — "SupplierNotFiled"
 *  becomes "Supplier Not Filed".
 *
 *  **A fallback, never the first choice.** A pack that declares
 *  `[findings.guidance]` owns what its findings are called, and that wording
 *  outranks anything this console could invent — see {@link titleFor}. This
 *  is what a pack without guidance renders instead of an identifier.
 *
 *  **An identifier, not prose.** The function refuses already-spaced input
 *  (a pack title reaching this path would otherwise get re-capitalised into
 *  the pack's voice), keeps acronyms together ("ITCNotAvailable" → "ITC Not
 *  Available"), and leaves a term with no camel transitions exactly alone.
 *  One console serves every pack, so a label this cannot parse must degrade
 *  to the raw name, never to an empty row. */
export function humanizeTerm(term: string): string {
  if (term === "") return term;
  if (term.includes(" ")) return term;
  // Two split rules, and both are insertions, not substitutions: between a
  // lower-case letter or digit and an upper-case one ("supplierNotFiled"),
  // and between an acronym run and the word after it ("ITCNotAvailable" —
  // the trailing `[A-Z][a-z]` guard is what keeps "ITC" intact). The
  // capture is only there to say where the space goes; both alternatives
  // replace themselves plus the space.
  const spaced = term.replace(
    /([a-z0-9])(?=[A-Z])|([A-Z])(?=[A-Z][a-z])/g,
    (_match, lower: string | undefined, acronym: string | undefined) =>
      `${acronym ?? lower} `,
  );
  return spaced
    .split(" ")
    .map((word) => (word.length > 0 ? word[0]!.toUpperCase() + word.slice(1) : word))
    .join(" ");
}

/** What a finding is called, for a reviewer — the pack's own declared title
 *  when it has one (`[findings.guidance]`, keyed by the finding's label),
 *  and the humanized local name when it does not.
 *
 *  **The pack owns the wording.** The GST pack calls `gst:SupplierNotFiled`
 *  "Supplier has not filed"; a hospitality pack calls its own findings
 *  whatever its manifest says. Hardcoding either here would be the first
 *  per-domain constant in a file whose whole purpose is domain neutrality
 *  (`plans/105-domain-neutrality.md`). The fallback exists only so a pack
 *  that declares no guidance still renders words rather than identifiers. */
export function titleFor(
  label: string,
  guidance: PackConsoleConfig["guidance"] | undefined,
): string {
  const declared = guidance?.[label]?.title;
  return declared ?? humanizeTerm(displayTerm(label));
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
 *  nodes need adapting, since a finding's subject is not a catalog asset and
 *  so has no `name`/`kind` the way `/assets/{id}/graph`'s nodes do. `kind`
 *  stays `null` always; `semanticType` (Plan 114 Slice F) carries what the
 *  pack's own `rdf:type` resolved to instead, so the canvas can still colour
 *  the node by something rather than drawing it uniformly grey.
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
      semanticType: node.semanticType,
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

/** The second candidate a rule's similarity band suspects is the same
 *  entity as this finding's own subject — Epic 105 P7's near-miss half
 *  (`plans/105g-evidence-provenance-and-near-miss.md`). `null` for every
 *  finding whose rule names none, which is most of them; a near-miss is
 *  specific to a rule suspecting two subjects are one entity and not yet
 *  linked. Named the same way every other node on this page is, via the
 *  shared `nodeLabel`. */
export function evidenceNearMiss(
  graph: EvidenceGraph,
): { id: string; iri: string | null; name: string; sources: readonly string[] } | null {
  if (!graph.nearMiss) return null;
  return {
    id: graph.nearMiss.id,
    // Plan 113 Slice C: what `ClickableSubject` resolves against —
    // `graph.nearMiss.id` is a bare local name and cannot be, on its own.
    iri: graph.nearMiss.iri,
    name: nodeLabel(graph.nearMiss),
    sources: graph.nearMiss.sources,
  };
}

/** Records the **pack's own blocking strategies** say are worth comparing
 *  with this finding's subject — Plan 111 Slice F.
 *
 *  **A different claim from a near miss, and it must read as one.** A near
 *  miss means the rule declared a similarity band and a value matched
 *  exactly; a candidate means a blocking key collided. The first is close to
 *  an assertion, the second an invitation to look — presenting them
 *  identically would flatten two strengths of evidence into one.
 *
 *  **An absent field reads as an empty list**, so a console older than its
 *  server and a pack that declares no blocking both render rather than fail:
 *  this section is additive to the panel, never a dependency of it.
 *
 *  **A candidate that cannot say why it matched is dropped.** The reason is
 *  the only thing that makes the row actionable; without it the row is a bare
 *  assertion that two records might be one. */
export function evidenceCandidates(
  graph: EvidenceGraph,
): {
  id: string;
  iri: string | null;
  name: string;
  sources: readonly string[];
  by: readonly string[];
}[] {
  return (graph.candidates ?? [])
    .filter((candidate) => candidate.by.length > 0)
    .map((candidate) => ({
      id: candidate.id,
      iri: candidate.iri,
      name: nodeLabel(candidate),
      sources: candidate.sources,
      by: candidate.by,
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

/** A pack's declared finding guidance, `undefined` when the pack declares
 *  none — `[findings.guidance]` is optional, and a pack without it is
 *  ordinary rather than broken (the console falls back to humanizing the
 *  label itself).
 *
 *  **Cached per pack, because two surfaces need it.** The list rows and the
 *  detail headline both read the pack's own wording, and a status-tab switch
 *  refetches entries but not guidance — the console has already fetched the
 *  whole console config once, so re-requesting it per finding would be
 *  paying the same round trip repeatedly for the same string. */
const guidanceCache = new Map<string, PackConsoleConfig["guidance"] | undefined>();

async function guidanceFor(pack: string): Promise<PackConsoleConfig["guidance"] | undefined> {
  const cached = guidanceCache.get(pack);
  if (cached !== undefined || guidanceCache.has(pack)) return cached;
  const config = await api.packConsole(pack);
  const guidance = config?.guidance;
  guidanceCache.set(pack, guidance);
  return guidance;
}

export function toQueueEntry(finding: PackFinding, title?: string): QueueEntry {
  return {
    id: finding.id,
    status: finding.status,
    summary: title ?? humanizeTerm(displayTerm(finding.label)),
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
      // The list rows are built from the pack's own wording, so the row
      // labels and the detail headline can never disagree about what a
      // finding is called. One request per distinct pack, then a cache —
      // a status-tab switch refetches entries, not guidance.
      const packs = [...new Set(findings.map((finding) => finding.pack))];
      const guidances = await Promise.all(packs.map((pack) => guidanceFor(pack)));
      const byPack = new Map(packs.map((pack, index) => [pack, guidances[index]]));
      return findings.map((finding) =>
        toQueueEntry(finding, titleFor(finding.label, byPack.get(finding.pack))),
      );
    },
    async fetchDetail(entry) {
      const finding = raw.get(entry.id);
      if (!finding) return null;
      const guidance = await guidanceFor(finding.pack);
      const title = titleFor(finding.label, guidance);
      // A missing traversal engine or an unresolvable subject must not take
      // down the rest of the detail pane — the flat evidence list above
      // already carries the finding's citation, and this section is an
      // addition to it, not a dependency of it.
      const graph = await api.findingEvidenceGraph(entry.id).catch(() => null);
      const nearMiss = graph ? evidenceNearMiss(graph) : null;
      // Plan 111 Slice F. Separate from the near miss above because the two are
      // different claims — see `evidenceCandidates`.
      const candidates = graph ? evidenceCandidates(graph) : [];
      const triples = graph ? evidenceTriples(graph) : [];
      const subjectLabel = displayTerm(finding.subject);
      return (
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          {/* The headline, in the pack's own words — what a reviewer scans
              for before deciding whether the citation below matters. */}
          <div>
            <Title level={3} style={{ margin: 0, fontSize: 18, fontWeight: 600 }}>
              {title}
            </Title>
            <Space size={8} style={{ marginTop: 6 }}>
              <Tag>{finding.pack}</Tag>
              <Text type="secondary" style={{ fontSize: 13 }}>
                {finding.summary}
              </Text>
            </Space>
          </div>

          {/* The citation as a definition block, not free text — a reviewer
              reads "Rule: Rule36-4 · Subject: 2b-INV-1001" the way a
              practitioner's file is laid out, and every term here is the
              local name a human decodes, never the raw IRI. */}
          <Descriptions
            size="small"
            column={1}
            items={[
              {
                key: "rule",
                label: COPY.governedByLabel,
                children: <Tag>{displayTerm(finding.governedBy)}</Tag>,
              },
              {
                key: "subject",
                label: COPY.subjectLabel,
                children: <Text code>{subjectLabel}</Text>,
              },
              {
                key: "evidence",
                label: COPY.evidenceLabel,
                children: (
                  <Space direction="vertical" size={2} style={{ width: "100%" }}>
                    {finding.evidence.map((fact, index) => (
                      <Space key={`${fact.predicate}-${index}`} size={6}>
                        <Tag>{displayTerm(fact.predicate)}</Tag>
                        <Text type="secondary">{fact.value}</Text>
                      </Space>
                    ))}
                  </Space>
                ),
              },
            ]}
          />

          {graph && (
            <>
              <Card size="small" title={COPY.evidenceGraphLabel}>
                {evidenceGraphIsJustTheSeed(graph) ? (
                  <Text type="secondary">{COPY.evidenceGraphSeedOnly}</Text>
                ) : (
                  <>
                    <GraphCanvas
                      picture={evidencePicture(finding, graph)}
                      colors={currentGraphColors()}
                      onExpand={() => {}}
                      label={`Evidence graph for ${subjectLabel}`}
                    />
                    {graph.truncated && (
                      <div style={{ marginTop: 8 }}>
                        <Text type="secondary">{COPY.evidenceGraphTruncated}</Text>
                      </div>
                    )}
                  </>
                )}
              </Card>

              {/* The edges and the provenance sit side by side — one answers
                  "what connects to what", the other "which document says so",
                  and a reviewer weighing the graph needs both at once. */}
              <Flex wrap gap={16}>
                {/* The canvas's own `role="img"`/`aria-label` names the
                    picture but not its content — this prose list is the same
                    non-visual equivalent `00f` requires of any rendered
                    graph, not a duplicate of the canvas above it. */}
                {graph.edges.length > 0 && (
                  <div style={{ flex: "1 1 280px", minWidth: 240 }}>
                    <Card size="small" title={COPY.edgesLabel} style={{ height: "100%" }}>
                      {graph.edges.map((edge, index) => (
                        <div
                          key={`${edge.from}-${edge.relationship}-${edge.to}-${index}`}
                          style={{ marginTop: 4 }}
                        >
                          <Text code style={{ fontSize: 12, wordBreak: "break-all" }}>
                            {describeEvidenceEdge(edge)}
                          </Text>
                        </div>
                      ))}
                    </Card>
                  </div>
                )}
                {/* One node per row, name then the documents that assert it —
                    the same row a near miss and a candidate use below, so the
                    provenance of anything named on this page reads alike.
                    Always shown: even a lone, unresolvable seed has a source,
                    and that provenance is part of the finding. */}
                <div style={{ flex: "1 1 280px", minWidth: 240 }}>
                  <Card size="small" title={COPY.sourcesLabel} style={{ height: "100%" }}>
                    {evidenceNodeSources(graph).map((row) => (
                      <div key={row.id} style={{ marginTop: 4 }}>
                        <Space wrap size={[4, 4]}>
                          <Text>{row.name}</Text>
                          {row.sources.length > 0 ? (
                            row.sources.map((source) => <Tag key={source}>{source}</Tag>)
                          ) : (
                            <Text type="secondary">{COPY.sourcesEmpty}</Text>
                          )}
                        </Space>
                      </div>
                    ))}
                  </Card>
                </div>
              </Flex>

              {nearMiss && (
                <Card size="small" title={COPY.nearMissLabel}>
                  <div>
                    <Text type="secondary">{COPY.nearMissHint}</Text>
                  </div>
                  <div style={{ marginTop: 8 }}>
                    <Space wrap size={[4, 4]}>
                      {nearMiss.iri ? (
                        <ClickableSubject seed={nearMiss.iri} label={nearMiss.name} />
                      ) : (
                        <Text>{nearMiss.name}</Text>
                      )}
                      {nearMiss.sources.length > 0 ? (
                        nearMiss.sources.map((source) => <Tag key={source}>{source}</Tag>)
                      ) : (
                        <Text type="secondary">{COPY.sourcesEmpty}</Text>
                      )}
                    </Space>
                  </div>
                </Card>
              )}

              {candidates.length > 0 && (
                <Card size="small" title={COPY.candidatesLabel}>
                  <div>
                    <Text type="secondary">{COPY.candidatesHint}</Text>
                  </div>
                  {candidates.map((candidate) => (
                    <div key={candidate.id} style={{ marginTop: 8 }}>
                      <Space wrap size={[4, 4]}>
                        {candidate.iri ? (
                          <ClickableSubject seed={candidate.iri} label={candidate.name} />
                        ) : (
                          <Text>{candidate.name}</Text>
                        )}
                        {/* Which strategy agreed, first: it is what tells
                            a reviewer how much weight the row carries,
                            and burying it after the provenance would make
                            every candidate read alike. */}
                        {candidate.by.map((strategy) => (
                          <Tag key={strategy} color="processing">
                            {strategy}
                          </Tag>
                        ))}
                        {candidate.sources.map((source) => (
                          <Tag key={source}>{source}</Tag>
                        ))}
                      </Space>
                    </div>
                  ))}
                </Card>
              )}

              {triples.length > 0 && (
                <Card size="small" title={COPY.triplesLabel}>
                  <Table
                    size="small"
                    pagination={false}
                    rowKey="key"
                    dataSource={triples.map((row, index) => ({
                      key: `${row.subject}—${row.predicate}—${row.object}—${index}`,
                      ...row,
                    }))}
                    columns={[
                      { title: COPY.subjectLabel, dataIndex: "subject", key: "subject" },
                      {
                        title: COPY.predicateLabel,
                        dataIndex: "predicate",
                        key: "predicate",
                        render: (value: string, row: { derived: boolean }) => (
                          <Space size={6}>
                            {value}
                            {row.derived && <Tag>{COPY.derivedTag}</Tag>}
                          </Space>
                        ),
                      },
                      { title: COPY.objectLabel, dataIndex: "object", key: "object" },
                    ]}
                  />
                </Card>
              )}
            </>
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
