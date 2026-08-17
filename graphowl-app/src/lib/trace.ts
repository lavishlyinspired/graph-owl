/** The shape every TRACE screen (Lineage, Paths, History, Evidence) reduces
 *  to — Plan 122a A4. "Four destinations from one pattern in four
 *  configurations": each screen's real API response is turned into this one
 *  config by a pure function below, and `routes/*.tsx` render it through the
 *  single `<TraceDetail>` component. Kept pure and separate from the
 *  component on purpose — `00f` requires graph/trace tests to assert the
 *  model (which KPI, which row, which relationship survived) rather than
 *  the picture, and a config object is what makes that possible without
 *  rendering anything. */

import type { AssetVersion, Finding, FoundPath, LineageEdge, LineageGraph, LineageNode } from "./api";
import { relativeTime } from "./format";
import type { RouteName } from "./routes";
import { strings } from "./strings";

export interface TraceKpi {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
}

export interface TraceCell {
  readonly text: string;
  readonly sub?: string;
  readonly mono?: boolean;
}

export interface TraceRow {
  readonly key: string;
  readonly cells: readonly TraceCell[];
}

export interface TraceRelated {
  readonly label: string;
  readonly route: RouteName;
  readonly id?: string;
}

/** One link in the visual upstream/downstream breadcrumb on the Lineage
 *  screen (Plan 122a mockup) — `sub` is the asset's real `AssetKind`, never
 *  a fabricated description, since the API has no "layer" or "change risk"
 *  concept to draw one from. */
export interface TraceChainNode {
  readonly id: string;
  readonly label: string;
  readonly sub: string;
  readonly current: boolean;
}

export interface TraceChain {
  readonly upstream: readonly TraceChainNode[];
  readonly downstream: readonly TraceChainNode[];
}

export interface TraceConfig {
  readonly title: string;
  readonly description: string;
  readonly kpis: readonly TraceKpi[];
  readonly columns: readonly string[];
  readonly rows: readonly TraceRow[];
  readonly emptyMessage: string;
  readonly noteTitle: string;
  readonly noteBody: string;
  readonly related: readonly TraceRelated[];
  /** Only Lineage sets this — the other three TRACE screens have no
   *  directional chain to walk, so `TraceDetail` renders the breadcrumb
   *  section only when it is present. */
  readonly chain?: TraceChain;
}

const SOURCE_LABEL: Record<LineageGraph["edges"][number]["source"], string> = {
  manual: "manual",
  connector: "connector",
  openlineage: "openlineage",
  agent: "agent",
};

/** The single longest simple path reachable from `rootId` by repeatedly
 *  following whichever edges anchor on the current node — walks every
 *  branch (guarding against cycles via `path.includes`) and keeps the
 *  deepest one, rather than stopping at the first edge found. Direction is
 *  supplied by the caller: `anchorOf`/`nextOf` swapped gives upstream vs
 *  downstream from the same function. */
function longestChain(params: {
  readonly rootId: string;
  readonly edges: readonly LineageEdge[];
  readonly anchorOf: (edge: LineageEdge) => string;
  readonly nextOf: (edge: LineageEdge) => string;
}): readonly string[] {
  const { rootId, edges, anchorOf, nextOf } = params;
  const byAnchor = new Map<string, LineageEdge[]>();
  for (const edge of edges) {
    const key = anchorOf(edge);
    const list = byAnchor.get(key) ?? [];
    list.push(edge);
    byAnchor.set(key, list);
  }

  let longest: readonly string[] = [rootId];
  const walk = (current: string, path: readonly string[]) => {
    if (path.length > longest.length) longest = path;
    for (const edge of byAnchor.get(current) ?? []) {
      const next = nextOf(edge);
      if (path.includes(next)) continue;
      walk(next, [...path, next]);
    }
  };
  walk(rootId, [rootId]);
  return longest;
}

function toChainNode(id: string, nodesById: Map<string, LineageNode>, rootId: string): TraceChainNode {
  const node = nodesById.get(id);
  return {
    id,
    label: node?.name ?? id,
    sub: node?.kind ?? "—",
    current: id === rootId,
  };
}

