/** The model, as AntV G6 wants it.
 *
 *  Replaces `cytoscape.ts` — 00f-ui-architecture.md's 14 Aug 2026 revision
 *  moves the Explorer from Cytoscape to AntV G6. `00f` requires graph tests
 *  to assert **the model, not the picture**, so the part of the renderer
 *  swap that can be wrong in a way a reader would act on lives here as a
 *  pure function: which nodes exist, which classes they carry, what style
 *  each class resolves to, and whether the layout is deterministic. What
 *  G6 paints from that is a drawing decision.
 *
 *  Everything in this file that does **not** mention a G6-specific shape —
 *  `Picture`, `edgeId`, `nodeClasses`, `edgeClasses`, `kindColor`,
 *  `semanticTypeColor`, `legendEntries`, `visiblePicture`, `wantsWebgl` —
 *  is carried over unchanged from `cytoscape.ts`: none of it was ever
 *  Cytoscape-specific, it only lived in a Cytoscape-named file. Only
 *  `toElements`/`graphStyle`/`layoutOptions` were actually renderer-shaped,
 *  and those three are what changed below.
 */

import type { AssetKind, GraphEdge, GraphNode } from "../api";
import { brand } from "../theme";
import { canvasLabel } from "./bidiLabel";
import type { Change, DiffEdge, DiffNode } from "./diff";

/** What the canvas is being asked to draw. Unchanged from `cytoscape.ts`. */
export interface Picture {
  readonly seedId: string;
  readonly nodes: readonly (GraphNode | DiffNode)[];
  readonly edges: readonly (GraphEdge | DiffEdge)[];
  /** Nodes already walked; the rest are drawn as expandable. */
  readonly expanded: readonly string[];
  /** Nodes hiding neighbours the budget cut. */
  readonly truncatedAt: readonly string[];
}

function changeOf(item: { change?: Change }): Change {
  return item.change ?? "unchanged";
}

/** A stable id for an edge. Unchanged from `cytoscape.ts` — see its own
 *  doc comment for why both endpoints and the relationship name are
 *  needed: `a contains b` and `a feeds b` are two facts about one pair,
 *  and an id built from the endpoints alone would collide them. */
export function edgeId(edge: GraphEdge): string {
  return `${edge.from}→${edge.to}→${edge.relationship}`;
}

/** Classes drive styling, and each one is a fact the reader can act on.
 *  Unchanged from `cytoscape.ts` — see its own doc comment for `removed`
 *  and `hidden-kind`'s reasoning. */
export function nodeClasses(node: GraphNode | DiffNode, picture: Picture): string {
  const classes: string[] = [changeOf(node as DiffNode)];
  if (node.id === picture.seedId) classes.push("seed");
  if (!picture.expanded.includes(node.id)) classes.push("expandable");
  if (picture.truncatedAt.includes(node.id)) classes.push("truncated");
  if (node.kind === null && !node.semanticType) classes.push("hidden-kind");
  return classes.join(" ");
}

export type ColorMode = "light" | "dark";

/** Plan 114 Slice D, carried over unchanged: the dataviz skill's validated
 *  categorical palette, slots 1–5 in fixed order. */
const KIND_COLORS: Record<ColorMode, Record<AssetKind, string>> = {
  light: {
    service: "#2a78d6",
    database: "#eb6834",
    schema: "#1baf7a",
    table: "#eda100",
    column: "#e87ba4",
  },
  dark: {
    service: "#3987e5",
    database: "#d95926",
    schema: "#199e70",
    table: "#c98500",
    column: "#d55181",
  },
};

export function kindColor(kind: AssetKind | null, mode: ColorMode, colors: StyleColors): string {
  return kind === null ? colors.border : KIND_COLORS[mode][kind];
}

const KIND_ORDER: readonly AssetKind[] = ["service", "database", "schema", "table", "column"];

const SEMANTIC_TYPE_COLORS: Record<ColorMode, readonly string[]> = {
  light: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#4a3aa7", "#e34948"],
  dark: ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"],
};

export function semanticTypeColor(type: string, mode: ColorMode): string {
  let hash = 0;
  for (let i = 0; i < type.length; i += 1) {
    hash = (hash * 31 + type.charCodeAt(i)) | 0;
  }
  const palette = SEMANTIC_TYPE_COLORS[mode];
  return palette[Math.abs(hash) % palette.length]!;
}

export interface LegendEntry {
  readonly key: string;
  readonly label: string;
  readonly color: string;
}

