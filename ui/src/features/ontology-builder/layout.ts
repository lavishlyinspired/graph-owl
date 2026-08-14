/** Node positions for the ontology diagram.
 *
 *  React Flow (unlike Cytoscape) does not compute layout itself — every
 *  node needs an `{x, y}` before it renders. `00f-ui-architecture.md`'s
 *  14 Aug 2026 revision pairs React Flow with d3-hierarchy (tree/radial)
 *  and d3-force, the same "small, MIT, this project already trusts the d3
 *  ecosystem" reasoning that picked d3-dag for lineage over ELK. */

import { forceCenter, forceLink, forceManyBody, forceSimulation } from "d3-force";
import { hierarchy, tree as d3tree } from "d3-hierarchy";

export type LayoutName = "radial" | "tree" | "force";

export interface LayoutNode {
  readonly id: string;
}

export interface LayoutEdge {
  readonly source: string;
  readonly target: string;
}

export type Positions = Record<string, { x: number; y: number }>;

const NODE_SPACING = 90;

/** Every node reachable from a synthetic root, one hop at a time —
 *  breadth-first, matching the `breadthfirst` layout the Cytoscape version
 *  used. A node no edge ever reaches (an isolated entity type) still gets a
 *  slot: attached directly under the synthetic root. */
function bfsTree(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[]): Map<string, string[]> {
  const children = new Map<string, string[]>();
  const hasParent = new Set<string>();
  for (const node of nodes) children.set(node.id, []);
  for (const edge of edges) {
    if (edge.source === edge.target) continue; // self-loop: not a layout relationship
    if (!children.has(edge.source) || !children.has(edge.target)) continue;
    if (hasParent.has(edge.target)) continue; // keep the tree simple: first parent wins
    children.get(edge.source)!.push(edge.target);
    hasParent.add(edge.target);
  }
  const roots = nodes.filter((n) => !hasParent.has(n.id)).map((n) => n.id);
  children.set("__root__", roots);
  return children;
}

interface TreeNode {
  readonly id: string;
  readonly children: TreeNode[];
}

function buildTree(id: string, children: Map<string, string[]>, seen: Set<string>): TreeNode {
  seen.add(id);
  const kids = (children.get(id) ?? []).filter((c) => !seen.has(c));
  return { id, children: kids.map((c) => buildTree(c, children, seen)) };
}

function treePositions(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[]): Positions {
  if (nodes.length === 0) return {};
  const children = bfsTree(nodes, edges);
  const seen = new Set<string>();
  const root = buildTree("__root__", children, seen);
  const layout = d3tree<TreeNode>().nodeSize([NODE_SPACING, NODE_SPACING]);
  const laidOut = layout(hierarchy(root, (d) => d.children));

  const positions: Positions = {};
  for (const node of laidOut.descendants()) {
    if (node.data.id === "__root__") continue;
    // d3's tree grows down the `x` axis for siblings and `y` for depth; this
    // project's canvas reads depth top-to-bottom, so depth maps to screen y.
    positions[node.data.id] = { x: node.x, y: node.y };
  }
  return positions;
}

function radialPositions(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[]): Positions {
  if (nodes.length === 0) return {};
  const children = bfsTree(nodes, edges);
  const seen = new Set<string>();
  const root = buildTree("__root__", children, seen);
  const layout = d3tree<TreeNode>().size([2 * Math.PI, 1]);
  const laidOut = layout(hierarchy(root, (d) => d.children));

  const positions: Positions = {};
  for (const node of laidOut.descendants()) {
    if (node.data.id === "__root__") continue;
    // Polar to Cartesian: `x` is angle (radians) from d3's size([2π, 1]),
    // `y` is depth (0..1) — scaled by NODE_SPACING per hop so each ring
    // sits a fixed distance farther from the centre, same spacing role
    // `minNodeSpacing` played in the Cytoscape `concentric` layout.
    const angle = node.x - Math.PI / 2;
    const radius = node.y * NODE_SPACING * (nodes.length + 1);
    positions[node.data.id] = { x: radius * Math.cos(angle), y: radius * Math.sin(angle) };
  }
  return positions;
}

/** Deterministic-*enough*: a fixed tick count rather than `.on("end")`, so
 *  this returns synchronously and the same input always runs the same
 *  number of simulation steps — unlike the Cytoscape `cose` layout it
 *  replaces, which was never deterministic either; this is a design
 *  surface (00f's determinism rule binds the Explorer, not this canvas). */
function forcePositions(nodes: readonly LayoutNode[], edges: readonly LayoutEdge[]): Positions {
  if (nodes.length === 0) return {};
  const simNodes = nodes.map((n) => ({ id: n.id, x: 0, y: 0 }));
  const simLinks = edges
    .filter((e) => e.source !== e.target)
    .map((e) => ({ source: e.source, target: e.target }));

  const simulation = forceSimulation(simNodes)
    .force("charge", forceManyBody().strength(-NODE_SPACING * 4))
    .force("link", forceLink(simLinks).id((d) => (d as { id: string }).id).distance(NODE_SPACING))
    .force("center", forceCenter(0, 0))
    .stop();

  for (let i = 0; i < 300; i += 1) simulation.tick();

  const positions: Positions = {};
  for (const node of simNodes) {
    positions[node.id] = { x: node.x, y: node.y };
  }
  return positions;
}

export function computeLayout(
  nodes: readonly LayoutNode[],
  edges: readonly LayoutEdge[],
  mode: LayoutName,
): Positions {
  switch (mode) {
    case "tree":
      return treePositions(nodes, edges);
    case "radial":
      return radialPositions(nodes, edges);
    case "force":
      return forcePositions(nodes, edges);
    default:
      return treePositions(nodes, edges);
  }
}
