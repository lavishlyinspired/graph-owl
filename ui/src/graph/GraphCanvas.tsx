/** The canvas.
 *
 *  Cytoscape rather than the hand-drawn SVG this replaced. The SVG was honest
 *  at demo scale and explicitly would not survive 10k nodes (`00f`); Cytoscape
 *  ships a WebGL renderer and a deterministic `breadthfirst` layout, which are
 *  the two things that decision needed. Plan 114 considered and rejected
 *  replacing Cytoscape with a third graph-rendering library (`00f` commits to
 *  exactly two, one per graph *shape*) — this file is the alternative:
 *  everything reported as "not looking good" fixed inside the existing
 *  renderer, plus the interactions Plan 114 explicitly asked for.
 *
 *  **Everything decidable lives in `graph/cytoscape.ts`** — which elements
 *  exist, what classes and colours they carry, whether the layout is
 *  deterministic — and is tested there. This component is the imperative
 *  shell: mount, feed, listen. `00f` requires graph tests to assert the model
 *  rather than the picture, and that is only possible if the picture is this
 *  thin. The one exception is `mode` (light/dark), derived from `colors` by
 *  reference against the two exported palette objects rather than threaded
 *  as a new prop through every caller between here and the theme hook —
 *  `colors` already *is* one of exactly those two objects everywhere it is
 *  passed, never a copy.
 *
 *  **Extracted from `App.tsx`** so it can be reused wherever a node/edge
 *  picture needs drawing, not only the asset explorer —
 *  `findingsQueue.tsx`'s evidence-graph section is the second caller (Epic
 *  105 P7's console half). Moving it out is what makes that reuse possible
 *  without a circular import: `App.tsx` already imports `ReviewSection.tsx`,
 *  which imports `findingsQueue.tsx`, so `findingsQueue.tsx` cannot import
 *  back from `App.tsx` itself. */

import { CompressOutlined, ExpandOutlined, EyeOutlined } from "@ant-design/icons";
import { Button, Card, Space, Typography } from "antd";
import cytoscape from "cytoscape";
import { useEffect, useMemo, useRef, useState } from "react";
import { palette } from "../theme";
import {
  MAX_ZOOM,
  type Picture,
  graphStyle,
  layoutOptions,
  legendEntries,
  toElements,
  visiblePicture,
  wantsWebgl,
} from "./cytoscape";

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

// Plan 114 Slice A. A layout with no `.fit()` after it draws at whatever zoom
// Cytoscape defaulted to — on a small neighbourhood (the common case for a
// finding's evidence graph) that is two nodes filling the visible area edge
// to edge, illegible. `padding` matches `layoutOptions`' own value so the
// fitted view and the layout agree on how much breathing room the picture
// gets. Not unit-tested directly — `00f` requires graph tests to assert the
// model, not the picture, and there is no model-level fact "the canvas is
// zoomed to fit"; `layoutOptions` itself stays fully tested in
// `cytoscape.test.ts`.
function layoutAndFit(instance: cytoscape.Core, seedId: string) {
  const layout = instance.layout(layoutOptions(seedId));
  layout.promiseOn("layoutstop").then(() => instance.fit(undefined, 24));
  layout.run();
}