export function legendEntries(picture: Picture, mode: ColorMode, colors: StyleColors): LegendEntry[] {
  const kindsPresent = new Set(
    picture.nodes.map((node) => node.kind).filter((kind): kind is AssetKind => kind !== null),
  );
  const kindEntries: LegendEntry[] = KIND_ORDER.filter((kind) => kindsPresent.has(kind)).map(
    (kind) => ({
      key: kind,
      label: kind.charAt(0).toUpperCase() + kind.slice(1),
      color: kindColor(kind, mode, colors),
    }),
  );

  const typesSeen: string[] = [];
  for (const node of picture.nodes) {
    if (node.kind === null && node.semanticType && !typesSeen.includes(node.semanticType)) {
      typesSeen.push(node.semanticType);
    }
  }
  const typeEntries: LegendEntry[] = typesSeen.map((type) => ({
    key: type,
    label: type,
    color: semanticTypeColor(type, mode),
  }));

  return [...kindEntries, ...typeEntries];
}

/** Plan 114 Slice B, unchanged. */
export function visiblePicture(picture: Picture, hidden: ReadonlySet<string>): Picture {
  return { ...picture, nodes: picture.nodes.filter((node) => !hidden.has(node.id)) };
}

function nodeColor(node: Picture["nodes"][number], mode: ColorMode): string | undefined {
  if (node.kind !== null) return KIND_COLORS[mode][node.kind];
  if (node.semanticType) return semanticTypeColor(node.semanticType, mode);
  return undefined;
}

/** Classes on an edge. Unchanged from `cytoscape.ts`. */
export function edgeClasses(edge: GraphEdge & { change?: Change }): string {
  const classes: string[] = [changeOf(edge)];
  if (edge.derived === true) classes.push("derived");
  return classes.join(" ");
}

/** The colours the style needs, named structurally rather than imported
 *  from `theme.ts`. Unchanged from `cytoscape.ts`. */
export interface StyleColors {
  readonly text: string;
  readonly primary: string;
  readonly border: string;
  readonly raised: string;
}

// ---------------------------------------------------------------------
// Everything below this line is G6-shaped and did not exist in this form
// in cytoscape.ts — the point where the renderer swap actually happens.
// ---------------------------------------------------------------------

/** A G6 node — typed structurally rather than imported from `@antv/g6`,
 *  matching {@link Picture}'s own reasoning: this module and its tests do
 *  not need the library, only the shape G6 will accept. */
export interface G6NodeDatum {
  readonly id: string;
  /** The registered G6 node type — `'circle'` is the default shape; a
   *  removed node draws as a diamond, an added one as a star, the same
   *  "shape and opacity, not colour alone" rule `cytoscape.ts` used,
   *  expressed at the element-type level rather than a CSS shape property
   *  because G6 has no such property — shape is chosen per-node via `type`. */
  readonly type: "circle" | "diamond" | "star";
  readonly data: {
    readonly label: string;
    readonly color?: string;
    readonly classes: string;
  };
}

export interface G6EdgeDatum {
  readonly id: string;
  readonly source: string;
  readonly target: string;
  readonly data: {
    readonly label: string;
    readonly classes: string;
  };
}

export interface G6Data {
  readonly nodes: readonly G6NodeDatum[];
  readonly edges: readonly G6EdgeDatum[];
}

function resolveNodeShape(classes: string): G6NodeDatum["type"] {
  if (classes.includes("removed")) return "diamond";
  if (classes.includes("added")) return "star";
  return "circle";
}

/** The whole picture as G6 data. An edge to a node not present in the
 *  picture is dropped rather than drawn to nowhere — unchanged reasoning
 *  from `cytoscape.ts`'s `toElements`, though G6 (unlike Cytoscape) does
 *  not require nodes to precede edges in one flat list; its `nodes`/`edges`
 *  are already separate collections, so that ordering concern from the
 *  Cytoscape version does not carry over — there is nothing left for it to
 *  be wrong about. */
export function toG6Data(picture: Picture, mode: ColorMode = "light"): G6Data {
  const present = new Set(picture.nodes.map((node) => node.id));

  const nodes: G6NodeDatum[] = picture.nodes.map((node) => {
    const classes = nodeClasses(node, picture);
    return {
      id: node.id,
      type: resolveNodeShape(classes),
      data: {
        label: canvasLabel(node.name),
        ...(nodeColor(node, mode) === undefined ? {} : { color: nodeColor(node, mode) }),
        classes,
      },
    };
  });

  const edges: G6EdgeDatum[] = picture.edges
    .filter((edge) => present.has(edge.from) && present.has(edge.to))
    .map((edge) => ({
      id: edgeId(edge),
      source: edge.from,
      target: edge.to,
      data: {
        label: edge.relationship,
        classes: edgeClasses(edge as GraphEdge & DiffEdge),
      },
    }));

  return { nodes, edges };
}

