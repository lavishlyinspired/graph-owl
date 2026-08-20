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

/** One entry of the **provenance key** — a line, not a swatch, because it
 *  describes how an edge is drawn rather than what colour a node is. */
export interface EdgeLegendEntry {
  readonly key: string;
  readonly label: string;
  readonly color: string;
  readonly dashed: boolean;
}

/** What a line's drawing means — the key to `resolveEdgeStyle`.
 *
 *  **Separate from `legendEntries` because it answers a different question,
 *  and because that one is empty for most real data.** `legendEntries` names
 *  the *node* kinds present, which requires a catalog `kind` or a pack-declared
 *  `semanticType`; a subject reached through `/graph/context` (a GST invoice,
 *  say) has neither, so the canvas that a reader spends most of their time on
 *  carried no key at all. This one is derived from the edge encoding itself,
 *  so it is present whenever anything is drawn.
 *
 *  **Both states are keyed whenever there are edges, rather than only the
 *  states present.** A legend is a key to an encoding, not a census of it:
 *  "dashed means inferred" is true of this canvas whether or not an inferred
 *  edge happens to be on screen, and it claims nothing about what is. Keying
 *  only what is present would also move the legend under the reader the moment
 *  an expansion returned a derived edge, and its position is part of how the
 *  canvas is read. An edgeless picture keys nothing — there is no encoding to
 *  explain, and a key over an edgeless canvas is furniture.
 *
 *  **`Contradicted` is deliberately absent**, though the design mock shows it:
 *  `GraphEdge` carries `derived` and nothing else, so there is no contradicted
 *  state for the canvas to draw or for this to key. Keying one would be a
 *  legend entry no edge can ever match. */
export function edgeLegendEntries(picture: Picture, colors: StyleColors): EdgeLegendEntry[] {
  if (picture.edges.length === 0) return [];
  return [
    { key: "asserted", label: "Asserted", color: colors.border, dashed: false },
    { key: "inferred", label: "Inferred", color: colors.inferred, dashed: true },
  ];
}

/** A brand-new node placed at the anchor's position, rather than left for
 *  the layout's own index-based default — what makes an expansion read as
 *  the new nodes **bubbling out of the node the reader clicked**, edges
 *  growing to their natural length from zero, instead of materializing
 *  wherever d3-force's default placement lands them and possibly jumping
 *  there from the opposite side of the canvas.
 *
 *  A node's own position becomes its collision/repulsion starting point for
 *  d3-force's simulation (`initializeNodes` only assigns its own index-based
 *  default when `x`/`y` are absent) *and* G6's "enter" transition's starting
 *  style — one seed serves both the physics and the animation, rather than
 *  needing to fake the animation independently of where the layout actually
 *  begins. */
export function withEntryPositions(
  data: G6Data,
  previous: G6Data,
  anchorId: string,
  anchorPosition: { readonly x: number; readonly y: number },
): G6Data {
  const existed = new Set(previous.nodes.map((node) => node.id));
  const nodes = data.nodes.map((node) =>
    node.id === anchorId || existed.has(node.id) ? node : { ...node, style: anchorPosition },
  );
  return { nodes, edges: data.edges };
}

export function visiblePicture(picture: Picture, hidden: ReadonlySet<string>): Picture {
  return { ...picture, nodes: picture.nodes.filter((node) => !hidden.has(node.id)) };
}

/** How much of a node's name the canvas draws before eliding.
 *
 *  **The canvas auto-fits, so only the ratio of label width to ring spacing
 *  matters — enlarging `unitRadius` alone buys nothing, because the fit
 *  zooms the text back down with it.** Both levers therefore move together:
 *  22 characters against a `unitRadius` of 280 is where the real GST
 *  identifiers (33 characters, six to a ring) stop colliding. A drawing
 *  budget, not a data limit — the full name is one click away in the detail
 *  panel. */
export const MAX_LABEL_CHARS = 22;

/** A name shortened to fit, eliding the middle rather than the tail.
 *
 *  **The tail is the part that distinguishes two nodes.** These names are
 *  identifiers — `books-19AABCP8087C1ZV-INV-MAR-006` — whose front names the
 *  family and whose back names the document. Two invoices from one supplier
 *  differ only in the last few characters, so a trailing ellipsis would draw
 *  them as the same node twice. */
