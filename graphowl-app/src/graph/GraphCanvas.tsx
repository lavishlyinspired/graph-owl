/** The canvas.
 *
 *  **Everything decidable lives in `lib/graph/graphModel.ts`** — which nodes
 *  and edges exist, what style and shape they carry, whether the layout is
 *  deterministic — and is tested there. This component is the imperative
 *  shell: mount, feed, listen. `00f` requires graph tests to assert the
 *  model rather than the picture, and that is only possible if the picture
 *  is this thin.
 *
 *  Adapted from `ui/src/graph/GraphCanvas.tsx` (Plan 122a A3) — restyled
 *  against this app's own Tailwind tokens instead of antd, and without
 *  diff/time-travel support (see `lib/graph/graphModel.ts`'s own doc
 *  comment for why). */

import { Graph } from "@antv/g6";
import { useEffect, useMemo, useRef, useState } from "react";
import { strings } from "../lib/strings";
import {
  MAX_ZOOM,
  type Picture,
  layoutOptions,
  legendEntries,
  resolveEdgeStyle,
  resolveNodeStyle,
  toG6Data,
  wantsWebgl,
  type G6NodeDatum,
  type StyleColors,
} from "../lib/graph/graphModel";

type CanvasNode = Picture["nodes"][number];

export type CanvasSelection =
  | { readonly kind: "node"; readonly node: CanvasNode }
  | { readonly kind: "edge"; readonly edgeId: string }
  | { readonly kind: "none" };

/** G6's `.on()` accepts one broad event union regardless of which event name
 *  string is passed — it does not narrow the element-click shape (which
 *  carries `target.id`) out of the wider event type per event name the way a
 *  typed overload would, and that narrower type is internal to G6 and not
 *  exported for a consumer to import. This is the one, narrow, justified
 *  assertion boundary for that gap, rather than typing every handler
 *  parameter as `unknown` and losing the field entirely. */
function elementId(evt: unknown): string | undefined {
  const id = (evt as { target?: { id?: unknown } }).target?.id;
  return typeof id === "string" ? id : undefined;
}

/** Lazily resolved: `@antv/g-webgl` is a real dependency, but constructing
 *  its renderer eagerly on every mount would pay a WebGL context-creation
 *  cost even for the common small-neighbourhood case that never needs it.
 *  G6 renders in four layers (background, main, label, transient); only
 *  `main` — where nodes and edges actually draw — benefits from WebGL, so
 *  the other three stay on canvas explicitly rather than left unset, since
 *  G6's own `renderer` option is a function called once per layer and must
 *  return a renderer for every one of them. */
async function layerRenderer(wantsWebglRenderer: boolean) {
  const { Renderer: CanvasRenderer } = await import("@antv/g-canvas");
  if (!wantsWebglRenderer) return () => new CanvasRenderer();
  const { Renderer: WebglRenderer } = await import("@antv/g-webgl");
  return (layer: "background" | "main" | "label" | "transient") =>
    layer === "main" ? new WebglRenderer() : new CanvasRenderer();
}