export function toLineageConfig(graph: LineageGraph, rootName: string): TraceConfig {
  const byId = new Map(graph.nodes.map((node) => [node.id, node.name]));
  const nodesById = new Map(graph.nodes.map((node) => [node.id, node]));

  const upstreamIds = [
    ...longestChain({
      rootId: graph.rootId,
      edges: graph.edges,
      anchorOf: (edge) => edge.toAssetId,
      nextOf: (edge) => edge.fromAssetId,
    }),
  ].reverse();
  const downstreamIds = longestChain({
    rootId: graph.rootId,
    edges: graph.edges,
    anchorOf: (edge) => edge.fromAssetId,
    nextOf: (edge) => edge.toAssetId,
  });
  const chain: TraceChain = {
    upstream: upstreamIds.map((id) => toChainNode(id, nodesById, graph.rootId)),
    downstream: downstreamIds.map((id) => toChainNode(id, nodesById, graph.rootId)),
  };

  return {
    title: strings.lineageTitle,
    description: strings.lineageDescription,
    kpis: [
      { label: strings.lineageKpiFocus, value: rootName },
      { label: strings.lineageKpiUpstream, value: String(upstreamIds.length - 1) },
      { label: strings.lineageKpiDownstream, value: String(downstreamIds.length - 1) },
      {
        label: strings.lineageKpiTruncated,
        value: graph.truncated ? strings.lineageTruncatedYes : strings.lineageTruncatedNo,
      },
    ],
    columns: [
      strings.lineageColRelationship,
      strings.lineageColFrom,
      strings.lineageColTo,
      strings.lineageColSource,
      strings.lineageColWhen,
    ],
    rows: graph.edges.map((edge) => ({
      key: edge.id,
      cells: [
        { text: edge.relationship, mono: true },
        { text: byId.get(edge.fromAssetId) ?? edge.fromAssetId },
        { text: byId.get(edge.toAssetId) ?? edge.toAssetId },
        { text: SOURCE_LABEL[edge.source], mono: true },
        { text: relativeTime(edge.createdAt, new Date()), sub: edge.createdBy },
      ],
    })),
    emptyMessage: strings.lineageEmpty,
    noteTitle: strings.lineageNoteTitle,
    noteBody: strings.lineageNoteBody,
    related: [
      { label: strings.traceRelatedExplore, route: "explore", id: graph.rootId },
      { label: strings.traceRelatedEntity, route: "entity", id: graph.rootId },
    ],
    chain,
  };
}

export function toPathsConfig(result: PathSearchResultLike, from: string, to: string): TraceConfig {
  return {
    title: strings.pathsTitle,
    description: strings.pathsDescription,
    kpis: [
      { label: strings.pathsKpiFrom, value: from },
      { label: strings.pathsKpiTo, value: to },
      { label: strings.pathsKpiFound, value: String(result.paths.length) },
      {
        label: strings.pathsKpiTruncated,
        value: result.truncated ? strings.lineageTruncatedYes : strings.lineageTruncatedNo,
      },
    ],
    columns: [strings.pathsColPath, strings.pathsColRoute, strings.pathsColHops],
    rows: result.paths.map((path, index) => ({
      key: `${index}`,
      cells: [
        { text: String(index + 1), mono: true },
        { text: path.nodes.join(" → "), mono: true },
        { text: String(path.length) },
      ],
    })),
    emptyMessage: strings.pathsEmpty,
    noteTitle: strings.pathsNoteTitle,
    noteBody: strings.pathsNoteBody,
    related: [
      { label: strings.traceRelatedExplore, route: "explore", id: from },
    ],
  };
}

interface PathSearchResultLike {
  readonly paths: readonly FoundPath[];
  readonly truncated: boolean;
}

export function toHistoryConfig(versions: readonly AssetVersion[], name: string): TraceConfig {
  return {
    title: strings.historyTitle,
    description: strings.historyDescription,
    kpis: [
      { label: strings.historyKpiEntity, value: name },
      { label: strings.historyKpiVersions, value: String(versions.length) },
      {
        label: strings.historyKpiLatest,
        value: versions[0] ? `${versions[0].version.major}.${versions[0].version.minor}` : "—",
      },
      {
        label: strings.historyKpiUpdatedBy,
        value: versions[0]?.updatedBy ?? "—",
      },
    ],
    columns: [strings.historyColVersion, strings.historyColChange, strings.historyColWho, strings.historyColWhen],
    rows: versions.map((version) => ({
      key: `${version.version.major}.${version.version.minor}-${version.updatedAt}`,
      cells: [
        { text: `${version.version.major}.${version.version.minor}`, mono: true },
        { text: version.changeDescription?.summary ?? strings.historyNoSummary },
        { text: version.updatedBy },
        { text: relativeTime(version.updatedAt, new Date()) },
      ],
    })),
    emptyMessage: strings.historyEmpty,
    noteTitle: strings.historyNoteTitle,
    noteBody: strings.historyNoteBody,
    related: [{ label: strings.traceRelatedEntity, route: "entity" }],
  };
}

export function toEvidenceConfig(findings: readonly Finding[]): TraceConfig {
  const rows: TraceRow[] = findings.flatMap((finding) =>
    finding.evidence.map((evidence, index) => ({
      key: `${finding.id}-${index}`,
      cells: [
        { text: `${evidence.subject} ${evidence.predicate} ${evidence.value}`, mono: true },
        { text: finding.pack },
        { text: evidence.var ?? "—", mono: true },
        { text: finding.status },
      ],
    })),
  );

  return {
    title: strings.evidenceTitle,
    description: strings.evidenceDescription,
    kpis: [
      { label: strings.evidenceKpiFindings, value: String(findings.length) },
      { label: strings.evidenceKpiEntries, value: String(rows.length) },
      {
        label: strings.evidenceKpiPending,
        value: String(findings.filter((finding) => finding.status === "pending").length),
      },
      {
        label: strings.evidenceKpiPacks,
        value: String(new Set(findings.map((finding) => finding.pack)).size),
      },
    ],
    columns: [strings.evidenceColFact, strings.evidenceColSource, strings.evidenceColField, strings.evidenceColStatus],
    rows,
    emptyMessage: strings.evidenceEmpty,
    noteTitle: strings.evidenceNoteTitle,
    noteBody: strings.evidenceNoteBody,
    related: [{ label: strings.traceRelatedContradictions, route: "contradictions" }],
  };
}
