/** The ontology model, as G6 wants it — the ontology-graph counterpart to
 *  `graphModel.ts`'s `toG6Data` for the instance graph.
 *
 *  Reuses that file's own style-resolution functions (`resolveNodeStyle`,
 *  `resolveEdgeStyle`, `layoutOptions`, `nodeGlyph`, `shortLabel`,
 *  `semanticTypeColor`) rather than re-deriving them: a class node and an
 *  instance node both draw as a labelled, coloured circle, and a
 *  relationship edge draws the same way an asserted instance edge does —
 *  there is no "derived" or "inferred" concept for a *declared* ontology
 *  relationship, so every edge here carries no classes at all, which
 *  `resolveEdgeStyle` already reads as its plain asserted default. */

import type { ColorMode, G6Data, G6EdgeDatum, G6NodeDatum, StyleColors } from "../graph/graphModel";
import { nodeGlyph, resolveEdgeStyle, resolveNodeStyle, semanticTypeColor, shortLabel } from "../graph/graphModel";
import type { OntologyModel } from "./ontologyModel";

/** `graphModel.ts`'s own `layoutOptions`, retuned for a wider spread.
 *
 *  That tuning was measured against a 5–10 node instance neighbourhood
 *  (`link.distance: 160`, `manyBody.strength: -220`); a whole pack's class
 *  diagram runs to 18+ classes, and reusing it packed the picture into an
 *  unreadably dense cluster — checked live against the real GST pack, not
 *  assumed. Wider spacing is the only change; everything else (the
 *  `d3-force` type, `collide`, `alphaDecay`) carries the same reasoning
 *  `graphModel.ts`'s own doc comment already gives. */
export function ontologyLayoutOptions(): Record<string, unknown> {
  return {
    type: "d3-force",
    link: { distance: 220 },
    manyBody: { strength: -420 },
    collide: { radius: 90 },
    alphaDecay: 0.05,
  };
}

/** `resolveNodeStyle`'s own cascade, drawn heavier — checked live against
 *  the real 18-class GST pack, not assumed. That function's sizing (34px
 *  circle, 1px stroke, 11px label) was tuned for a 5–10 node instance
 *  neighbourhood where `fitView` barely has to zoom out at all. A whole
 *  pack's class diagram forces a far smaller `fitView` zoom just to fit
 *  everything on screen, and at that zoom the same sizing shrinks to a
 *  handful of screen pixels — reading as "faded" even though every colour
 *  value underneath is already full-contrast. Drawing measurably heavier
 *  is what survives that zoom-out; it is not a colour fix because colour
 *  was never the broken part. */
export function ontologyNodeStyle(
  datum: Parameters<typeof resolveNodeStyle>[0],
  colors: StyleColors,
): Record<string, unknown> {
  const base = resolveNodeStyle(datum, colors);
  return {
    ...base,
    size: 46,
    lineWidth: 2,
    fillOpacity: 0.92,
    labelFontSize: 13,
    iconFontSize: 12,
  };
}

/** Same reasoning as {@link ontologyNodeStyle}, for edges. */
export function ontologyEdgeStyle(
  datum: Parameters<typeof resolveEdgeStyle>[0],
  colors: StyleColors,
): Record<string, unknown> {
  const base = resolveEdgeStyle(datum, colors);
  return {
    ...base,
    lineWidth: 1.75,
    labelFontSize: 11.5,
  };
}

export function toOntologyG6Data(model: OntologyModel, mode: ColorMode): G6Data {
  const nodes: G6NodeDatum[] = model.classes.map((cls) => ({
    id: cls.id,
    type: "circle",
    data: {
      label: shortLabel(cls.name),
      glyph: nodeGlyph(cls.name),
      // Every class in one pack shares that pack's own namespace, so this
      // is one consistent colour per pack today — and a real split, not an
      // arbitrary one, the moment a second pack's classes are shown
      // alongside it.
      color: semanticTypeColor(cls.namespace, mode),
      classes: "",
    },
  }));

  const edges: G6EdgeDatum[] = model.relationships.map((rel) => ({
    id: rel.id,
    source: rel.from,
    target: rel.to,
    data: {
      label: rel.label,
      classes: "",
    },
  }));

  return { nodes, edges };
}
