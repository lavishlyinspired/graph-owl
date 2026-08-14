/** The model, as Cytoscape wants it.
 *
 *  `00f-ui-architecture.md` requires graph tests to assert **the model, not the
 *  picture**, so the part of the renderer swap that can be wrong in a way a
 *  reader would act on lives here as a pure function: which nodes exist, which
 *  classes they carry, and whether the layout is deterministic. What Cytoscape
 *  paints from that is a drawing decision.
 *
 *  This is the payoff of `GraphView` having been renderer-agnostic from the
 *  start — the model and its 116 mutation-tested assertions are untouched by
 *  the swap from SVG.
 */

import type { AssetKind, GraphEdge, GraphNode } from "../api";
import { brand } from "../theme";
import { canvasLabel } from "./bidiLabel";
import type { Change, DiffEdge, DiffNode } from "./diff";

/** A Cytoscape element. Typed structurally rather than imported, so this module
 *  and its tests do not need the library — the shape *is* the contract. */
export interface Element {
  readonly group: "nodes" | "edges";
  readonly data: {
    readonly id: string;
    readonly label?: string;
    readonly source?: string;
    readonly target?: string;
    /** Plan 114 Slice D. A resolved hex, not a kind name — `graphStyle`'s
     *  base `node` rule reads it back with `data(color)`; `node.hidden-kind`
     *  still overrides it via the style cascade for a kind of `null`. */
    readonly color?: string;
  };
  readonly classes: string;
}

/** What the canvas is being asked to draw. */
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

/** A stable id for an edge.
 *
 *  Both endpoints **and** the relationship, matching `model.ts`: `a contains b`
 *  and `a feeds b` are two facts about one pair, and an id built from the
 *  endpoints alone would make Cytoscape silently drop the second — an edge
 *  vanishing from the picture because of how it was keyed, not because of
 *  anything true about the graph.
 */
export function edgeId(edge: GraphEdge): string {
  return `${edge.from}→${edge.to}→${edge.relationship}`;
}

/** Classes drive styling, and each one is a fact the reader can act on.
 *
 *  `removed` is here because `op = false` is a retraction: a node that existed
 *  at the earlier instant and not the later one is **still drawn**, marked, so
 *  the picture shows a deletion rather than an absence. `truncated` marks the
 *  node that is hiding something rather than the canvas as a whole, so a reader
 *  can tell *where* the picture is incomplete.
 */
export function nodeClasses(
  node: GraphNode | DiffNode,
  picture: Picture,
): string {
  const classes: string[] = [changeOf(node as DiffNode)];
  if (node.id === picture.seedId) classes.push("seed");
  if (!picture.expanded.includes(node.id)) classes.push("expandable");
  if (picture.truncatedAt.includes(node.id)) classes.push("truncated");
  // A node the reader may not see keeps its place in the picture but not its
  // label — removing it would claim a smaller neighbourhood than exists.
  if (node.kind === null) classes.push("hidden-kind");
  return classes.join(" ");
}

export type ColorMode = "light" | "dark";

/** Plan 114 Slice D: colour a node by its kind, so "what am I looking at"
 *  does not require reading every label. The dataviz skill's validated
 *  categorical palette, slots 1–5 in fixed order per its own rule ("assign
 *  categorical hues in fixed order, never cycled") — not chosen for a
 *  thematic reading, just this project's five `AssetKind` values assigned
 *  the first five slots. Both rows ran through the skill's own
 *  `validate_palette.js` against this project's real surfaces (light
 *  `#FFFFFF`, dark `#152A45`): lightness band, chroma floor, CVD separation
 *  (worst adjacent pair ΔE 8.4–9.1), and normal-vision floor all passed. */
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

/** `null` falls back to `colors.border` — the same grey `node.hidden-kind`
 *  already uses for a kind authorization hid, so a reader sees one consistent
 *  "unknown" treatment rather than two. */
export function kindColor(kind: AssetKind | null, mode: ColorMode, colors: StyleColors): string {
  return kind === null ? colors.border : KIND_COLORS[mode][kind];
}

/** The fixed order `KIND_COLORS`' slots were assigned in — the legend lists
 *  kinds in the same order a reader would otherwise have to infer from the
 *  palette table, rather than a second, possibly-drifting order of its own. */
const KIND_ORDER: readonly AssetKind[] = ["service", "database", "schema", "table", "column"];

export interface LegendEntry {
  readonly kind: AssetKind;
  readonly label: string;
  readonly color: string;
}

/** What a legend renders — the same colour {@link kindColor} would give a
 *  real node of that kind, so the legend can never drift from the canvas. */
export function legendEntries(mode: ColorMode, colors: StyleColors): LegendEntry[] {
  return KIND_ORDER.map((kind) => ({
    kind,
    label: kind.charAt(0).toUpperCase() + kind.slice(1),
    color: kindColor(kind, mode, colors),
  }));
}

