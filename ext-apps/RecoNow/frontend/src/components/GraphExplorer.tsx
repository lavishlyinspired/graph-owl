import { useMemo, useState } from "react";
import type { CaseGraph, GraphEdge, GraphNode } from "../lib/api";
import { layout, type Position } from "../lib/graphLayout";

const WIDTH = 360;
const HEIGHT = 300;
const NODE_RADIUS = 15;

/** One invoice's own neighbourhood in the graph — same visual pattern as
 *  graph-owl's own console Explore screen (circular badge node, label
 *  underneath, `namespace:Type` underneath that; edges as labelled lines,
 *  dashed where derived; the seed and anything touching it in the accent
 *  colour), rendered here for a single case rather than the whole graph.
 *
 *  SVG, not canvas/G6 — see `graphLayout.ts` for why. */
export function GraphExplorer({ graph }: { readonly graph: CaseGraph }) {
  const [hovered, setHovered] = useState<string | null>(null);
  const positions = useMemo(
    () => layout(graph.nodes, graph.edges, WIDTH, HEIGHT),
    [graph.nodes, graph.edges],
  );

  if (graph.nodes.length === 0) {
    return (
      <p className="text-[11.5px] text-reco-t4">
        Nothing else in the graph references this invoice yet.
      </p>
    );
  }

  const byId = new Map(graph.nodes.map((n) => [n.id, n]));

  return (
    <div>
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        width="100%"
        height={HEIGHT}
        className="rounded border border-reco-line bg-reco-panel-2"
      >
        <g>
          {graph.edges.map((edge, i) => (
            <Edge
              key={`${edge.from}-${edge.to}-${i}`}
              edge={edge}
              from={positions.get(edge.from)}
              to={positions.get(edge.to)}
            />
          ))}
        </g>
        <g>
          {graph.nodes.map((node) => (
            <Node
              key={node.id}
              node={node}
              at={positions.get(node.id)}
              hovered={hovered === node.id}
              onHover={setHovered}
            />
          ))}
        </g>
      </svg>
      {hovered && byId.get(hovered) && (
        <p className="mt-1.5 text-[11px] text-reco-t4">
          <span className="text-reco-t2">{byId.get(hovered)!.label}</span>
          {byId.get(hovered)!.type_line && (
            <span className="ml-1.5 font-mono text-[9.5px]">{byId.get(hovered)!.type_line}</span>
          )}
        </p>
      )}
    </div>
  );
}

function Edge({
  edge,
  from,
  to,
}: {
  readonly edge: GraphEdge;
  readonly from: Position | undefined;
  readonly to: Position | undefined;
}) {
  if (!from || !to) return null;
  const mx = (from.x + to.x) / 2;
  const my = (from.y + to.y) / 2;
  const stroke = edge.highlighted ? "var(--reco-accent)" : "var(--reco-line-3)";

  return (
    <g>
      <line
        x1={from.x}
        y1={from.y}
        x2={to.x}
        y2={to.y}
        stroke={stroke}
        strokeWidth={edge.highlighted ? 1.4 : 1}
        strokeDasharray={edge.style === "dashed" ? "3 3" : undefined}
        opacity={edge.highlighted ? 0.85 : 0.55}
      />
      <text
        x={mx}
        y={my}
        textAnchor="middle"
        className="font-mono"
        fontSize={8}
        fill="var(--reco-t5)"
        style={{ paintOrder: "stroke", stroke: "var(--reco-panel-2)", strokeWidth: 3 }}
      >
        {edge.label}
      </text>
    </g>
  );
}

function Node({
  node,
  at,
  hovered,
  onHover,
}: {
  readonly node: GraphNode;
  readonly at: Position | undefined;
  readonly hovered: boolean;
  readonly onHover: (id: string | null) => void;
}) {
  if (!at) return null;
  const ring = node.is_seed ? "var(--reco-accent)" : "var(--reco-line-3)";
  const fill = node.is_seed ? "var(--reco-accent-bg)" : "var(--reco-panel)";

  return (
    <g
      transform={`translate(${at.x}, ${at.y})`}
      onMouseEnter={() => onHover(node.id)}
      onMouseLeave={() => onHover(null)}
      style={{ cursor: "default" }}
    >
      <circle
        r={NODE_RADIUS}
        fill={fill}
        stroke={ring}
        strokeWidth={node.is_seed || hovered ? 2 : 1.2}
      />
      <text
        textAnchor="middle"
        dominantBaseline="central"
        className="font-mono font-medium"
        fontSize={8.5}
        fill={node.is_seed ? "var(--reco-accent-hi)" : "var(--reco-t3)"}
      >
        {node.badge}
      </text>
      <text
        y={NODE_RADIUS + 11}
        textAnchor="middle"
        fontSize={9}
        fill="var(--reco-t2)"
      >
        {truncate(node.label, 16)}
      </text>
      {node.type_line && (
        <text
          y={NODE_RADIUS + 21}
          textAnchor="middle"
          className="font-mono"
          fontSize={7.5}
          fill="var(--reco-t5)"
        >
          {truncate(node.type_line, 20)}
        </text>
      )}
    </g>
  );
}

function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}
