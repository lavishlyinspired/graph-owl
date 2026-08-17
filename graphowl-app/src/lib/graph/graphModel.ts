/** The model, as AntV G6 wants it.
 *
 *  `00f` requires graph tests to assert **the model, not the picture**, so
 *  the part of the renderer that can be wrong in a way a reader would act on
 *  lives here as a pure function: which nodes exist, which classes they
 *  carry, what style each class resolves to, and whether the layout is
 *  deterministic. What G6 paints from that is a drawing decision.
 *
 *  Ported from `ui/src/graph/graphModel.ts` (Plan 122a A3), **without**
 *  diff/time-travel support (`Change`/`DiffNode`/`DiffEdge`, "removed"/
 *  "added" classes) — A3's Explore opens on a seed and expands; comparing
 *  two instants of the same graph is a distinct capability with its own
 *  acceptance criteria the plan schedules separately, and porting the
 *  half-used diff machinery here would carry untested surface with no
 *  caller. Re-add it, from `ui/`'s original, if and when that capability is
 *  actually built. */

import type { AssetKind, GraphEdge, GraphNode } from "../api";
import { canvasLabel } from "./bidiLabel";

/** What the canvas is being asked to draw. */
export interface Picture {
  readonly seedId: string;
  readonly nodes: readonly GraphNode[];
  readonly edges: readonly GraphEdge[];
  /** Nodes already walked; the rest are drawn as expandable. */
  readonly expanded: readonly string[];
  /** Nodes hiding neighbours the budget cut. */
  readonly truncatedAt: readonly string[];
}

/** A stable id for an edge. Both endpoints and the relationship name are
 *  needed: `a contains b` and `a feeds b` are two facts about one pair, and
 *  an id built from the endpoints alone would collide them. */
export function edgeId(edge: GraphEdge): string {
  return `${edge.from}→${edge.to}→${edge.relationship}`;
}

/** Classes drive styling, and each one is a fact the reader can act on. */
export function nodeClasses(node: GraphNode, picture: Picture): string {
  const classes: string[] = [];
  if (node.id === picture.seedId) classes.push("seed");
  if (!picture.expanded.includes(node.id)) classes.push("expandable");
  if (picture.truncatedAt.includes(node.id)) classes.push("truncated");
  if (node.kind === null && !node.semanticType) classes.push("hidden-kind");
  return classes.join(" ");
}

export type ColorMode = "light" | "dark";

/** The dataviz skill's validated categorical palette, slots 1–5 in fixed
 *  order. */
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
  const kindEntries: LegendEntry[] = KIND_ORDER.filter((kind) => kindsPresent.has(kind)).map((kind) => ({
    key: kind,
    label: kind.charAt(0).toUpperCase() + kind.slice(1),
    color: kindColor(kind, mode, colors),
  }));

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

export function visiblePicture(picture: Picture, hidden: ReadonlySet<string>): Picture {
  return { ...picture, nodes: picture.nodes.filter((node) => !hidden.has(node.id)) };
}

/** A short, fixed-width glyph drawn inside the node circle — the mockup's
 *  own `INV`/`CO`/`GEO` badges read as the node's *own name*, shortened
 *  (`INV-1024` → `INV`), not a lookup keyed by kind or type — a
 *  domain-neutral catalog cannot know in advance which business-entity
 *  types or naming conventions a deployment will ever use (`00i` rule 3),
 *  so the glyph has to come from data every node already carries: its own
 *  label. Takes the first run of letters/digits (stopping at a separator
 *  like `-`, `_` or a space, matching `INV-1024` → `INV`) and uppercases
 *  up to three characters of it. */
export function nodeGlyph(name: string): string {
  const firstWord = /[A-Za-z0-9]+/.exec(name)?.[0] ?? "";
  return firstWord.slice(0, 3).toUpperCase() || "?";
}

function nodeColor(node: GraphNode, mode: ColorMode): string | undefined {
  if (node.kind !== null) return KIND_COLORS[mode][node.kind];
  if (node.semanticType) return semanticTypeColor(node.semanticType, mode);
  return undefined;
}

/** Classes on an edge. */
export function edgeClasses(edge: GraphEdge): string {
  return edge.derived === true ? "derived" : "";
}

/** The colours the style needs, named structurally rather than imported
 *  from a theme module directly — {@link "./graphColors"} supplies the
 *  literal values for each mode. */
export interface StyleColors {
  readonly text: string;
  readonly primary: string;
  readonly border: string;
  readonly raised: string;
}