export function shortLabel(name: string): string {
  if (name.length <= MAX_LABEL_CHARS) return name;
  // The ellipsis costs one of the budgeted characters; split what is left so
  // the tail keeps the larger half, since that is the distinguishing end.
  const keep = MAX_LABEL_CHARS - 1;
  const head = Math.floor(keep / 2);
  return `${name.slice(0, head)}\u2026${name.slice(name.length - (keep - head))}`;
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

/** What a node *is*, captioned under its name — its pack-declared semantic
 *  type, or its catalog kind, whichever it has. A node with neither is
 *  captioned with nothing at all: a placeholder like "unknown" would read as
 *  a class the ontology actually declares. */
export function nodeCaption(node: GraphNode): string | undefined {
  return node.semanticType ?? node.kind ?? undefined;
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
  /** The colour an inferred edge is drawn in — amber in both themes, kept
   *  distinct from `primary` so "the reasoner concluded this" never reads as
   *  "this is the thing you selected". */
  readonly inferred: string;
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
    readonly caption?: string;
    readonly color?: string;
    readonly classes: string;
  };
  /** Present only on a node just seeded by {@link withEntryPositions} — an
   *  explicit starting point for the layout and for G6's own enter
   *  animation, rather than wherever the layout's index-based default would
   *  otherwise place it. */
  readonly style?: { readonly x: number; readonly y: number };
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
        label: shortLabel(canvasLabel(node.name)),
        glyph: nodeGlyph(node.name),
        ...(nodeCaption(node) === undefined ? {} : { caption: nodeCaption(node) }),
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
  datum: {
    readonly classes: string;
    readonly color?: string;
    readonly label: string;
    readonly glyph?: string;
    readonly caption?: string;
  },
  colors: StyleColors,
): Record<string, unknown> {
  const has = new Set(datum.classes.split(" "));
  const ring = datum.color ?? colors.border;
  let style: Record<string, unknown> = {
    size: 34,
    fill: colors.raised,
    fillOpacity: 0.6,
    stroke: ring,
    // A hairline, as the design draws it. Anything heavier reads as a state
    // marker, and the states below are what earn the extra weight.
    lineWidth: 1,
    icon: true,
    iconText: datum.glyph ?? "",
    iconFontFamily: "monospace",
    iconFontSize: 10,
    iconFill: ring,
    labelText: datum.caption === undefined ? datum.label : `${datum.label}\n${datum.caption}`,
    labelFontSize: 11,
    labelFill: colors.text,
    labelPlacement: "bottom",
    labelOffsetY: 6,
  };
  if (has.has("seed")) {
    style = { ...style, size: 44, lineWidth: 2, labelFontWeight: "bold" };
  }
  // **`expandable` deliberately draws nothing.** In a freshly opened
  // neighbourhood almost every node is expandable, so a ring marker fired
  // nearly everywhere and made the whole canvas read as heavy. The hover
  // controls carry that affordance now, and say what the click does rather
  // than leaving it to be inferred from a border weight.
  if (has.has("truncated")) {
    style = { ...style, stroke: colors.text, lineWidth: 2, lineDash: [4, 2] };
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
    labelFontSize: 9.5,
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
    style = {
      ...style,
      lineDash: [6, 5],
      stroke: colors.inferred,
      endArrowFill: colors.inferred,
      labelFill: colors.inferred,
    };
  }
  return style;
}

/** The layout. **Concentric, ringed by degree, and deterministic.**
 *
 *  No force simulation decides anything here, so the same picture settles the
 *  same way on every render — verified by loading one neighbourhood twice and
 *  comparing the rendered canvas pixel for pixel, not assumed. Animation is
 *  disabled at the Graph level (`GraphCanvas.tsx`) rather than per-layout:
 *  "nothing moves without the reader" applies to more than the layout, so it
 *  is set once, in one place.
 *
 *  **This replaced `radial`, which took no seed argument by accident but by
 *  necessity.** `radial` centres on a named `focusNode`, which is exactly
 *  what was wanted — but on a star-shaped neighbourhood (a seed and its
 *  neighbours, which is every picture this canvas opens on) its stress solve
 *  converged with every neighbour at roughly the same angle, stacking them in
 *  a line down one side of the seed. `concentric` spaces a ring by angle
 *  directly, so the ring is a ring.
 *
 *  **The cost, stated plainly: the centre is now the most-connected node
 *  rather than the seed by name.** In a freshly opened neighbourhood these
 *  are the same node — every edge in the picture touches the seed. They can
 *  diverge once a reader expands a neighbour that turns out to be busier,
 *  and the centre will then shift to it. The seed stays identifiable
 *  regardless: `nodeClasses` marks it, and `resolveNodeStyle` draws it larger
 *  and bolder than anything else on the canvas. */
export function layoutOptions(): Record<string, unknown> {
  return {
    // https://g6.antv.antgroup.com/en/examples/layout/force-directed/#d3-force
    // — organic spacing from real physics rather than a fixed geometric
    // rule. `concentric` (this canvas's previous layout) placed every node
    // at a distance decided purely by its ring; a force layout spaces nodes
    // by how many neighbours are pulling on them, which reads better once a
    // few expansions have made the picture uneven.
    //
    // `link`/`manyBody`/`collide` as nested objects, not the flatter
    // `linkDistance`/`nodeStrength`/`preventOverlap` fields `@antv/layout`
    // also accepts — matching the reference example's own shape rather than
    // an equivalent it never demonstrates.
    type: "d3-force",
    link: {
      // The ideal edge length — enough room for a relationship label
      // (`onInvoice`, `recordedIn`) to sit on the line without touching
      // either endpoint's own two-line caption.
      distance: 160,
    },
    manyBody: {
      // Repulsion between every pair of nodes, not only connected ones —
      // negative is repulsive in d3-force's convention. This is what keeps
      // an unrelated branch of the graph from drifting into another's space.
      strength: -220,
    },
    collide: {
      // Sized to the *label*, not the circle — the same lesson `concentric`
      // needed: a node is 34–44px across but its two-line caption runs
      // wider, and a collision radius sized to the circle let two labels
      // overlap even while their circles stayed clear.
      radius: 90,
    },
    // Faster than d3-force's own default (`0.028`, tuned for 300 ticks) — a
    // neighbourhood here is a handful of nodes, not the thousand-node graphs
    // that default was chosen for, and it settles in well under a second at
    // this rate instead of several.
    alphaDecay: 0.05,
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
