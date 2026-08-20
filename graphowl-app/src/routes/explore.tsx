import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { GraphCanvas, type CanvasSelection } from "../graph/GraphCanvas";
import { ConnectivityPanel } from "../graph/ConnectivityPanel";
import { EntityPanel } from "./EntityPanel";
import { GRAPH_COLORS } from "../lib/graph/graphColors";
import { edgeId as toEdgeId, visiblePicture, type Picture } from "../lib/graph/graphModel";
import { expand, seed, type GraphModel } from "../lib/graph/model";
import { findingsFor, mergeEntityOptions, seedsFromFindings, type ExploreSeed } from "../lib/exploreSeeds";
import { allEntitiesQuery, entitiesFromSparqlRows, type EntitySummary } from "../lib/graph/entityList";
import { fetchFindings, fetchInstalledPacks, runSparql, type Finding } from "../lib/api";
import { knownKinds, filterParam } from "../lib/graph/edgeFilter";
import { looksLikeAssetId, openTargetFor } from "../lib/graph/graphContext";
import { SubjectFindings } from "../components/SubjectFindings";
import {
  fetchAssetGraph,
  fetchGraphContext,
  fetchGraphContextAnalytics,
  pinToInvestigation,
  type GraphEdge,
  type GraphNode,
  type GraphView,
} from "../lib/api";
import { resolveInitialTheme } from "../lib/theme";
import { strings } from "../lib/strings";

type ExploreView = "graph" | "entity";

function toPicture(model: GraphModel): Picture {
  return {
    seedId: model.seedId,
    nodes: model.nodes,
    edges: model.edges,
    expanded: model.expanded,
    truncatedAt: model.truncatedAt,
  };
}

/** A seed comes from two different places with two different id spaces:
 *  `entity.tsx` links here with a catalog asset's own UUID, and the seed
 *  picker below links here with a finding's *subject* — most often a bare
 *  graph IRI with no `assets` row behind it at all (a GST invoice, say).
 *  `/assets/{id}/graph` only ever accepts the former; `/graph/context`
 *  accepts both, so only the id shape decides which one gets called. */
function fetchNeighbourhood(
  id: string,
  options: { readonly hops: number; readonly relationshipTypes?: readonly string[] },
): Promise<GraphView> {
  return looksLikeAssetId(id) ? fetchAssetGraph(id, options) : fetchGraphContext(id, options);
}

