/** The canvas.
 *
 *  AntV G6 rather than Cytoscape — 00f-ui-architecture.md's 14 Aug 2026
 *  revision. See that entry for the full reasoning; in short, G6 covers in
 *  one dependency what Cytoscape plus this project's own layout config
 *  covered before (built-in deterministic layouts, built-in WebGL), which
 *  is close to a like-for-like swap.
 *
 *  **Everything decidable lives in `graph/graphModel.ts`** — which nodes
 *  and edges exist, what style and shape they carry, whether the layout is
 *  deterministic — and is tested there. This component is the imperative
 *  shell: mount, feed, listen. `00f` requires graph tests to assert the
 *  model rather than the picture, and that is only possible if the picture
 *  is this thin. The one exception is `mode` (light/dark), derived from
 *  `colors` by reference against the two exported palette objects rather
 *  than threaded as a new prop through every caller between here and the
 *  theme hook — `colors` already *is* one of exactly those two objects
 *  everywhere it is passed, never a copy.
 *
 *  Reused wherever a node/edge picture needs drawing: the asset explorer
 *  (`App.tsx`) and `findingsQueue.tsx`'s evidence-graph section. */

import { CompressOutlined, ExpandOutlined, EyeOutlined } from "@ant-design/icons";
import { Button, Card, Space, Typography } from "./../components/ui/antd-compat";
import { Graph } from "@antv/g6";
import { useEffect, useMemo, useRef, useState } from "react";
import { palette } from "../theme";
import {
  MAX_ZOOM,
  type Picture,
  layoutOptions,
  legendEntries,
  resolveEdgeStyle,
  resolveNodeStyle,
  toG6Data,
  visiblePicture,
  wantsWebgl,
  type G6NodeDatum,
} from "./graphModel";

const { Text } = Typography;

const COPY = {
  showAll: "hidden — show all",
  fullscreenEnter: "Full screen",
  fullscreenExit: "Exit full screen",
  close: "✕",
  kind: "Kind",
  kindHidden: "hidden by authorization",
  id: "Id",
  fqn: "Fully qualified name",
  hideNode: "Hide this node",
};

type CanvasNode = Picture["nodes"][number];

/** G6's `.on()` accepts one broad event union regardless of which event
 *  name string is passed — it does not narrow `IElementEvent` (which has
 *  `target.id`) out of the wider `IEvent` per event name the way a typed
 *  overload would, and `IElementEvent` itself is an internal type G6 does
 *  not export for a consumer to import. This is the one, narrow, justified
 *  assertion boundary for that gap, rather than typing every handler
 *  parameter as `any`. */
function elementId(evt: unknown): string | undefined {
  const id = (evt as { target?: { id?: unknown } }).target?.id;
  return typeof id === "string" ? id : undefined;
}