type CanvasNode = Picture["nodes"][number];

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
  // `colors` is always exactly `palette.light` or `palette.dark` by
  // reference — every caller threads the same object straight through from
  // the theme hook, never a copy — so this is the mode Plan 114's colour
  // work needs without adding a prop every intermediate component would
  // have to pass on unchanged.
  const mode = colors === palette.dark ? "dark" : "light";

  const host = useRef<HTMLDivElement | null>(null);
  const wrapper = useRef<HTMLDivElement | null>(null);
  const cy = useRef<cytoscape.Core | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;

  // Plan 114 Slice B: which nodes a reader chose to hide, right now, on this
  // picture. Purely client-side (`00f`'s own state-ownership table: "Explorer
  // canvas state — selection, expansion, filters — is genuinely client-side"),
  // never persisted into `GraphModel`. Reset whenever the canvas opens on a
  // different seed, so a hide from one investigation cannot silently carry
  // into the next.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => setHidden(new Set()), [picture.seedId]);

  // The Neo4j-style inspector: click any node, not only an expandable one,
  // and see what it actually is without leaving the canvas.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected: CanvasNode | undefined = picture.nodes.find((node) => node.id === selectedId);

  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    const onChange = () => setFullscreen(document.fullscreenElement === wrapper.current);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  const visible = useMemo(() => visiblePicture(picture, hidden), [picture, hidden]);
  const elements = useMemo(() => toElements(visible, mode), [visible, mode]);
  const legend = useMemo(() => legendEntries(mode, colors), [mode, colors]);

  useEffect(() => {
    if (!host.current) return undefined;
    const instance = cytoscape({
      container: host.current,
      elements,
      // Chosen once, at creation. `00f` rejects a hybrid that swaps renderers
      // mid-session: the swap discards the layout at the moment a reader most
      // needs it, because their mental map of where things are is the main
      // thing keeping a large graph legible.
      // `renderer` is not in Cytoscape's published option type, but it is the
      // documented way to select the WebGL backend, so the cast is narrow and
      // stated rather than an `any` on the whole options object.
      ...(wantsWebgl(picture.nodes.length)
        ? ({ renderer: { name: "canvas", webgl: true } } as unknown as cytoscape.CytoscapeOptions)
        : {}),
      // Cytoscape has no text-direction style property — it renders labels
      // straight to canvas via a plain fillText call, with no bidi option
      // exposed. A right-to-left node caption is a real, unresolved gap left
      // for Epic 40 (the canvas is that epic's, Slice E only asserted the
      // rule this file cannot yet satisfy), not silently worked around with
      // an API that does not exist.
      //
      // The style array itself lives in `graph/cytoscape.ts` as `graphStyle`
      // — Plan 114 Slice A — so a missing label or arrowhead is a RED test
      // there rather than a screenshot someone has to notice.
      style: graphStyle(colors) as cytoscape.CytoscapeOptions["style"],
      // The layout itself never animates or re-runs on its own (`layoutOptions`:
      // `animate: false`) — that is the "nothing moves without the reader"
      // guarantee `00f` asks for. `autoungrabify` was a second, broader lock
      // this file added on top of that: it also disables manually *dragging*
      // a node, which nothing in `00f` asks for and Plan 114 explicitly asks
      // to remove — a reader repositioning one node to see behind it is a
      // normal way to read a graph, not a threat to layout determinism (the
      // layout is not re-run on drag).
      maxZoom: MAX_ZOOM,
    });
    instance.on("tap", "node.expandable", (event) => {
      expand.current(event.target.id());
    });
    // Any node, expandable or not — the inspector reads a node the reader is
    // curious about, not only one they could grow.
    instance.on("tap", "node", (event) => {
      setSelectedId(event.target.id());
    });
    // Right-click / long-press to hide — Cytoscape's own convention for a
    // secondary action, so it does not collide with the primary tap-to-expand
    // or tap-to-inspect gesture.
    instance.on("cxttap", "node", (event) => {
      const id = event.target.id() as string;
      setHidden((current) => new Set(current).add(id));
      setSelectedId((current) => (current === id ? null : current));
    });
    cy.current = instance;
    layoutAndFit(instance, picture.seedId);
    return () => {
      instance.destroy();
      cy.current = null;
    };
    // Colours change only with the theme, which remounts cheaply; elements are
    // handled below so an expansion does not tear the canvas down.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  // Elements are replaced in place and re-laid out, rather than remounting.
  // A remount loses the reader's pan and zoom on every expand, which is the
  // one thing they were using to keep their place.
  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    instance.elements().remove();
    instance.add(elements as cytoscape.ElementDefinition[]);
    layoutAndFit(instance, picture.seedId);
  }, [elements, picture.seedId]);

  // Opening or closing the inspector changes how much width the canvas
  // actually has (the 240px card plus its gap), the same class of resize the
  // fullscreen toggle already has to announce to Cytoscape explicitly.
  useEffect(() => {
    const instance = cy.current;
    if (!instance) return;
    requestAnimationFrame(() => instance.resize());
  }, [selectedId]);

  const toggleFullscreen = () => {
    if (document.fullscreenElement) {
      void document.exitFullscreen();
    } else {
      void wrapper.current?.requestFullscreen();
    }
    // A fullscreen toggle resizes the host element outside React's own
    // render cycle — Cytoscape needs telling, or it keeps drawing at the old
    // canvas size until the next unrelated re-layout.
    requestAnimationFrame(() => {
      const instance = cy.current;
      if (!instance) return;
      instance.resize();
      instance.fit(undefined, 24);
    });
  };

  return (
    <div ref={wrapper} style={{ background: colors.raised }}>
      <Space style={{ marginBottom: 8 }} wrap>
        {legend.map((entry) => (
          // A dot plus a plain-ink label, not coloured text on a coloured
          // fill: the dataviz skill's own rule is that text carries identity
          // through the mark beside it, never through its own colour, so a
          // low-contrast hue (the amber `table` slot against white) never
          // has to double as a text colour.
          <Space key={entry.kind} size={4}>
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

      {/* `minWidth: 0` on the flex child: a flex item's default min-width is
          `auto` (its content size), which on a canvas host with no intrinsic
          size floors it at whatever the parent's natural width already was —
          the inspector card next to it then has nowhere to go but off the
          right edge of the viewport rather than actually sharing the row. */}
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
          // The Neo4j-style inspector: whatever is selected, on the right,
          // rather than a reader having to hunt through a separate list for
          // the node they just clicked.
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
                <Text>{selected.kind ?? COPY.kindHidden}</Text>
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