/** A node's drawing style, resolved explicitly in the same priority order
 *  `cytoscape.ts`'s CSS-cascade style array applied its selectors in — base,
 *  then seed, then expandable, then truncated, then hidden-kind, then
 *  removed, then added, each layer's properties overriding the last for
 *  whatever it sets. G6 has no selector cascade of its own (a node's style
 *  is one callback returning one object), so the cascade has to be written
 *  out, not declared. */
export function resolveNodeStyle(
  datum: { readonly classes: string; readonly color?: string; readonly label: string },
  colors: StyleColors,
): Record<string, unknown> {
  const has = new Set(datum.classes.split(" "));
  let style: Record<string, unknown> = {
    size: 18,
    fill: datum.color,
    labelText: datum.label,
    labelFontSize: 11,
    labelFill: colors.text,
    labelPlacement: "bottom",
    labelOffsetY: 4,
  };
  if (has.has("seed")) {
    style = { ...style, size: 26, labelFontWeight: "bold" };
  }
  // A ring, not a colour: the expandable marker has to survive a reader
  // who cannot distinguish two hues.
  if (has.has("expandable")) {
    style = { ...style, stroke: colors.primary, lineWidth: 3, fillOpacity: 0.35 };
  }
  if (has.has("truncated")) {
    style = { ...style, stroke: colors.text, lineWidth: 3, lineDash: [4, 2] };
  }
  if (has.has("hidden-kind")) {
    style = { ...style, fill: colors.border };
  }
  // Removed nodes stay in the picture, marked by shape (resolveNodeShape)
  // *and* opacity rather than colour alone — a deletion shown only in red
  // is invisible to a reader who cannot see red.
  if (has.has("removed")) {
    style = { ...style, fillOpacity: 0.4, stroke: colors.text, lineWidth: 2, lineDash: [4, 2] };
  }
  return style;
}

/** An edge's drawing style, same cascade discipline as
 *  {@link resolveNodeStyle}. `endArrowType: 'triangle'` on the base style —
 *  not conditional — is what makes every edge directed by default;
 *  `source`/`target` are a directed fact and an undirected line would
 *  throw that away. */
export function resolveEdgeStyle(
  datum: { readonly classes: string; readonly label: string },
  colors: StyleColors,
): Record<string, unknown> {
  const has = new Set(datum.classes.split(" "));
  let style: Record<string, unknown> = {
    lineWidth: 1,
    stroke: colors.border,
    labelText: datum.label,
    labelFontSize: 9,
    labelFill: colors.text,
    labelBackground: true,
    labelBackgroundFill: colors.raised,
    labelBackgroundOpacity: 1,
    endArrow: true,
    endArrowType: "triangle",
    endArrowFill: colors.border,
  };
  // A conclusion, drawn as one. Dashed and tinted, so it is legible as
  // inferred without colour alone carrying the meaning — 00h requires a
  // state to survive being unable to tell two hues apart.
  if (has.has("derived")) {
    style = { ...style, lineDash: [4, 2], stroke: brand.cyan400, endArrowFill: brand.cyan400 };
  }
  if (has.has("removed")) {
    style = { ...style, lineDash: [4, 2], stroke: colors.text };
  }
  if (has.has("added")) {
    style = { ...style, lineWidth: 2, stroke: colors.primary };
  }
  return style;
}

/** The layout. **Radial, focused on the seed, and deterministic.** Replaces
 *  Cytoscape's `breadthfirst` — the property that mattered was never
 *  "breadthfirst" by name, it was "rings by undirected distance from the
 *  seed, settling the same way every run" (see `cytoscape.ts`'s own doc
 *  comment on `directed: false`). G6's `radial` layout (`focusNode`) is
 *  exactly that: rings by shortest path from a focus node, direction-
 *  agnostic, no force simulation deciding the ring assignment. Animation is
 *  disabled at the Graph level (`GraphCanvas.tsx`), not per-layout — the
 *  "nothing moves without the reader" guarantee applies to more than the
 *  layout, so it is set once, in one place. */
export function layoutOptions(seedId: string): Record<string, unknown> {
  return {
    type: "radial",
    focusNode: seedId,
    unitRadius: 90,
    preventOverlap: false,
  };
}

/** Whether the canvas should use WebGL. Unchanged reasoning from
 *  `cytoscape.ts` — see its own doc comment. */
export const WEBGL_THRESHOLD = 256;

export function wantsWebgl(nodeCount: number): boolean {
  return nodeCount >= WEBGL_THRESHOLD;
}

/** The cap on how far `.fitView()` may zoom a sparse graph. Unchanged
 *  reasoning from `cytoscape.ts` — see its own doc comment. */
export const MAX_ZOOM = 2;