// ---------------------------------------------------------------------
// Everything below this line is G6-shaped.
// ---------------------------------------------------------------------

export interface G6NodeDatum {
  readonly id: string;
  readonly type: "circle";
  readonly data: {
    readonly label: string;
    readonly glyph: string;
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

/** The whole picture as G6 data. An edge to a node not present in the
 *  picture is dropped rather than drawn to nowhere. */
export function toG6Data(picture: Picture, mode: ColorMode = "light"): G6Data {
  const present = new Set(picture.nodes.map((node) => node.id));

  const nodes: G6NodeDatum[] = picture.nodes.map((node) => {
    const classes = nodeClasses(node, picture);
    return {
      id: node.id,
      type: "circle",
      data: {
        label: canvasLabel(node.name),
        glyph: nodeGlyph(node.name),
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
        classes: edgeClasses(edge),
      },
    }));

  return { nodes, edges };
}

/** A node's drawing style, resolved explicitly in priority order — base,
 *  then seed, then expandable, then truncated, then hidden-kind. G6 has no
 *  selector cascade of its own (a node's style is one callback returning
 *  one object), so the cascade is written out, not declared.
 *
 *  An outlined circle with the kind's glyph lettered inside, name below —
 *  not a solid-filled dot — matching the delivered mockup's own node
 *  treatment: the colour reads as an *identity ring* around a legible
 *  abbreviation, rather than a flat swatch a colour-blind reader cannot
 *  tell apart from its neighbours by hue alone. */
export function resolveNodeStyle(
  datum: { readonly classes: string; readonly color?: string; readonly label: string; readonly glyph?: string },
  colors: StyleColors,
): Record<string, unknown> {
  const has = new Set(datum.classes.split(" "));
  const ring = datum.color ?? colors.border;
  let style: Record<string, unknown> = {
    size: 34,
    fill: colors.raised,
    fillOpacity: 0.6,
    stroke: ring,
    lineWidth: 1.5,
    icon: true,
    iconText: datum.glyph ?? "",
    iconFontFamily: "monospace",
    iconFontSize: 10,
    iconFill: ring,
    labelText: datum.label,
    labelFontSize: 11,
    labelFill: colors.text,
    labelPlacement: "bottom",
    labelOffsetY: 6,
  };
  if (has.has("seed")) {
    style = { ...style, size: 44, lineWidth: 2, labelFontWeight: "bold" };
  }
  // A thicker ring, not a colour: the expandable marker has to survive a
  // reader who cannot distinguish two hues.
  if (has.has("expandable")) {
    style = { ...style, lineWidth: 3 };
  }
  if (has.has("truncated")) {
    style = { ...style, stroke: colors.text, lineWidth: 3, lineDash: [4, 2] };
  }
  if (has.has("hidden-kind")) {
    style = { ...style, stroke: colors.border, iconFill: colors.border };
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
  // inferred without colour alone carrying the meaning — a state must
  // survive a reader unable to tell two hues apart.
  if (has.has("derived")) {
    style = { ...style, lineDash: [4, 2], stroke: colors.primary, endArrowFill: colors.primary };
  }
  return style;
}

/** The layout. **Radial, focused on the seed, and deterministic.** G6's
 *  `radial` layout (`focusNode`) draws rings by shortest path from a focus
 *  node, direction-agnostic, with no force simulation deciding the ring
 *  assignment — so the same picture settles the same way on every render.
 *  Animation is disabled at the Graph level (`GraphCanvas.tsx`), not
 *  per-layout — "nothing moves without the reader" applies to more than the
 *  layout, so it is set once, in one place. */
export function layoutOptions(seedId: string): Record<string, unknown> {
  return {
    type: "radial",
    focusNode: seedId,
    unitRadius: 90,
    preventOverlap: false,
  };
}

/** Whether the canvas should use WebGL. Constructing a WebGL renderer
 *  eagerly on every mount pays a context-creation cost even for the common
 *  small-neighbourhood case that never needs it, so the threshold exists to
 *  defer that cost until a picture is actually large enough to want it. */
export const WEBGL_THRESHOLD = 256;

export function wantsWebgl(nodeCount: number): boolean {
  return nodeCount >= WEBGL_THRESHOLD;
}

/** The cap on how far `.fitView()` may zoom a sparse graph — a two-node
 *  picture fit to fill the canvas would zoom in far enough that the nodes
 *  read as meaningless full-bleed circles. */
export const MAX_ZOOM = 2;