export default function ExploreRoute() {
  const { id } = useParams<{ id?: string }>();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  // Read once at mount rather than kept live: this route has no shared
  // theme context (`AppShell` owns that state and does not thread it
  // through `<Outlet />`), and G6 draws to canvas, which cannot react to a
  // CSS custom property changing after paint anyway — a canvas mounted
  // before a mid-session theme toggle keeps its colours from the moment it
  // opened, same as any other canvas-backed view would.
  const mode = useMemo(() => resolveInitialTheme(), []);
  const colors = GRAPH_COLORS[mode];

  const [model, setModel] = useState<GraphModel | null>(null);
  const [error, setError] = useState(false);
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<GraphEdge | null>(null);
  const [pinStatus, setPinStatus] = useState<"idle" | "pinning" | "done" | "failed">("idle");
  const [availableKinds, setAvailableKinds] = useState<readonly string[]>([]);
  // The dropdown's options. Loaded once for the screen rather than per
  // selection: the list is what a reader picks *from*, so it has to survive
  // the selection changing under it.
  const [seeds, setSeeds] = useState<readonly ExploreSeed[] | null>(null);
  // Every real entity, across every installed pack — merged with `seeds`'
  // real finding counts rather than replacing it. `null` until the first
  // pack's own scan resolves, same "loading vs empty" distinction `seeds`
  // itself already draws.
  const [allEntities, setAllEntities] = useState<readonly EntitySummary[] | null>(null);
  // The findings themselves, not just the subjects distilled out of them —
  // the detail panel needs each rule's bindings, which `ExploreSeed` drops.
  const [findings, setFindings] = useState<readonly Finding[]>([]);
  // `openTargetFor`'s `?view=entity` is a one-shot landing instruction,
  // re-read whenever `id` changes (below) rather than kept in sync with
  // the URL continuously — switching tabs by hand does not rewrite it,
  // matching every other tab bar in this console.
  const [activeView, setActiveView] = useState<ExploreView>(() =>
    searchParams.get("view") === "entity" ? "entity" : "graph",
  );
  // Nodes the reader hid from the picture. Kept per selected entity — a fresh
  // seed is a fresh question, and carrying hidden nodes across would silently
  // omit things from a neighbourhood nobody hid anything in.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [selectedKinds, setSelectedKinds] = useState<readonly string[]>([]);

  useEffect(() => {
    setModel(null);
    setError(false);
    setSelectedNode(null);
    setSelectedEdge(null);
    setPinStatus("idle");
    setHidden(new Set());
    setActiveView(searchParams.get("view") === "entity" ? "entity" : "graph");
    if (!id) return;
    fetchNeighbourhood(id, { hops: 1, relationshipTypes: filterParam(selectedKinds) })
      .then((view) => {
        setModel(seed(id, view));
        setAvailableKinds((seen) => knownKinds(seen, view.edges.map((edge) => edge.relationship)));
      })
      .catch(() => setError(true));
    // `searchParams` deliberately not listed: re-reading `?view=` is meant
    // to fire when the reader navigates to a *different* entity, not on
    // every keystroke of a hand-edited URL.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, selectedKinds]);

  useEffect(() => {
    let cancelled = false;
    fetchFindings()
      .then((all) => {
        if (cancelled) return;
        setFindings(all);
        setSeeds(seedsFromFindings(all));
      })
      .catch(() => {
        if (!cancelled) setSeeds([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchInstalledPacks()
      .then((packs) =>
        Promise.all(packs.map((pack) => runSparql(allEntitiesQuery(pack.packId)).then((r) => r.rows))),
      )
      .then((rowSets) => {
        if (cancelled) return;
        setAllEntities(entitiesFromSparqlRows(rowSets.flat()));
      })
      .catch(() => {
        if (!cancelled) setAllEntities([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const entityOptions = useMemo(
    () => (allEntities === null ? seeds : mergeEntityOptions(allEntities, seeds ?? [])),
    [allEntities, seeds],
  );

  // **Open on something rather than on nothing.** Explore's whole purpose is
  // the canvas, and an empty one asks the reader to make a choice before they
  // have seen anything to base it on. `replace` so the redirect does not
  // become a history entry the back button gets stuck on.
  useEffect(() => {
    if (id || entityOptions === null || entityOptions.length === 0) return;
    const first = entityOptions[0];
    if (first) navigate(`/explore/${encodeURIComponent(first.id)}`, { replace: true });
  }, [id, entityOptions, navigate]);

  const handleHide = useCallback((nodeId: string) => {
    setHidden((current) => new Set(current).add(nodeId));
  }, []);

  const handleExpand = useCallback(
    (nodeId: string) => {
      fetchNeighbourhood(nodeId, { hops: 1, relationshipTypes: filterParam(selectedKinds) })
        .then((view) => {
          setModel((current) => (current ? expand(current, nodeId, view) : current));
          setAvailableKinds((seen) => knownKinds(seen, view.edges.map((edge) => edge.relationship)));
        })
        .catch(() => setError(true));
    },
    [selectedKinds],
  );

  const toggleKind = (kind: string) => {
    setSelectedKinds((current) =>
      current.includes(kind) ? current.filter((k) => k !== kind) : [...current, kind],
    );
  };

  const handleSelect = (selection: CanvasSelection) => {
    if (selection.kind === "node") {
      setSelectedNode(selection.node);
      setSelectedEdge(null);
    } else if (selection.kind === "edge") {
      const found = model?.edges.find((edge) => toEdgeId(edge) === selection.edgeId) ?? null;
      setSelectedEdge(found);
      setSelectedNode(null);
    } else {
      setSelectedNode(null);
      setSelectedEdge(null);
    }
    setPinStatus("idle");
  };

  const handlePin = async () => {
    if (!id || !selectedNode) return;
    setPinStatus("pinning");
    try {
      await pinToInvestigation(id, `Pinned from Explorer: ${selectedNode.name}`);
      setPinStatus("done");
    } catch {
      setPinStatus("failed");
    }
  };

  const picture = model ? toPicture(model) : null;
  const endpointNode = (nodeId: string) => model?.nodes.find((n) => n.id === nodeId);
  const endpointName = (nodeId: string) => endpointNode(nodeId)?.name ?? nodeId;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-[46px] flex-none items-center gap-2.5 border-b border-gowl-line bg-gowl-panel px-4">
        <span className="text-[17px] font-semibold text-gowl-t1">{strings.exploreTitle}</span>
        <span className="text-gowl-dim">{strings.exploreBreadcrumbSeparator}</span>
        <EntitySelect
          seeds={entityOptions}
          selected={id ?? ""}
          onSelect={(next) => navigate(next === "" ? "/explore" : `/explore/${encodeURIComponent(next)}`)}
        />
        {model?.truncated && activeView === "graph" && (
          <span className="ml-4 text-[15px] text-gowl-amber">{strings.exploreTruncated}</span>
        )}
        <div className="flex gap-1 rounded-md border border-gowl-line-2 p-0.5">
          {(["graph", "entity"] as const).map((v) => (
            <button
              key={v}
              type="button"
              onClick={() => setActiveView(v)}
              className={`rounded px-2.5 py-1 text-[15.5px] ${
                activeView === v ? "bg-gowl-accent text-gowl-bg" : "text-gowl-t5 hover:text-gowl-t2"
              }`}
            >
              {v === "graph" ? strings.exploreViewGraph : strings.exploreViewEntity}
            </button>
          ))}
        </div>
        {activeView === "graph" && (
          <div className="ml-auto flex items-center gap-1.5">
            {availableKinds.map((kind) => {
              const active = selectedKinds.includes(kind);
              return (
                <button
                  key={kind}
                  type="button"
                  onClick={() => toggleKind(kind)}
                  className={
                    active
                      ? "rounded-md border border-gowl-accent-border bg-gowl-accent-bg px-2.5 py-1 text-[15.5px] text-gowl-accent"
                      : "rounded-md border border-gowl-line-2 px-2.5 py-1 text-[15.5px] text-gowl-t4 hover:border-gowl-hover"
                  }
                >
                  {kind}
                </button>
              );
            })}
            {selectedKinds.length > 0 && (
              <button
                type="button"
                onClick={() => setSelectedKinds([])}
                className="text-[15px] text-gowl-t6 hover:text-gowl-t4"
              >
                {strings.exploreClearFilter}
              </button>
            )}
          </div>
        )}
      </div>

      {!id && (
        <div className="flex flex-1 items-center justify-center p-8 text-[17px] text-gowl-t5">
          {strings.exploreNoSeed}
        </div>
      )}

      {id && activeView === "entity" && <EntityPanel id={id} />}

      {id && activeView === "graph" && error && (
        <div className="p-8 text-[17px] text-gowl-bad">{strings.exploreError}</div>
      )}
      {id && activeView === "graph" && !error && !model && (
        <div className="p-8 text-[17px] text-gowl-t5">{strings.exploreLoading}</div>
      )}

      {id && activeView === "graph" && !error && model && picture && (
      <div className="flex min-h-0 flex-1">
        <GraphCanvas
          picture={visiblePicture(picture, hidden)}
          colors={colors}
          mode={mode}
          onExpand={handleExpand}
          onHide={handleHide}
          onOpen={(id) => navigate(openTargetFor(id))}
          onSelect={handleSelect}
          label={`Neighbourhood of ${model.seedId}`}
        />

        {(selectedNode ?? selectedEdge) && (
          <div className="w-[352px] flex-none overflow-auto border-l border-gowl-line bg-gowl-panel p-4">
            {selectedNode && (
              <>
                <div className="mb-2 flex items-center gap-2">
                  <span className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-accent">
                    {selectedNode.semanticType ?? strings.exploreEntityBadge}
                  </span>
                </div>
                <div className="mb-1 text-[21px] font-semibold text-gowl-t1">{selectedNode.name}</div>
                <div className="mb-4 break-all font-mono text-[15px] text-gowl-t6">{selectedNode.id}</div>
                <div className="space-y-2 text-[16px]">
                  <div className="flex justify-between">
                    <span className="text-gowl-t5">{strings.exploreKind}</span>
                    <span className="text-gowl-t2">{selectedNode.kind ?? strings.exploreKindHidden}</span>
                  </div>
                  {selectedNode.fullyQualifiedName && (
                    <div>
                      <div className="mb-1 text-gowl-t5">{strings.exploreFqn}</div>
                      <div className="break-all font-mono text-[15px] text-gowl-t3">
                        {selectedNode.fullyQualifiedName}
                      </div>
                    </div>
                  )}
                </div>
                <div className="mt-5">
                  <ProvenanceBlock
                    endpoints={[{ role: strings.exploreSeenIn, node: selectedNode }]}
                  />
                  <SubjectFindings
                    findings={findingsFor(findings, selectedNode.id)}
                    semanticType={selectedNode.semanticType ?? undefined}
                  />
                </div>
                <div className="mt-5">
                  <ConnectivityPanel
                    cacheKey={selectedNode.id}
                    load={() => fetchGraphContextAnalytics({ seed: selectedNode.id })}
                    names={new Map(picture?.nodes.map((node) => [node.id, node.name]) ?? [])}
                  />
                </div>
                <div className="mt-4 flex flex-col gap-2">
                  <Link
                    to={openTargetFor(selectedNode.id)}
                    className="rounded-md border border-gowl-line-2 px-3 py-2 text-center text-[16px] text-gowl-t3 hover:border-gowl-hover"
                  >
                    {strings.exploreOpenEntity}
                  </Link>
                  <button
                    type="button"
                    onClick={() => void handlePin()}
                    disabled={pinStatus === "pinning"}
                    className="rounded-md bg-gowl-accent px-3 py-2 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-60"
                  >
                    {strings.explorePinToInvestigation}
                  </button>
                  {pinStatus === "done" && (
                    <div className="text-[15px] text-gowl-ok">{strings.explorePinned}</div>
                  )}
                  {pinStatus === "failed" && (
                    <div className="text-[15px] text-gowl-bad">{strings.explorePinFailed}</div>
                  )}
                </div>
              </>
            )}
            {selectedEdge && (
              <>
                <div className="mb-3 flex items-center gap-2">
                  <span className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-accent">
                    {strings.exploreRelationshipBadge}
                  </span>
                  <span className={`rounded border px-1.5 py-0.5 font-mono text-[13.5px] ${
                    selectedEdge.derived
                      ? "border-gowl-amber-border bg-gowl-amber-bg text-gowl-amber"
                      : "border-gowl-line-2 bg-gowl-panel-2 text-gowl-t5"
                  }`}>
                    {selectedEdge.derived ? strings.exploreEdgeInferred : strings.exploreEdgeAsserted}
                  </span>
                </div>
                <div className="mb-1 text-[21px] font-semibold text-gowl-t1">{selectedEdge.relationship}</div>
                <div className="mb-4 font-mono text-[15px] text-gowl-t6">
                  {`${endpointName(selectedEdge.from)} → ${endpointName(selectedEdge.to)}`}
                </div>

                {/* Tabs */}
                <div className="mb-4 flex gap-1 border-b border-gowl-line">
                  {[strings.exploreDetailTab, strings.exploreEvidenceTab, strings.exploreProvenanceTab, strings.exploreTimeTab].map((tab, i) => (
                    <button
                      key={tab}
                      type="button"
                      className={`px-2.5 py-1.5 text-[15px] ${
                        i === 0 ? "border-b-2 border-gowl-accent text-gowl-accent" : "text-gowl-t5 hover:text-gowl-t2"
                      }`}
                    >
                      {tab}
                    </button>
                  ))}
                </div>

                <ProvenanceBlock
                  endpoints={[
                    { role: strings.exploreEndpointFrom, node: endpointNode(selectedEdge.from) },
                    { role: strings.exploreEndpointTo, node: endpointNode(selectedEdge.to) },
                  ]}
                />
                <SubjectFindings
                  findings={findingsFor(findings, selectedEdge.from)}
                  semanticType={endpointNode(selectedEdge.from)?.semanticType ?? undefined}
                />

                {/* Action buttons */}
                <div className="flex flex-col gap-2">
                  <Link
                    to={`/paths?from=${encodeURIComponent(selectedEdge.from)}&to=${encodeURIComponent(model.seedId)}`}
                    className="rounded-md bg-gowl-accent px-3 py-2 text-center text-[16px] font-semibold text-gowl-accent-on"
                  >
                    {strings.exploreTracePathAction}
                  </Link>
                  <div className="flex gap-2">
                    <Link
                      to={`/lineage-view/${encodeURIComponent(selectedEdge.from)}`}
                      className="flex-1 rounded-md border border-gowl-line-2 px-3 py-2 text-center text-[16px] text-gowl-t3 hover:border-gowl-hover"
                    >
                      {strings.exploreLineageAction}
                    </Link>
                    <Link
                      to={`/history/${encodeURIComponent(selectedEdge.from)}`}
                      className="flex-1 rounded-md border border-gowl-line-2 px-3 py-2 text-center text-[16px] text-gowl-t3 hover:border-gowl-hover"
                    >
                      {strings.exploreHistoryAction}
                    </Link>
                  </div>
                </div>
              </>
            )}
          </div>
        )}
      </div>
      )}
    </div>
  );
}


/** Where each endpoint of a relationship was seen — the named import graphs
 *  the graph API returns per subject, which is the only provenance it
 *  actually carries. */
function ProvenanceBlock({
  endpoints,
}: {
  readonly endpoints: readonly { readonly role: string; readonly node: GraphNode | undefined }[];
}) {
  return (
    <div className="mb-4">
      <div className="mb-2.5 font-mono text-[13.5px] tracking-[0.14em] text-gowl-t6">
        {strings.exploreProvenanceTitle}
      </div>
      <div className="space-y-2">
        {endpoints.map(({ role, node }) => (
          <div key={role} className="rounded-md border border-gowl-line-2 bg-gowl-panel-2 px-3 py-2">
            <div className="flex items-baseline justify-between gap-2">
              <span className="font-mono text-[13.5px] tracking-[0.14em] text-gowl-t6">{role}</span>
              {node?.semanticType && (
                <span className="font-mono text-[14px] text-gowl-accent">{node.semanticType}</span>
              )}
            </div>
            <div className="mt-1 text-[16px] text-gowl-t2">
              {node?.name ?? strings.exploreUnknownEndpoint}
            </div>
            {node?.sources && node.sources.length > 0 ? (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {node.sources.map((source) => (
                  <span
                    key={source}
                    className="rounded border border-gowl-line-2 bg-gowl-row px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-t4"
                  >
                    {source}
                  </span>
                ))}
              </div>
            ) : (
              <div className="mt-1 text-[14.5px] text-gowl-t7">{strings.exploreNoSources}</div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

/** The entity picker — a dropdown in the toolbar, not a screen of its own.
 *
 *  Explore used to be two screens: `/explore` listed flagged subjects and
 *  `/explore/:id` drew the graph, so choosing what to look at and looking at
 *  it never happened in the same place, and changing subject meant going
 *  back. The picker lives beside the canvas instead, so the graph is one
 *  selection away at all times and the URL stays deep-linkable.
 *
 *  Seeded from **findings** rather than recent subjects: a finding is a
 *  subject somebody has a reason to look at, where recency says only that
 *  something was written. */
function EntitySelect({
  seeds,
  selected,
  onSelect,
}: {
  readonly seeds: readonly ExploreSeed[] | null;
  readonly selected: string;
  readonly onSelect: (id: string) => void;
}) {
  // A subject reached from elsewhere (`entity.tsx` links straight in) need
  // not be one of the flagged ones, and a `<select>` silently shows a blank
  // value for an option it does not have — which reads as "nothing
  // selected" over a graph that is plainly of something. Carry it as its
  // own option so the control always names what is on screen.
  const known = (seeds ?? []).some((s) => s.id === selected);
  return (
    <select
      aria-label={strings.exploreEntityPickerLabel}
      value={selected}
      onChange={(event) => onSelect(event.target.value)}
      className="max-w-[560px] flex-1 truncate rounded-md border border-gowl-line-2 bg-gowl-panel-2 px-2.5 py-1 font-mono text-[15.5px] text-gowl-accent hover:border-gowl-hover focus:border-gowl-accent focus:outline-none"
    >
      <option value="">
        {seeds === null ? strings.exploreLoading : strings.exploreEntityPickerPlaceholder}
      </option>
      {selected !== "" && !known && <option value={selected}>{selected}</option>}
      {(seeds ?? []).map((s) => (
        <option key={s.id} value={s.id}>
          {`${s.id}  —  ${s.label} · ${s.findings} finding${s.findings === 1 ? "" : "s"}`}
        </option>
      ))}
    </select>
  );
}