/** Lazily resolved: `@antv/g-webgl` is a real dependency, but constructing
 *  its renderer eagerly on every mount would pay a WebGL context-creation
 *  cost even for the common small-neighbourhood case that never needs it —
 *  the same reasoning `wantsWebgl`'s own doc comment gives for the
 *  threshold existing at all. G6 renders in four layers (background, main,
 *  label, transient); only `main` — where nodes and edges actually draw —
 *  benefits from WebGL, so the other three stay on canvas explicitly
 *  rather than left unset, since G6's own `renderer` option is a function
 *  called once per layer and must return a renderer for every one of them. */
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
  onExpand,
  label,
}: {
  picture: Picture;
  colors: (typeof palette)["light"];
  onExpand: (id: string) => void;
  label: string;
}) {
  const mode = colors === palette.dark ? "dark" : "light";

  const host = useRef<HTMLDivElement | null>(null);
  const wrapper = useRef<HTMLDivElement | null>(null);
  const graph = useRef<Graph | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;

  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => setHidden(new Set()), [picture.seedId]);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected: CanvasNode | undefined = picture.nodes.find((node) => node.id === selectedId);

  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    const onChange = () => setFullscreen(document.fullscreenElement === wrapper.current);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  const visible = useMemo(() => visiblePicture(picture, hidden), [picture, hidden]);
  const data = useMemo(() => toG6Data(visible, mode), [visible, mode]);
  const legend = useMemo(() => legendEntries(visible, mode, colors), [visible, mode, colors]);

  useEffect(() => {
    if (!host.current) return undefined;
    let disposed = false;

    // Chosen once, at creation, per node count *at creation*. `00f` rejects
    // a hybrid that swaps renderers mid-session: the swap would discard the
    // layout at the moment a reader most needs it — their mental map of
    // where things are is the main thing keeping a large graph legible.
    void layerRenderer(wantsWebgl(picture.nodes.length)).then((renderer) => {
      if (disposed || !host.current) return;

      const instance = new Graph({
        container: host.current,
        data: toG6Data(visiblePicture(picture, hidden), mode) as never,
        node: {
          style: (datum) => {
            const nodeDatum = datum as unknown as G6NodeDatum;
            return resolveNodeStyle(
              { classes: nodeDatum.data.classes, color: nodeDatum.data.color, label: nodeDatum.data.label },
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
        // No `node`/`edge`/`combo` drag — a reader repositioning one node to
        // see behind it is a normal way to read a graph (Plan 114 explicitly
        // asks for this), but the layout is never re-run on drag, so nothing
        // here re-triggers it either.
        behaviors: ["zoom-canvas", "drag-canvas"],
        animation: false,
        // Re-fits after every `render()` call, including the data-update
        // effect below — this is what replaces the explicit `.fit()` call
        // Cytoscape needed after each layout run.
        autoFit: "view",
        padding: 24,
        // `MAX_ZOOM`: ported from the Cytoscape version, where it was
        // verified live against a real 2-node evidence graph. Not
        // re-verified live against G6's own `autoFit`/`zoomRange` — worth
        // checking in a browser before trusting this cap the same way.
        zoomRange: [0.01, MAX_ZOOM],
        renderer,
      });

      instance.on("node:click", (evt) => {
        const id = elementId(evt);
        if (id) setSelectedId(id);
      });
      instance.on("node:contextmenu", (evt) => {
        (evt as { preventDefault?: () => void }).preventDefault?.();
        const id = elementId(evt);
        if (!id) return;
        setHidden((current) => new Set(current).add(id));
        setSelectedId((current) => (current === id ? null : current));
      });
      instance.on("canvas:click", () => {
        // A click that hits no element — G6 does not fire `node:click` for
        // this, so the canvas's own click is the pane-click signal.
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
    // `autoFit: "view"` on the Graph itself re-fits after this render, the
    // same guarantee the explicit `.fit()` call gave the Cytoscape version.
    void instance.render();
  }, [data]);

  // Which nodes are expandable changes what a click on them should do —
  // re-bind rather than re-derive from `picture` inside the handler, since
  // the handler closure was captured at mount time.
  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;
    const expandableIds = new Set(visible.nodes.filter((n) => !visible.expanded.includes(n.id)).map((n) => n.id));
    const onNodeClick: Parameters<typeof instance.on>[1] = (evt) => {
      const id = elementId(evt);
      if (id && expandableIds.has(id)) expand.current(id);
    };
    instance.on("node:click", onNodeClick);
    return () => {
      instance.off("node:click", onNodeClick);
    };
  }, [visible]);

  useEffect(() => {
    const instance = graph.current;
    if (!instance) return;
    requestAnimationFrame(() => instance.resize());
  }, [selectedId]);

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
    <div ref={wrapper} style={{ background: colors.raised }}>
      <Space style={{ marginBottom: 8 }} wrap>
        {legend.map((entry) => (
          <Space key={entry.key} size={4}>
            <span
              aria-hidden
              style={{
                display: "inline-block",
                width: 10,
                height: 10,
                borderRadius: "50%",
                background: entry.color,
              }}
            />
            <Text style={{ fontSize: 12 }}>{entry.label}</Text>
          </Space>
        ))}
        {hidden.size > 0 && (
          <Button size="small" icon={<EyeOutlined />} onClick={() => setHidden(new Set())}>
            {hidden.size} {COPY.showAll}
          </Button>
        )}
        <Button
          size="small"
          icon={fullscreen ? <CompressOutlined /> : <ExpandOutlined />}
          onClick={toggleFullscreen}
          aria-label={fullscreen ? COPY.fullscreenExit : COPY.fullscreenEnter}
        >
          {fullscreen ? COPY.fullscreenExit : COPY.fullscreenEnter}
        </Button>
      </Space>

      <div style={{ display: "flex", gap: 12 }}>
        <div
          ref={host}
          role="img"
          aria-label={label}
          style={{
            flex: 1,
            minWidth: 0,
            height: fullscreen ? "calc(100vh - 56px)" : 420,
            border: `1px solid ${colors.border}`,
            borderRadius: 16,
            background: colors.raised,
          }}
        />
        {selected && (
          <Card
            size="small"
            title={selected.name}
            style={{ width: 240, flexShrink: 0 }}
            extra={
              <Button size="small" type="text" onClick={() => setSelectedId(null)}>
                {COPY.close}
              </Button>
            }
          >
            <Space direction="vertical" size={4} style={{ width: "100%" }}>
              <Space>
                <Text type="secondary">{COPY.kind}</Text>
                <Text>{selected.kind ?? selected.semanticType ?? COPY.kindHidden}</Text>
              </Space>
              <Space>
                <Text type="secondary">{COPY.id}</Text>
                <Text code copyable style={{ fontSize: 12 }}>
                  {selected.id}
                </Text>
              </Space>
              {"fullyQualifiedName" in selected && selected.fullyQualifiedName && (
                <Space direction="vertical" size={0}>
                  <Text type="secondary">{COPY.fqn}</Text>
                  <Text code style={{ fontSize: 12, wordBreak: "break-all" }}>
                    {selected.fullyQualifiedName}
                  </Text>
                </Space>
              )}
              <Button
                size="small"
                danger
                onClick={() => {
                  setHidden((current) => new Set(current).add(selected.id));
                  setSelectedId(null);
                }}
              >
                {COPY.hideNode}
              </Button>
            </Space>
          </Card>
        )}
      </div>
    </div>
  );
}
