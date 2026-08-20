/** The ontology graph's own canvas — the read-only counterpart to
 *  `GraphCanvas.tsx` for a class/relationship diagram rather than an
 *  instance neighbourhood.
 *
 *  **Deliberately not a reuse of `GraphCanvas.tsx` itself.** That
 *  component's interaction model — expand a node to fetch its neighbours,
 *  hover/right-click for Expand/Hide/Open actions, a seed the layout
 *  centres on — is built for *incremental* exploration of a graph too large
 *  to load at once. An ontology's classes and relationships are already
 *  fully loaded in one `/sparql` call (`ontologyModel.ts`); there is
 *  nothing left to expand, and forcing that menu onto a node that has
 *  nothing more to reveal would offer an action that does nothing when
 *  clicked. What *is* reused is the technology and the drawing rules'
 *  *cascade* (`ontologyNodeStyle`/`ontologyEdgeStyle` in
 *  `lib/ontology/ontologyGraphModel.ts` wrap `resolveNodeStyle`/
 *  `resolveEdgeStyle` rather than diverging from them), so a class node and
 *  an instance node still read as the same kind of thing wherever either is
 *  drawn. What differs — the layout tuning (`ontologyLayoutOptions`) and
 *  the node/edge weight (`ontologyNodeStyle`/`ontologyEdgeStyle`) — both
 *  exist because a class diagram's node count forces `fitView` to a far
 *  smaller zoom than an instance neighbourhood ever needs, and the
 *  instance sizing goes illegible at that zoom (checked live).
 *
 *  Pan, zoom and drag-to-reposition only, for slice 1 — no selection, no
 *  detail panel. Clicking a node to see its own relationships is slice 2's
 *  job, once this walking skeleton is in. */

import { Graph } from "@antv/g6";
import { useEffect, useRef } from "react";
import {
  MAX_ZOOM,
  wantsWebgl,
  type G6Data,
  type G6NodeDatum,
  type StyleColors,
} from "../lib/graph/graphModel";
import { ontologyEdgeStyle, ontologyLayoutOptions, ontologyNodeStyle } from "../lib/ontology/ontologyGraphModel";

/** Below this, a legend/relationship label is no longer reliably readable
 *  — checked live at 0.35 against the real 18-class GST pack. `fitView`
 *  would otherwise zoom out as far as it takes to fit every node, however
 *  small that makes them; clamping the floor means panning takes over
 *  once a diagram outgrows what a single screen can show at a legible
 *  size, the same trade every graph-diagram tool below a certain node
 *  count has to make. */
const MIN_ONTOLOGY_ZOOM = 0.35;

/** Lazily resolved, same reasoning as `GraphCanvas.tsx`'s own copy: paying
 *  for a WebGL context on every mount would cost the common
 *  small-diagram case that never needs it. */
async function layerRenderer(wantsWebglRenderer: boolean) {
  const { Renderer: CanvasRenderer } = await import("@antv/g-canvas");
  if (!wantsWebglRenderer) return () => new CanvasRenderer();
  const { Renderer: WebglRenderer } = await import("@antv/g-webgl");
  return (layer: "background" | "main" | "label" | "transient") =>
    layer === "main" ? new WebglRenderer() : new CanvasRenderer();
}

export function OntologyCanvas({
  data,
  colors,
  label,
}: {
  readonly data: G6Data;
  readonly colors: StyleColors;
  readonly label: string;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const graph = useRef<Graph | null>(null);

  useEffect(() => {
    const element = host.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(() => graph.current?.resize());
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!host.current) return undefined;
    let disposed = false;

    void layerRenderer(wantsWebgl(data.nodes.length)).then((renderer) => {
      if (disposed || !host.current) return;

      const instance = new Graph({
        container: host.current,
        data: data as never,
        node: {
          style: (datum) => {
            const nodeDatum = datum as unknown as G6NodeDatum;
            return ontologyNodeStyle(
              {
                classes: nodeDatum.data.classes,
                color: nodeDatum.data.color,
                label: nodeDatum.data.label,
                glyph: nodeDatum.data.glyph,
              },
              colors,
            ) as never;
          },
        },
        edge: {
          style: (datum) => {
            const edgeData = (datum.data ?? {}) as unknown as { classes: string; label: string };
            return ontologyEdgeStyle({ classes: edgeData.classes, label: edgeData.label }, colors) as never;
          },
        },
        layout: ontologyLayoutOptions() as never,
        behaviors: ["zoom-canvas", "drag-canvas", "drag-element"],
        animation: true,
        padding: 56,
        zoomRange: [MIN_ONTOLOGY_ZOOM, MAX_ZOOM],
        renderer,
      });

      void instance.render().then(() => {
        if (disposed) return;
        void instance.fitView();
      });
      graph.current = instance;
    });

    return () => {
      disposed = true;
      graph.current?.destroy();
      graph.current = null;
    };
    // `data` changes are handled by the effect below, in place; only colour
    // changes warrant tearing the instance down and remounting.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;
    instance.setData(data as never);
    void instance.render().then(() => void instance.fitView());
  }, [data]);

  return (
    <div
      ref={host}
      role="img"
      aria-label={label}
      className="min-h-[75vh] flex-1"
      style={{
        backgroundImage: `radial-gradient(${colors.border} 1px, transparent 1px)`,
        backgroundSize: "26px 26px",
      }}
    />
  );
}