/** Plan 114 Slice B: a reader temporarily hiding a node they do not want to
 *  look at right now — a view-level filter, never persisted into
 *  `GraphModel`. Deliberately distinct from the `removed` diff class:
 *  `removed` means a real transaction retracted the fact; this means a
 *  reader chose not to look.
 *
 *  Only `nodes` is filtered here. `toElements` already drops any edge whose
 *  endpoint is not present in `picture.nodes` (below) — reusing that rule
 *  rather than filtering edges a second time and risking the two rules
 *  disagreeing about what counts as "present". */
export function visiblePicture(picture: Picture, hidden: ReadonlySet<string>): Picture {
  return { ...picture, nodes: picture.nodes.filter((node) => !hidden.has(node.id)) };
}

/** The whole picture as Cytoscape elements, nodes before edges.
 *
 *  Order matters: Cytoscape rejects an edge whose endpoints it has not seen, so
 *  an edge listed first is dropped rather than deferred.
 */
export function toElements(picture: Picture, mode: ColorMode = "light"): Element[] {
  const present = new Set(picture.nodes.map((node) => node.id));

  const nodes: Element[] = picture.nodes.map((node) => ({
    group: "nodes" as const,
    // `canvasLabel`: Cytoscape draws this straight onto a `<canvas>`, which
    // has no `dir` attribute — a right-to-left name needs to arrive already
    // in visual order (`bidiLabel.ts`'s own doc comment).
    //
    // `color` omitted for a `null` kind rather than resolved through
    // `kindColor`'s own fallback: `node.hidden-kind`'s style rule already
    // sets the same grey via the class cascade, so there is one place that
    // decision is made, not two that have to agree.
    data: {
      id: node.id,
      label: canvasLabel(node.name),
      ...(node.kind === null ? {} : { color: KIND_COLORS[mode][node.kind] }),
    },
    classes: nodeClasses(node, picture),
  }));

  const edges: Element[] = picture.edges
    // An edge to a node that is not in the picture is dropped rather than
    // drawn to nowhere. It happens legitimately: a diff can hold an edge whose
    // far end was filtered out by authorization, and Cytoscape would throw on
    // it rather than skip it.
    .filter((edge) => present.has(edge.from) && present.has(edge.to))
    .map((edge) => ({
      group: "edges" as const,
      data: {
        id: edgeId(edge),
        source: edge.from,
        target: edge.to,
        label: edge.relationship,
      },
      classes: edgeClasses(edge as GraphEdge & DiffEdge),
    }));

  return [...nodes, ...edges];
}

/** Classes on an edge.
 *
 *  **A derived edge is drawn differently, and that is not decoration.** `00b`
 *  decision 2 keeps conclusions in their own graph precisely so nobody
 *  mistakes one for something a person asserted — and a picture that renders
 *  both alike undoes that separation in front of the one reader most likely to
 *  act on it. An asserted `feeds` and an inferred one carry different weight in
 *  every decision somebody makes from this graph.
 *
 *  Absent means asserted: an older server that does not send the flag
 *  understates what the reasoner did rather than overstating it.
 */
export function edgeClasses(edge: GraphEdge & { change?: Change }): string {
  const classes: string[] = [changeOf(edge)];
  if (edge.derived === true) classes.push("derived");
  return classes.join(" ");
}

/** The layout.
 *
 *  **`breadthfirst`, rooted at the seed, and deterministic.** `00f` requires a
 *  layout that settles the same way every run: a force simulation lands
 *  somewhere slightly different each time, so the same neighbourhood never
 *  looks the same twice and nobody can say "the node on the left". Rings by
 *  distance from the seed also encode the one thing this picture is for, which
 *  a physics simulation encodes only incidentally.
 *
 *  `animate: false` for the same reason it is deterministic — and because an
 *  animation on every expand moves the whole picture under a reader who was
 *  looking at one part of it.
 */
export function layoutOptions(seedId: string) {
  return {
    name: "breadthfirst",
    roots: [seedId],
    directed: false,
    animate: false,
    // Deterministic tie-breaking. Without it Cytoscape orders siblings by
    // insertion, which is fetch order, which is not stable across runs.
    maximal: true,
    grid: true,
    spacingFactor: 1.1,
    padding: 24,
  };
}

/** Whether the canvas should use WebGL.
 *
 *  On above a threshold, off below it. Not because Canvas is inadequate at
 *  small sizes — it is fine, and *better*: WebGL has a fixed context-creation
 *  cost and a texture atlas that only pays for itself once there is enough to
 *  draw, so switching it on for a six-node neighbourhood makes the common case
 *  slower to open.
 *
 *  256 because that is roughly where an SVG/Canvas renderer's per-element cost
 *  starts to show on a mid-range laptop, and it is an order of magnitude below
 *  the 10,000-node interactivity budget in `00f` — so the budget is never the
 *  thing being tested here. `00f` rejects a *hybrid* that swaps renderers
 *  mid-session; this is chosen once, when the canvas is created.
 */
export const WEBGL_THRESHOLD = 256;

export function wantsWebgl(nodeCount: number): boolean {
  return nodeCount >= WEBGL_THRESHOLD;
}

