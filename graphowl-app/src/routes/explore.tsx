import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { GraphCanvas, type CanvasSelection } from "../graph/GraphCanvas";
import { GRAPH_COLORS } from "../lib/graph/graphColors";
import { edgeId as toEdgeId, type Picture } from "../lib/graph/graphModel";
import { expand, seed, type GraphModel } from "../lib/graph/model";
import { knownKinds, filterParam } from "../lib/graph/edgeFilter";
import { fetchAssetGraph, pinToInvestigation, type GraphEdge, type GraphNode } from "../lib/api";
import { resolveInitialTheme } from "../lib/theme";
import { strings } from "../lib/strings";

function toPicture(model: GraphModel): Picture {
  return {
    seedId: model.seedId,
    nodes: model.nodes,
    edges: model.edges,
    expanded: model.expanded,
    truncatedAt: model.truncatedAt,
  };
}

export default function ExploreRoute() {
  const { id } = useParams<{ id?: string }>();
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
  const [selectedKinds, setSelectedKinds] = useState<readonly string[]>([]);

  useEffect(() => {
    setModel(null);
    setError(false);
    setSelectedNode(null);
    setSelectedEdge(null);
    setPinStatus("idle");
    if (!id) return;
    fetchAssetGraph(id, { hops: 1, relationshipTypes: filterParam(selectedKinds) })
      .then((view) => {
        setModel(seed(id, view));
        setAvailableKinds((seen) => knownKinds(seen, view.edges.map((edge) => edge.relationship)));
      })
      .catch(() => setError(true));
  }, [id, selectedKinds]);

  const handleExpand = useCallback(
    (nodeId: string) => {
      fetchAssetGraph(nodeId, { hops: 1, relationshipTypes: filterParam(selectedKinds) })
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

  if (!id) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.exploreNoSeed}</div>;
  }
  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.exploreError}</div>;
  }
  if (!model) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.exploreLoading}</div>;
  }

  const picture = toPicture(model);
  const endpointName = (nodeId: string) => model.nodes.find((n) => n.id === nodeId)?.name ?? nodeId;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-[46px] flex-none items-center gap-2.5 border-b border-gowl-line bg-gowl-panel px-4">
        <span className="text-[13px] font-semibold text-gowl-t1">{strings.exploreTitle}</span>
        <span className="text-gowl-dim">{strings.exploreBreadcrumbSeparator}</span>
        <span className="font-mono text-[11.5px] text-gowl-accent">{model.seedId}</span>
        {model.truncated && (
          <span className="ml-4 text-[11px] text-gowl-amber">{strings.exploreTruncated}</span>
        )}
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
                    ? "rounded-md border border-gowl-accent-border bg-gowl-accent-bg px-2.5 py-1 text-[11.5px] text-gowl-accent"
                    : "rounded-md border border-gowl-line-2 px-2.5 py-1 text-[11.5px] text-gowl-t4 hover:border-gowl-hover"
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
              className="text-[11px] text-gowl-t6 hover:text-gowl-t4"
            >
              {strings.exploreClearFilter}
            </button>
          )}
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        <GraphCanvas
          picture={picture}
          colors={colors}
          mode={mode}
          onExpand={handleExpand}
          onSelect={handleSelect}
          label={`Neighbourhood of ${model.seedId}`}
        />

        {(selectedNode ?? selectedEdge) && (
          <div className="w-[352px] flex-none overflow-auto border-l border-gowl-line bg-gowl-panel p-4">
            {selectedNode && (
              <>
                <div className="mb-1 text-[17px] font-semibold text-gowl-t1">{selectedNode.name}</div>
                <div className="mb-4 break-all font-mono text-[11px] text-gowl-t6">{selectedNode.id}</div>
                <div className="space-y-2 text-[12px]">
                  <div className="flex justify-between">
                    <span className="text-gowl-t5">{strings.exploreKind}</span>
                    <span className="text-gowl-t2">{selectedNode.kind ?? strings.exploreKindHidden}</span>
                  </div>
                  {selectedNode.fullyQualifiedName && (
                    <div>
                      <div className="mb-1 text-gowl-t5">{strings.exploreFqn}</div>
                      <div className="break-all font-mono text-[11px] text-gowl-t3">
                        {selectedNode.fullyQualifiedName}
                      </div>
                    </div>
                  )}
                </div>
                <div className="mt-4 flex flex-col gap-2">
                  <Link
                    to={`/entity/${encodeURIComponent(selectedNode.id)}`}
                    className="rounded-md border border-gowl-line-2 px-3 py-2 text-center text-[12px] text-gowl-t3 hover:border-gowl-hover"
                  >
                    {strings.exploreOpenEntity}
                  </Link>
                  <button
                    type="button"
                    onClick={() => void handlePin()}
                    disabled={pinStatus === "pinning"}
                    className="rounded-md bg-gowl-accent px-3 py-2 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-60"
                  >
                    {strings.explorePinToInvestigation}
                  </button>
                  {pinStatus === "done" && (
                    <div className="text-[11px] text-gowl-ok">{strings.explorePinned}</div>
                  )}
                  {pinStatus === "failed" && (
                    <div className="text-[11px] text-gowl-bad">{strings.explorePinFailed}</div>
                  )}
                </div>
              </>
            )}
            {selectedEdge && (
              <>
                <span className="mb-2 inline-block rounded border border-gowl-accent-border bg-gowl-accent-bg px-1.5 py-0.5 font-mono text-[9.5px] text-gowl-accent">
                  {selectedEdge.derived ? strings.exploreEdgeDerived : strings.exploreEdgeAsserted}
                </span>
                <div className="mb-1 text-[17px] font-semibold text-gowl-t1">{selectedEdge.relationship}</div>
                <div className="mb-4 font-mono text-[11px] text-gowl-t6">
                  {`${endpointName(selectedEdge.from)} → ${endpointName(selectedEdge.to)}`}
                </div>
                <Link
                  to={`/paths?from=${encodeURIComponent(selectedEdge.from)}&to=${encodeURIComponent(model.seedId)}`}
                  className="block rounded-md border border-gowl-accent-border bg-gowl-accent-bg px-3 py-2 text-center text-[12px] text-gowl-accent"
                >
                  {strings.exploreTracePath}
                </Link>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
