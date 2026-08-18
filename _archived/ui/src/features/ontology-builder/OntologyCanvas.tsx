/** The ontology diagram canvas.
 *
 *  Renders entity types as coloured, icon-glyph nodes and relationships as
 *  directed edges, on React Flow — 00f-ui-architecture.md's 14 Aug 2026
 *  revision replaces this canvas's Cytoscape instance. Supports radial,
 *  tree, and force layouts (`layout.ts`) and polyline/orthogonal edge
 *  styles. Selecting a node or edge reports it upstream. */

import { useEffect, useMemo, useRef } from "react";
import {
  Background,
  Handle,
  Position,
  ReactFlow,
  type Edge,
  type EdgeTypes,
  type Node,
  type NodeProps,
  type NodeTypes,
  type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { palette } from "../../theme";
import { computeLayout, type LayoutName } from "./layout";
import type { FlowNodeData } from "./flowModel";

export type { LayoutName };
export type EdgeStyle = "polyline" | "orthogonal";

type Colors = (typeof palette)["light"];

interface OntologyCanvasProps {
  readonly elements: { readonly nodes: readonly FlowNodeData[]; readonly edges: readonly import("./flowModel").FlowEdgeData[] };
  readonly colors: Colors;
  readonly layout: LayoutName;
  readonly edgeStyle: EdgeStyle;
  readonly selectedId: string | null;
  readonly resetToken: number;
  readonly onSelectNode: (id: string) => void;
  readonly onSelectEdge: (id: string) => void;
  readonly onClearSelection: () => void;
}

function EntityNode({ data, selected }: NodeProps<Node<FlowNodeData & Record<string, unknown>>>) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", width: 80 }}>
      <div
        style={{
          width: 40,
          height: 40,
          borderRadius: "50%",
          // Same light-tint recipe as the Explorer/nodeIcons pattern: the
          // colour at full saturation, blended down via an alpha suffix
          // rather than a pre-computed rgba value, so one hex per entity is
          // the only colour input this node needs.
          background: `${data.color}29`,
          border: `${selected ? 4 : 2}px solid ${data.color}`,
          backgroundImage: `url(${data.icon})`,
          backgroundSize: "58%",
          backgroundRepeat: "no-repeat",
          backgroundPosition: "center",
          position: "relative",
        }}
      >
        <Handle type="target" position={Position.Top} style={{ opacity: 0 }} />
        <Handle type="source" position={Position.Bottom} style={{ opacity: 0 }} />
      </div>
      <span
        style={{
          marginTop: 6,
          fontSize: 12,
          textAlign: "center",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          maxWidth: 80,
        }}
      >
        {data.label}
      </span>
    </div>
  );
}

const NODE_TYPES: NodeTypes = { entityType: EntityNode };
const EDGE_TYPES: EdgeTypes = {};

export function OntologyCanvas({
  elements,
  colors,
  layout,
  edgeStyle,
  selectedId,
  resetToken,
  onSelectNode,
  onSelectEdge,
  onClearSelection,
}: OntologyCanvasProps) {
  const instanceRef = useRef<ReactFlowInstance | null>(null);

  const positions = useMemo(
    () => computeLayout(elements.nodes, elements.edges, layout),
    [elements.nodes, elements.edges, layout],
  );

  const flowNodes: Node[] = useMemo(
    () =>
      elements.nodes.map((node) => ({
        id: node.id,
        type: "entityType",
        position: positions[node.id] ?? { x: 0, y: 0 },
        data: node,
        selected: node.id === selectedId,
      })),
    [elements.nodes, positions, selectedId],
  );

  const flowEdges: Edge[] = useMemo(
    () =>
      elements.edges.map((edge) => ({
        id: edge.id,
        source: edge.source,
        target: edge.target,
        label: edge.label,
        selected: edge.id === selectedId,
        // A self-loop needs the bezier the 'default' type draws — 'straight'
        // and 'step' both degenerate to nothing when source === target.
        type: edge.selfLoop ? "default" : edgeStyle === "orthogonal" ? "step" : "straight",
        markerEnd: { type: "arrowclosed" as const, color: colors.textSubtle, width: 16, height: 16 },
        style: { stroke: colors.textSubtle },
        labelStyle: { fill: colors.textMuted, fontSize: 10 },
        labelBgStyle: { fill: colors.raised },
      })),
    [elements.edges, edgeStyle, selectedId, colors],
  );

  useEffect(() => {
    // `resetToken` is a plain counter bumped by the "Reset view" button —
    // its own value carries no meaning, only its *change* does.
    instanceRef.current?.fitView({ padding: 0.2 });
  }, [resetToken, layout]);

  return (
    <div
      role="img"
      aria-label="Ontology diagram showing entity types and relationships"
      style={{ width: "100%", height: "100%", background: colors.surface, borderRadius: 16 }}
    >
      <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        edgeTypes={EDGE_TYPES}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        onInit={(instance) => {
          instanceRef.current = instance;
          instance.fitView({ padding: 0.2 });
        }}
        onNodeClick={(_, node) => onSelectNode(node.id)}
        onEdgeClick={(_, edge) => onSelectEdge(edge.id)}
        onPaneClick={onClearSelection}
        proOptions={{ hideAttribution: true }}
      >
        <Background color={colors.border} gap={20} />
      </ReactFlow>
    </div>
  );
}
