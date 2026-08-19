import type { GraphEdge, GraphNode } from "./api";

export interface Position {
  readonly x: number;
  readonly y: number;
}

/** A small force-directed layout: nodes repel, edges pull their two ends
 *  together, everything is drawn gently toward the centre.
 *
 *  **Deterministic, not random.** A layout that jitters between renders is
 *  unreadable — a reviewer who looks away and back should not have to
 *  re-find every node. Starting positions come from a hash of each node's own
 *  id, not `Math.random()`, so the same graph always lands the same way.
 *
 *  Hand-rolled rather than a physics library: the console's own Explore
 *  screen (`graphowl-app`) uses AntV G6 on a raw `<canvas>`, and this
 *  project's own CLAUDE.md documents a real bug class there — a React state
 *  setter called from G6's raw `canvas.addEventListener` can run correctly
 *  and never update state. A handful of nodes in a drawer does not need a
 *  charting engine, and SVG keeps every element a normal, clickable DOM node
 *  with ordinary React events. */
export function layout(
  nodes: readonly GraphNode[],
  edges: readonly GraphEdge[],
  width: number,
  height: number,
): Map<string, Position> {
  if (nodes.length === 0) return new Map();

  const positions = new Map<string, { x: number; y: number }>();
  const cx = width / 2;
  const cy = height / 2;
  const radius = Math.min(width, height) * 0.32;

  nodes.forEach((n, i) => {
    // Deterministic "random": a hash of the id picks an angle, so the same
    // node always starts in the same place regardless of array order.
    let hash = 0;
    for (let c = 0; c < n.id.length; c += 1) hash = (hash * 31 + n.id.charCodeAt(c)) | 0;
    const angle = (Math.abs(hash) % 360) * (Math.PI / 180) + i * 0.0001;
    positions.set(n.id, {
      x: cx + radius * Math.cos(angle),
      y: cy + radius * Math.sin(angle),
    });
  });

  const ids = nodes.map((n) => n.id);
  const REPULSION = 12000;
  const SPRING_LENGTH = 130;
  const SPRING_STRENGTH = 0.02;
  const CENTER_STRENGTH = 0.01;
  const ITERATIONS = 200;

  for (let iter = 0; iter < ITERATIONS; iter += 1) {
    const forces = new Map<string, { fx: number; fy: number }>();
    for (const id of ids) forces.set(id, { fx: 0, fy: 0 });

    for (let i = 0; i < ids.length; i += 1) {
      for (let j = i + 1; j < ids.length; j += 1) {
        const a = positions.get(ids[i]!)!;
        const b = positions.get(ids[j]!)!;
        const dx = a.x - b.x;
        const dy = a.y - b.y;
        const distSq = Math.max(dx * dx + dy * dy, 1);
        const force = REPULSION / distSq;
        const dist = Math.sqrt(distSq);
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        forces.get(ids[i]!)!.fx += fx;
        forces.get(ids[i]!)!.fy += fy;
        forces.get(ids[j]!)!.fx -= fx;
        forces.get(ids[j]!)!.fy -= fy;
      }
    }

    for (const edge of edges) {
      const a = positions.get(edge.from);
      const b = positions.get(edge.to);
      if (!a || !b) continue;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const dist = Math.max(Math.hypot(dx, dy), 1);
      const stretch = dist - SPRING_LENGTH;
      const fx = (dx / dist) * stretch * SPRING_STRENGTH;
      const fy = (dy / dist) * stretch * SPRING_STRENGTH;
      forces.get(edge.from)!.fx += fx;
      forces.get(edge.from)!.fy += fy;
      forces.get(edge.to)!.fx -= fx;
      forces.get(edge.to)!.fy -= fy;
    }

    for (const id of ids) {
      const p = positions.get(id)!;
      const f = forces.get(id)!;
      f.fx += (cx - p.x) * CENTER_STRENGTH;
      f.fy += (cy - p.y) * CENTER_STRENGTH;
      const damping = 0.85;
      positions.set(id, {
        x: Math.min(width, Math.max(0, p.x + f.fx * damping)),
        y: Math.min(height, Math.max(0, p.y + f.fy * damping)),
      });
    }
  }

  return positions;
}