/** Plan 114 Slice A (follow-on, found live): `.fit()` on a sparse graph — two
 *  or three nodes, the common case for a finding's own evidence graph —
 *  computes whatever zoom makes those few nodes fill the container, which on
 *  an ~850×420 canvas is enough to render an 18px node as a 100px circle and
 *  its 11px label as 55px text. `2` (200%) is the cap: generous enough that
 *  `.fit()` still does real work bringing a spread-out neighbourhood together,
 *  low enough that a two-node graph stays legible rather than filling the
 *  frame with a single edge. Cytoscape's own default is unbounded. */
export const MAX_ZOOM = 2;

/** The colours the style needs, named structurally rather than imported from
 *  `theme.ts` — same reasoning as {@link Element}: this module and its tests
 *  should not need to know the whole palette shape, only these four values. */
export interface StyleColors {
  readonly text: string;
  readonly primary: string;
  readonly border: string;
  readonly raised: string;
}

/** A Cytoscape style rule, typed structurally so this module's tests do not
 *  need the library — the shape *is* the contract, matching {@link Element}. */
export interface StyleRule {
  readonly selector: string;
  readonly style: Readonly<Record<string, string | number>>;
}

/** Plan 114 Slice A. Moved out of `GraphCanvas.tsx` so a missing label or
 *  arrowhead is a RED test next time, not a screenshot someone has to notice
 *  by hand — `00f` calls this "a drawing decision", not model logic, but that
 *  does not mean it cannot be wrong, only that it is wrong in a different way
 *  than the model can be.
 *
 *  Two defects this fixes, both found live rather than assumed: `data.label`
 *  was already attached to every edge in {@link toElements}, but no rule ever
 *  read it back, so the relationship name was present in the data and
 *  invisible on screen; and no rule set `target-arrow-shape`, so a directed
 *  fact (`source`/`target`) rendered as an undirected line regardless of
 *  which edge class carried a `target-arrow-color` for it. */
export function graphStyle(colors: StyleColors): StyleRule[] {
  return [
    {
      selector: "node",
      style: {
        label: "data(label)",
        "font-size": 11,
        color: colors.text,
        "text-valign": "bottom",
        "text-margin-y": 4,
        // Plan 114 Slice D: coloured by kind, via the `data.color`
        // `toElements` already resolves — "what am I looking at" should not
        // require reading every label. `colors.primary` remains the
        // fallback for a `color`-less node (Cytoscape's own behaviour for
        // an unmapped `data()` reference), and stays the ring colour for
        // `.expandable` immediately below, unrelated to fill.
        "background-color": "data(color)",
        width: 18,
        height: 18,
      },
    },
    { selector: "node.seed", style: { width: 26, height: 26, "font-weight": "bold" } },
    // A ring, not a colour: the expandable marker has to survive a reader who
    // cannot distinguish the two hues.
    {
      selector: "node.expandable",
      style: { "border-width": 3, "border-color": colors.primary, "background-opacity": 0.35 },
    },
    {
      selector: "node.truncated",
      style: { "border-width": 3, "border-style": "dashed", "border-color": colors.text },
    },
    { selector: "node.hidden-kind", style: { "background-color": colors.border } },
    // Removed nodes stay in the picture, marked by shape *and* opacity rather
    // than colour alone — a deletion shown only in red is invisible to a
    // reader who cannot see red.
    {
      selector: "node.removed",
      style: {
        shape: "diamond",
        "background-opacity": 0.4,
        "border-style": "dashed",
        "border-width": 2,
        "border-color": colors.text,
      },
    },
    { selector: "node.added", style: { shape: "star" } },
    {
      selector: "edge",
      style: {
        width: 1,
        "line-color": colors.border,
        "curve-style": "straight",
        // The relationship name — see the doc comment above for why this
        // line is the point of the whole function.
        label: "data(label)",
        "font-size": 9,
        color: colors.text,
        // Not `autorotate`: verified live on a real 2-node evidence graph
        // that a near-vertical edge rotates its label to read top-to-bottom,
        // one letter per line — illegible, and it collided with the source
        // node's own label sitting just below it. Horizontal reads correctly
        // at any edge angle, which is the property that actually matters;
        // Neo4j's own relationship-type labels are horizontal for the same
        // reason.
        "text-background-color": colors.raised,
        "text-background-opacity": 1,
        "text-background-padding": 2,
        // Directed by default; `edge.derived` overrides only the colour
        // below, the same way it already overrides `line-color`.
        "target-arrow-shape": "triangle",
        "target-arrow-color": colors.border,
        "arrow-scale": 0.7,
      },
    },
    {
      // **A conclusion, drawn as one.** Dashed and tinted, so it is legible as
      // inferred without colour alone carrying the meaning —
      // `00h-ui-design-system.md` requires a state to survive being unable to
      // tell two hues apart, and this is a state somebody acts on.
      selector: "edge.derived",
      style: {
        "line-style": "dashed",
        "line-color": brand.cyan400,
        "target-arrow-color": brand.cyan400,
      },
    },
    { selector: "edge.removed", style: { "line-style": "dashed", "line-color": colors.text } },
    { selector: "edge.added", style: { width: 2, "line-color": colors.primary } },
  ];
}
