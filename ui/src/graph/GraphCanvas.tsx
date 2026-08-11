/** The canvas.
 *
 *  Cytoscape rather than the hand-drawn SVG this replaced. The SVG was honest
 *  at demo scale and explicitly would not survive 10k nodes (`00f`); Cytoscape
 *  ships a WebGL renderer and a deterministic `breadthfirst` layout, which are
 *  the two things that decision needed.
 *
 *  **Everything decidable lives in `graph/cytoscape.ts`** — which elements
 *  exist, what classes they carry, whether the layout is deterministic — and is
 *  tested there. This component is the imperative shell: mount, feed, listen.
 *  `00f` requires graph tests to assert the model rather than the picture, and
 *  that is only possible if the picture is this thin.
 *
 *  **Extracted from `App.tsx`** so it can be reused wherever a node/edge
 *  picture needs drawing, not only the asset explorer —
 *  `findingsQueue.tsx`'s evidence-graph section is the second caller (Epic
 *  105 P7's console half). Moving it out is what makes that reuse possible
 *  without a circular import: `App.tsx` already imports `ReviewSection.tsx`,
 *  which imports `findingsQueue.tsx`, so `findingsQueue.tsx` cannot import
 *  back from `App.tsx` itself. Behaviour is unchanged from the inline
 *  version; only the file moved. */

import cytoscape from "cytoscape";
import { useEffect, useMemo, useRef } from "react";
import { brand, type palette } from "../theme";
import { type Picture, layoutOptions, toElements, wantsWebgl } from "./cytoscape";

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
  const host = useRef<HTMLDivElement | null>(null);
  const cy = useRef<cytoscape.Core | null>(null);
  const expand = useRef(onExpand);
  expand.current = onExpand;

  const elements = useMemo(() => toElements(picture), [picture]);

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
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "font-size": 11,
            color: colors.text,
            "text-valign": "bottom",
            "text-margin-y": 4,
            "background-color": colors.primary,
            width: 18,
            height: 18,
            // Cytoscape has no text-direction style property — it renders
            // labels straight to canvas via a plain fillText call, with no
            // bidi option exposed. A right-to-left node caption is a real,
            // unresolved gap left for Epic 40 (the canvas is that epic's,
            // Slice E only asserted the rule this file cannot yet satisfy),
            // not silently worked around with an API that does not exist.
          },
        },
        { selector: "node.seed", style: { width: 26, height: 26, "font-weight": "bold" } },
        // A ring, not a colour: the expandable marker has to survive a reader
        // who cannot distinguish the two hues.
        {
          selector: "node.expandable",
          style: { "border-width": 3, "border-color": colors.primary, "background-opacity": 0.35 },
        },
        {
          selector: "node.truncated",
          style: { "border-width": 3, "border-style": "dashed", "border-color": colors.text },
        },
        { selector: "node.hidden-kind", style: { "background-color": colors.border } },
        // Removed nodes stay in the picture, marked by shape *and* opacity
        // rather than colour alone — a deletion shown only in red is invisible
        // to a reader who cannot see red.
        {
          selector: "node.removed",
          style: { shape: "diamond", "background-opacity": 0.4, "border-style": "dashed", "border-width": 2, "border-color": colors.text },
        },
        { selector: "node.added", style: { shape: "star" } },
        { selector: "edge", style: { width: 1, "line-color": colors.border, "curve-style": "straight" } },
        {
          // **A conclusion, drawn as one.** Dashed and tinted, so it is legible
          // as inferred without colour alone carrying the meaning —
          // `00h-ui-design-system.md` requires a state to survive being unable
          // to tell two hues apart, and this is a state somebody acts on.
          selector: "edge.derived",
          style: {
            "line-style": "dashed",
            "line-color": brand.cyan400,
            "target-arrow-color": brand.cyan400,
          },
        },
        { selector: "edge.removed", style: { "line-style": "dashed", "line-color": colors.text } },
        { selector: "edge.added", style: { width: 2, "line-color": colors.primary } },
      ],
      layout: layoutOptions(picture.seedId),
      // The reader drives the picture; nothing moves on its own.
      autoungrabify: true,
    });
    instance.on("tap", "node.expandable", (event) => {
      expand.current(event.target.id());
    });
    cy.current = instance;
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
    instance.layout(layoutOptions(picture.seedId)).run();
  }, [elements, picture.seedId]);

  return (
    <div
      ref={host}
      role="img"
      aria-label={label}
      style={{
        height: 420,
        border: `1px solid ${colors.border}`,
        borderRadius: 16,
        background: colors.raised,
      }}
    />
  );
}