export function GraphCanvas({
  picture,
  colors,
  mode,
  onExpand,
  onSelect,
  label,
}: {
  readonly picture: Picture;
  readonly colors: StyleColors;
  readonly mode: "dark" | "light";
  readonly onExpand: (id: string) => void;
  readonly onSelect: (selection: CanvasSelection) => void;
  readonly label: string;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const wrapper = useRef<HTMLDivElement | null>(null);
  const graph = useRef<Graph | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;

  // What G6's own hit-testing found for the click currently in flight —
  // written by the G6 listeners below, read and cleared by the plain React
  // `onClick` on `host` further down. **This indirection is required, not
  // stylistic.** G6's `node:click`/`edge:click`/`canvas:click` are bound via
  // `canvas.addEventListener` deep inside G6's own runtime
  // (`BehaviorController.forwardCanvasEvents`), entirely outside React's
  // event delegation — and calling a React state setter directly from that
  // context was observed, live, to never commit: the callback ran with the
  // correct node, `setState` was reached, and React's own fiber state
  // stayed `null` afterward every time, `flushSync` included. Only a setter
  // invoked from inside a *bona fide* React synthetic event (this
  // component's own `onClick`, which the browser's native click for the
  // same gesture also triggers, since `host` is an ancestor of the canvas)
  // was ever observed to stick. So G6 is used for what only it can do —
  // resolve which element a pixel hit — and the actual selection change is
  // made from React's own handler.
  const pendingClick = useRef<CanvasSelection | null>(null);

  // A single physical click on a node or edge is also observed to raise
  // *two* G6 events, not one — its own `node:click`/`edge:click` **and**
  // `canvas:click` immediately after, for the same gesture (G6's
  // `forwardCanvasEvents` re-resolves the event target per underlying DOM
  // event, and a `pointerup`-derived click and a separately synthesized
  // `click` do not always resolve to the same target — the second falls
  // through to the canvas). Without this guard the canvas round would
  // overwrite `pendingClick` back to "nothing" before the React `onClick`
  // below ever reads it. Set the instant an element's own click is seen,
  // consumed by the very next `canvas:click`, so a *genuine* later click on
  // empty canvas still clears the selection normally.
  const suppressNextCanvasClear = useRef(false);

  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    const onChange = () => setFullscreen(document.fullscreenElement === wrapper.current);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  const data = useMemo(() => toG6Data(picture, mode), [picture, mode]);
  const legend = useMemo(() => legendEntries(picture, mode, colors), [picture, mode, colors]);

  useEffect(() => {
    if (!host.current) return undefined;
    let disposed = false;

    // Chosen once, at creation, per node count *at creation*. A hybrid that
    // swapped renderers mid-session would discard the layout at the moment
    // a reader most needs it — their mental map of where things are is the
    // main thing keeping a large graph legible.
    void layerRenderer(wantsWebgl(picture.nodes.length)).then((renderer) => {
      if (disposed || !host.current) return;

      const instance = new Graph({
        container: host.current,
        data: toG6Data(picture, mode) as never,
        node: {
          style: (datum) => {
            const nodeDatum = datum as unknown as G6NodeDatum;
            return resolveNodeStyle(
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
            return resolveEdgeStyle({ classes: edgeData.classes, label: edgeData.label }, colors) as never;
          },
        },
        layout: layoutOptions(picture.seedId) as never,
        // No node/edge/combo drag — a reader repositioning one node to see
        // behind it is a normal way to read a graph, but the layout is
        // never re-run on drag, so nothing here re-triggers it either.
        behaviors: ["zoom-canvas", "drag-canvas"],
        animation: false,
        // Re-fits after every `render()` call, including the data-update
        // effect below.
        autoFit: "view",
        padding: 24,
        zoomRange: [0.01, MAX_ZOOM],
        renderer,
      });

      instance.on("node:click", (evt) => {
        const id = elementId(evt);
        if (!id) return;
        const found = picture.nodes.find((node) => node.id === id);
        if (!found) return;
        suppressNextCanvasClear.current = true;
        pendingClick.current = { kind: "node", node: found };
      });
      instance.on("edge:click", (evt) => {
        const id = elementId(evt);
        if (!id) return;
        suppressNextCanvasClear.current = true;
        pendingClick.current = { kind: "edge", edgeId: id };
      });
      instance.on("canvas:click", () => {
        if (suppressNextCanvasClear.current) {
          suppressNextCanvasClear.current = false;
          return;
        }
        pendingClick.current = { kind: "none" };
      });

      void instance.render();
      graph.current = instance;
    });

    return () => {
      disposed = true;
      graph.current?.destroy();
      graph.current = null;
    };
    // Colours (and therefore `mode`) change only with the theme, which
    // remounts cheaply; elements are handled below so an expansion does not
    // tear the canvas down.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  // Elements are replaced in place and re-laid out, rather than remounting.
  // A remount loses the reader's pan and zoom on every expand, which is the
  // one thing they were using to keep their place.
  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;
    instance.setData(data as never);
    void instance.render();
  }, [data]);

  // Which nodes are expandable changes what a click on them should do —
  // re-bind rather than re-derive from `picture` inside the handler, since
  // the handler closure was captured at mount time.
  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;
    const expandableIds = new Set(
      picture.nodes.filter((n) => !picture.expanded.includes(n.id)).map((n) => n.id),
    );
    const onNodeClick: Parameters<typeof instance.on>[1] = (evt) => {
      const id = elementId(evt);
      if (id && expandableIds.has(id)) expand.current(id);
    };
    instance.on("node:click", onNodeClick);
    return () => {
      instance.off("node:click", onNodeClick);
    };
  }, [picture]);

  const toggleFullscreen = () => {
    if (document.fullscreenElement) {
      void document.exitFullscreen();
    } else {
      void wrapper.current?.requestFullscreen();
    }
    requestAnimationFrame(() => {
      const instance = graph.current;
      if (!instance) return;
      instance.resize();
      void instance.fitView();
    });
  };

  return (
    <div ref={wrapper} className="flex min-h-0 min-w-0 flex-1 flex-col bg-gowl-bg">
      <div className="flex flex-none flex-wrap items-center gap-3 border-b border-gowl-line bg-gowl-panel px-4 py-2">
        {legend.map((entry) => (
          <div key={entry.key} className="flex items-center gap-1.5 text-[11px] text-gowl-t5">
            <span aria-hidden className="inline-block h-2.5 w-2.5 rounded-full" style={{ background: entry.color }} />
            {entry.label}
          </div>
        ))}
        <button
          type="button"
          className="ml-auto rounded-md border border-gowl-line-2 px-2 py-1 text-[11px] text-gowl-t4 hover:border-gowl-hover"
          onClick={toggleFullscreen}
        >
          {fullscreen ? strings.exploreFullscreenExit : strings.exploreFullscreenEnter}
        </button>
      </div>

      <div
        ref={host}
        role="img"
        aria-label={label}
        className="relative min-h-0 flex-1"
        style={{
          backgroundImage: `radial-gradient(${colors.border} 1px, transparent 1px)`,
          backgroundSize: "26px 26px",
        }}
        // The real click handler — see `pendingClick`'s doc comment above
        // for why the state change happens here and not in the G6 listener
        // that actually determined it. The same native gesture that G6's
        // own listener (deeper in the DOM, on the `<canvas>` it renders
        // into) already processed synchronously also bubbles up to this
        // ancestor, so `pendingClick.current` is already populated by the
        // time this fires.
        onClick={() => {
          const result = pendingClick.current;
          if (!result) return;
          pendingClick.current = null;
          onSelect(result);
        }}
      />
    </div>
  );
}
