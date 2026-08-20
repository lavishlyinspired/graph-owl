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
import { createNodeContextMenu } from "./nodeContextMenu";
import { strings } from "../lib/strings";
import {
  MAX_ZOOM,
  edgeLegendEntries,
  type Picture,
  layoutOptions,
  legendEntries,
  resolveEdgeStyle,
  resolveNodeStyle,
  toG6Data,
  wantsWebgl,
  withEntryPositions,
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

/** Which target type a G6 element event fired on. Same justified assertion
 *  boundary as {@link elementId}. */
function targetType(evt: unknown): "canvas" | "node" | "edge" | "combo" | undefined {
  const type = (evt as { targetType?: unknown }).targetType;
  return type === "canvas" || type === "node" || type === "edge" || type === "combo"
    ? type
    : undefined;
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
  onHide,
  onOpen,
  onSelect,
  label,
}: {
  readonly picture: Picture;
  readonly colors: StyleColors;
  readonly mode: "dark" | "light";
  readonly onExpand: (id: string) => void;
  readonly onHide: (id: string) => void;
  /** Right-click "Open entity" — the actual navigation, via `openTargetFor`
   *  (`lib/graph/graphContext.ts`), since this component has no router
   *  access of its own. */
  readonly onOpen: (id: string) => void;
  readonly onSelect: (selection: CanvasSelection) => void;
  readonly label: string;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const wrapper = useRef<HTMLDivElement | null>(null);
  const graph = useRef<Graph | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;
  const hide = useRef(onHide);
  hide.current = onHide;
  const open = useRef(onOpen);
  open.current = onOpen;
  // Which node the reader asked to expand, so the data-update effect knows
  // where newly-arrived nodes should bubble out *from* — see
  // `withEntryPositions`'s doc comment. Consumed (reset to `null`) the moment
  // it is used, so a later update this ref had nothing to do with (hiding a
  // node, toggling a relationship filter) never mistakenly bubbles something
  // out of a stale anchor.
  const expandAnchor = useRef<string | null>(null);
  const requestExpand = (id: string) => {
    expandAnchor.current = id;
    expand.current(id);
  };
  // The hover menu's `getContent` closes over this at Graph-construction
  // time, once — it needs a way to read whichever `picture` is current
  // rather than the one that existed when the Graph was built, same reason
  // `expand`/`hide` are refs and not the props directly.
  const pictureRef = useRef(picture);
  pictureRef.current = picture;

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

  /** The seed the view was last framed for — see the data effect below for
   *  why an expansion must not refit. */
  const fittedSeed = useRef<string | null>(null);

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

  /** **G6 sizes its `<canvas>` once, at creation, and never notices the
   *  container changing.** Opening the detail panel shrinks this canvas's
   *  column, but the canvas element keeps its old, wider box — so it hangs
   *  over the panel and, being later in paint order, swallows every click
   *  aimed at the panel's own buttons. That is what made "open entity" appear
   *  to do nothing: the link was never reached. Re-sizing on container change
   *  keeps the canvas inside its column. */
  useEffect(() => {
    const element = host.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(() => graph.current?.resize());
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const data = useMemo(() => toG6Data(picture, mode), [picture, mode]);
  const legend = useMemo(() => legendEntries(picture, mode, colors), [picture, mode, colors]);
  const edgeLegend = useMemo(() => edgeLegendEntries(picture, colors), [picture, colors]);

  useEffect(() => {
    if (!host.current) return undefined;
    let disposed = false;

    // **The `d3-force` layout's determinism was verified here empirically,
    // not only read off its source** (`lib/graph/graphModel.ts`'s
    // `layoutOptions` doc comment has the source-level argument: a fixed LCG
    // seed and index-derived starting positions, not `Math.random`). The
    // empirical check: load the same neighbourhood twice and compare each
    // node's `style.x`/`style.y` from `graph.getNodeData()` — identical to
    // two decimal places, every node, both loads. A *rendered-pixel* hash of
    // the two loads is not the same check and does not agree — canvas
    // anti-aliasing is not guaranteed pixel-stable across two independent
    // WebGL/Canvas contexts even when the underlying coordinates are
    // identical, so a PNG diff was conflating renderer jitter with layout
    // determinism. The model's own numbers are the claim that actually
    // matters here, and they hold.

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
                caption: nodeDatum.data.caption,
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
        layout: layoutOptions() as never,
        // `drag-element-force` is the pairing G6's own source names for a
        // `d3-force` layout (`DragElementForce.forceLayoutInstance`, which
        // looks specifically for a `d3-force`/`d3-force-3d` layout and warns
        // if it does not find one): dragging feeds the pointer position back
        // into the running simulation, so the rest of the graph reacts the
        // way it would if that node had actually moved there under its own
        // physics, not just been repositioned past it. `fixed: true` is what
        // keeps a released node where it was dropped rather than letting the
        // simulation pull it back — the same "stays exactly where it was
        // put" guarantee a non-force layout gets by simply never re-running.
        behaviors: ["zoom-canvas", "drag-canvas", { type: "drag-element-force", fixed: true }],
        // **A right-click menu, not a hover one.** A hover-triggered version
        // was tried first and reliably covered part of the graph the reader
        // had not asked to see — it opened the instant the pointer crossed a
        // node, whether or not they wanted a menu, and had to be dismissed
        // before the node underneath could be read. `contextmenu` only opens
        // on a deliberate right-click, matching what a reader already
        // expects from every other graph tool this console is modelled on.
        //
        // The content is plain DOM (`createNodeContextMenu`), built with
        // `getContent` rather than the plugin's own `getItems` shortcut —
        // `getItems` renders through the plugin's built-in CSS, which is a
        // hard-coded white list (`rgba(255,255,255,0.96)`) that would sit
        // wrong against this console's dark theme, and it has no per-item
        // disabled state, which "Expand" on an already-walked node needs.
        // Its click handlers are ordinary `addEventListener` calls — no G6
        // event dispatch and no React synthetic event system sit between the
        // click and the callback.
        plugins: [
          {
            type: "contextmenu",
            trigger: "contextmenu",
            enable: (evt: unknown) => targetType(evt) === "node",
            getContent: (evt: unknown) => {
              const id = elementId(evt);
              const node = id ? pictureRef.current.nodes.find((n) => n.id === id) : undefined;
              if (!id || !node) return Promise.resolve(document.createElement("div"));
              return Promise.resolve(
                createNodeContextMenu({
                  name: node.name,
                  alreadyExpanded: pictureRef.current.expanded.includes(id),
                  labels: {
                    expand: strings.exploreExpandNode,
                    alreadyExpanded: strings.exploreAlreadyExpanded,
                    hide: strings.exploreHideNode,
                    openEntity: strings.exploreOpenEntity,
                  },
                  // No explicit dismiss call needed here, unlike the hover
                  // menu this replaced: `Contextmenu`'s own `trigger` is
                  // `'contextmenu'`, not `'click'`, so its `document`-level
                  // click listener hides it unconditionally on *any* click —
                  // including the one that just fired this handler.
                  onExpand: () => requestExpand(id),
                  onHide: () => hide.current(id),
                  onOpen: () => open.current(id),
                }),
              );
            },
          },
        ],
        // **On, not off — found the hard way.** This was `false` under the
        // rule "nothing moves without the reader," written for the earlier
        // `concentric` layout, where turning it on would have let G6 quietly
        // slide nodes to a new spot on every re-render. It does not mean
        // that for `d3-force`: with it `false`, `drag-element-force` visibly
        // failed — a node could be dragged across the canvas and, verified
        // via `graph.getElementRenderStyle`, its model position never
        // changed, not even mid-drag. The reason is `D3ForceLayout`'s own
        // split: `layout()` resolves once, on the simulation's `'end'`
        // event, and only *that* resolution calls `syncPositionsFromD3` to
        // write d3's positions back into G6's model. A drag instead reheats
        // the simulation directly (`onDragStart`'s `alphaTarget(0.3).
        // restart()`) and depends on G6's per-tick animation subscription to
        // keep pulling positions out of it — which only exists when
        // `animation` is on. The rule still holds for *data* changes
        // nobody asked for; it was never meant to cover motion the reader's
        // own drag produces, and for `d3-force` those turn out to be the
        // same flag.
        animation: true,
        // **No `autoFit`.** It re-fits after *every* `render()`, including the
        // data-update effect below — so each expansion rescaled the whole
        // picture to fit the new, larger extent. Nothing about the geometry
        // actually changed, but the zoom did, and the reader sees exactly what
        // they reported: nodes shrinking and edges stretching a little further
        // on every expand. Every graph explorer worth copying holds the zoom
        // steady and lets the graph grow past the viewport instead. Fitting is
        // now explicit, and happens only when the canvas opens on a new seed.
        // Room for a two-line label below the lowest node: `autoFit` fits the
        // *nodes*, and a caption drawn under the bottom of the ring was
        // clipped by the canvas edge until the padding accounted for it.
        padding: 56,
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

      void instance.render().then(() => {
        if (disposed) return;
        void instance.fitView();
        fittedSeed.current = picture.seedId;
      });
      graph.current = instance;
      (window as unknown as { __debugGraph?: Graph }).__debugGraph = instance;
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
  //
  // **Refit only when the seed changes, never on an expansion.** A new seed is
  // a new question and deserves a fresh frame; an expansion is the same
  // question with more of it visible, and rescaling there is what made nodes
  // shrink and edges stretch each time. The reader keeps their zoom, and pans
  // to whatever arrived outside the viewport.
  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;

    // **Nodes bubble out of the node that was expanded, rather than
    // appearing wherever the layout's own default placement lands them.**
    // `withEntryPositions` needs the anchor's *current on-screen* position,
    // which only exists once this node has actually been rendered — so this
    // reads it from the live instance rather than from `picture`, which only
    // ever carries the ids and facts, never a screen coordinate. Consumed
    // immediately: a later update this same ref still pointed at (hiding a
    // node, toggling a filter) must not bubble something out of a stale
    // anchor.
    const anchorId = expandAnchor.current;
    expandAnchor.current = null;
    const anchor = anchorId ? instance.getElementRenderStyle(anchorId) : undefined;

    if (anchorId && typeof anchor?.["x"] === "number" && typeof anchor["y"] === "number") {
      const previous = { nodes: instance.getNodeData(), edges: instance.getEdgeData() } as never;
      const seeded = withEntryPositions(data, previous, anchorId, {
        x: anchor["x"],
        y: anchor["y"],
      });
      // Phase one: the new nodes enter *at* the anchor — no visible motion
      // yet, since they are co-located with a node already on screen.
      instance.setData(seeded as never);
      void instance
        .render()
        .then(() =>
          // Phase two: the same nodes, now already part of the model, move
          // from the anchor to their real settled position — an *update*,
          // not an *enter*, which is what makes G6 tween it rather than
          // snapping straight to the final spot.
          instance.setData(data as never),
        )
        .then(() => instance.render())
        .then(() => {
          if (fittedSeed.current === picture.seedId) return;
          void instance.fitView();
          fittedSeed.current = picture.seedId;
        });
      return;
    }

    instance.setData(data as never);
    void instance.render().then(() => {
      if (fittedSeed.current === picture.seedId) return;
      void instance.fitView();
      fittedSeed.current = picture.seedId;
    });
  }, [data, picture.seedId]);

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
      if (id && expandableIds.has(id)) requestExpand(id);
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
        <button
          type="button"
          className="ml-auto rounded-md border border-gowl-line-2 px-2 py-1 text-[15px] text-gowl-t4 hover:border-gowl-hover"
          onClick={toggleFullscreen}
        >
          {fullscreen ? strings.exploreFullscreenExit : strings.exploreFullscreenEnter}
        </button>
      </div>

      {/* The host is a *sibling* of the legend rather than its parent: G6
       *  owns the contents of whatever container it is handed, so React
       *  children placed inside it are children of something another
       *  library is mutating. A positioned wrapper keeps both under one
       *  coordinate space without either one owning the other. */}
      <div
        className="relative min-h-[400px] flex-1 overflow-hidden"
        style={{
          backgroundImage: `radial-gradient(${colors.border} 1px, transparent 1px)`,
          backgroundSize: "26px 26px",
        }}
      >
        <div
          ref={host}
          role="img"
          aria-label={label}
          className="absolute inset-0"
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

        {/* The key, over the canvas rather than above it — a reader checking
         *  what a dashed line means is looking at the drawing, not at the
         *  chrome. `pointer-events-none` so it never eats a canvas click:
         *  the click handler above sits on a sibling, so a swallowed click
         *  here would simply be lost. */}
        {(legend.length > 0 || edgeLegend.length > 0) && (
          <div className="pointer-events-none absolute bottom-4 left-4 flex flex-wrap items-center gap-4 rounded-lg border border-gowl-line bg-gowl-panel px-3.5 py-2.5">
            {legend.map((entry) => (
              <div key={entry.key} className="flex items-center gap-1.5 text-[15px] text-gowl-t5">
                <span
                  aria-hidden
                  className="inline-block h-2.5 w-2.5 rounded-full"
                  style={{ background: entry.color }}
                />
                {entry.label}
              </div>
            ))}
            {edgeLegend.map((entry) => (
              <div key={entry.key} className="flex items-center gap-1.5 text-[15px] text-gowl-t5">
                <span
                  aria-hidden
                  className="inline-block w-[18px]"
                  style={{
                    borderTopWidth: "1.5px",
                    borderTopStyle: entry.dashed ? "dashed" : "solid",
                    borderTopColor: entry.color,
                  }}
                />
                {entry.label}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
