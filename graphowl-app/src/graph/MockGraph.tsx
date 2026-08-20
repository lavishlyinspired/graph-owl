/** Static SVG + HTML graph matching the HTML mockup's explore view exactly.
 *
 *  Shown when the backend has no data (zero findings, zero assets) so the
 *  explore page is not a blank screen.  Every coordinate, glyph, colour and
 *  edge style comes from the mockup's `nodes`/`edges`/`edgeLabels`/`legend`
 *  arrays.  The graph is not interactive beyond the node click → navigate
 *  because there is no backend to query — it is a visual anchor, not a
 *  live exploration surface. */

import { useMemo } from "react";
import { Link } from "react-router-dom";
import { resolveInitialTheme } from "../lib/theme";

/* ------------------------------------------------------------------ */
/*  Data — lifted verbatim from the mockup (lines 2982-3013).         */
/*  CSS var() refs map to our --gowl-* tokens so dark/light works.    */
/* ------------------------------------------------------------------ */

interface MockNode {
  readonly left: string;
  readonly top: string;
  readonly size: string;
  readonly fill: string;
  readonly border: string;
  readonly glow: string;
  readonly glyph: string;
  readonly glyphColor: string;
  readonly label: string;
  readonly type: string;
  readonly labelColor: string;
  /** Route to navigate to on click (relative path). */
  readonly to: string;
}

interface MockEdge {
  readonly x1: string;
  readonly y1: string;
  readonly x2: string;
  readonly y2: string;
  readonly stroke: string;
  readonly w: number;
  readonly dash: string;
}

interface MockEdgeLabel {
  readonly left: string;
  readonly top: string;
  readonly text: string;
  readonly color: string;
}

interface LegendEntry {
  readonly label: string;
  readonly line: string;
}

function darkNodes(): readonly MockNode[] {
  return [
    { left: "44%", top: "46%", size: "56px", fill: "#12262a", border: "2px solid #5cc7d8", glow: "0 0 0 6px rgba(92,199,216,0.1)", glyph: "ORG", glyphColor: "#5cc7d8", label: "Supplier ABC", type: "gst:Supplier", labelColor: "#ffffff", to: "/entity/gst:Supplier/27AABCU9603R1ZM" },
    { left: "20%", top: "20%", size: "40px", fill: "#141920", border: "1px solid #2b323c", glow: "none", glyph: "DOC", glyphColor: "#8b939f", label: "GST Return Jul", type: "gst:Filing", labelColor: "#c2c9d2", to: "/entity/gst:Filing/2026-07" },
    { left: "20%", top: "72%", size: "40px", fill: "#141920", border: "1px solid #2b323c", glow: "none", glyph: "SRC", glyphColor: "#8b939f", label: "ERP Master", type: "src:Table", labelColor: "#c2c9d2", to: "/entity/src:Table/erp.supplier_master" },
    { left: "70%", top: "22%", size: "44px", fill: "#141920", border: "1px solid #2b323c", glow: "none", glyph: "INV", glyphColor: "#8b939f", label: "INV-1024", type: "gst:Invoice", labelColor: "#c2c9d2", to: "/entity/gst:Invoice/INV-1024" },
    { left: "72%", top: "58%", size: "40px", fill: "#1d1810", border: "1px dashed #d3a35c", glow: "none", glyph: "GEO", glyphColor: "#d3a35c", label: "Maharashtra", type: "geo:State", labelColor: "#d3a35c", to: "/entity/geo:State/Maharashtra" },
    { left: "52%", top: "80%", size: "38px", fill: "#141920", border: "1px solid #2b323c", glow: "none", glyph: "PAN", glyphColor: "#8b939f", label: "AABCU9603R", type: "gst:PAN", labelColor: "#c2c9d2", to: "/entity/gst:PAN/AABCU9603R" },
    { left: "88%", top: "40%", size: "34px", fill: "#141920", border: "1px solid #2b323c", glow: "none", glyph: "CO", glyphColor: "#8b939f", label: "Company XYZ", type: "org:Company", labelColor: "#c2c9d2", to: "/entity/org:Company/XYZ" },
  ];
}

function lightNodes(): readonly MockNode[] {
  return [
    { left: "44%", top: "46%", size: "56px", fill: "#e6f4f7", border: "2px solid #0f7f92", glow: "0 0 0 6px rgba(15,127,146,0.12)", glyph: "ORG", glyphColor: "#0f7f92", label: "Supplier ABC", type: "gst:Supplier", labelColor: "#1b1a17", to: "/entity/gst:Supplier/27AABCU9603R1ZM" },
    { left: "20%", top: "20%", size: "40px", fill: "#f4f2ee", border: "1px solid #d0cbc1", glow: "none", glyph: "DOC", glyphColor: "#6b665d", label: "GST Return Jul", type: "gst:Filing", labelColor: "#3d3a34", to: "/entity/gst:Filing/2026-07" },
    { left: "20%", top: "72%", size: "40px", fill: "#f4f2ee", border: "1px solid #d0cbc1", glow: "none", glyph: "SRC", glyphColor: "#6b665d", label: "ERP Master", type: "src:Table", labelColor: "#3d3a34", to: "/entity/src:Table/erp.supplier_master" },
    { left: "70%", top: "22%", size: "44px", fill: "#f4f2ee", border: "1px solid #d0cbc1", glow: "none", glyph: "INV", glyphColor: "#6b665d", label: "INV-1024", type: "gst:Invoice", labelColor: "#3d3a34", to: "/entity/gst:Invoice/INV-1024" },
    { left: "72%", top: "58%", size: "40px", fill: "#fdf4e6", border: "1px dashed #a3641a", glow: "none", glyph: "GEO", glyphColor: "#a3641a", label: "Maharashtra", type: "geo:State", labelColor: "#a3641a", to: "/entity/geo:State/Maharashtra" },
    { left: "52%", top: "80%", size: "38px", fill: "#f4f2ee", border: "1px solid #d0cbc1", glow: "none", glyph: "PAN", glyphColor: "#6b665d", label: "AABCU9603R", type: "gst:PAN", labelColor: "#3d3a34", to: "/entity/gst:PAN/AABCU9603R" },
    { left: "88%", top: "40%", size: "34px", fill: "#f4f2ee", border: "1px solid #d0cbc1", glow: "none", glyph: "CO", glyphColor: "#6b665d", label: "Company XYZ", type: "org:Company", labelColor: "#3d3a34", to: "/entity/org:Company/XYZ" },
  ];
}

function darkEdges(): readonly MockEdge[] {
  return [
    { x1: "20%", y1: "20%", x2: "44%", y2: "46%", stroke: "#39414c", w: 1.4, dash: "0" },
    { x1: "20%", y1: "72%", x2: "44%", y2: "46%", stroke: "#39414c", w: 1.4, dash: "0" },
    { x1: "44%", y1: "46%", x2: "70%", y2: "22%", stroke: "#5cc7d8", w: 1.8, dash: "0" },
    { x1: "44%", y1: "46%", x2: "72%", y2: "58%", stroke: "#d3a35c", w: 1.6, dash: "6 5" },
    { x1: "44%", y1: "46%", x2: "52%", y2: "80%", stroke: "#39414c", w: 1.4, dash: "2 4" },
    { x1: "70%", y1: "22%", x2: "88%", y2: "40%", stroke: "#39414c", w: 1.2, dash: "0" },
    { x1: "52%", y1: "80%", x2: "88%", y2: "40%", stroke: "#39414c", w: 1.2, dash: "2 4" },
  ];
}

function lightEdges(): readonly MockEdge[] {
  return [
    { x1: "20%", y1: "20%", x2: "44%", y2: "46%", stroke: "#bdb7ab", w: 1.4, dash: "0" },
    { x1: "20%", y1: "72%", x2: "44%", y2: "46%", stroke: "#bdb7ab", w: 1.4, dash: "0" },
    { x1: "44%", y1: "46%", x2: "70%", y2: "22%", stroke: "#0f7f92", w: 1.8, dash: "0" },
    { x1: "44%", y1: "46%", x2: "72%", y2: "58%", stroke: "#a3641a", w: 1.6, dash: "6 5" },
    { x1: "44%", y1: "46%", x2: "52%", y2: "80%", stroke: "#bdb7ab", w: 1.4, dash: "2 4" },
    { x1: "70%", y1: "22%", x2: "88%", y2: "40%", stroke: "#bdb7ab", w: 1.2, dash: "0" },
    { x1: "52%", y1: "80%", x2: "88%", y2: "40%", stroke: "#bdb7ab", w: 1.2, dash: "2 4" },
  ];
}

function darkEdgeLabels(): readonly MockEdgeLabel[] {
  return [
    { left: "32%", top: "31%", text: "declaredIn", color: "#76808e" },
    { left: "32%", top: "60%", text: "masteredAs", color: "#76808e" },
    { left: "57%", top: "32%", text: "supplied \u00b7 0.96", color: "#5cc7d8" },
    { left: "59%", top: "53%", text: "locatedIn \u00b7 0.84 \u2227", color: "#d3a35c" },
    { left: "46%", top: "65%", text: "hasPAN", color: "#76808e" },
    { left: "80%", top: "29%", text: "filedBy", color: "#76808e" },
  ];
}

function lightEdgeLabels(): readonly MockEdgeLabel[] {
  return [
    { left: "32%", top: "31%", text: "declaredIn", color: "#7c766c" },
    { left: "32%", top: "60%", text: "masteredAs", color: "#7c766c" },
    { left: "57%", top: "32%", text: "supplied \u00b7 0.96", color: "#0f7f92" },
    { left: "59%", top: "53%", text: "locatedIn \u00b7 0.84 \u2227", color: "#a3641a" },
    { left: "46%", top: "65%", text: "hasPAN", color: "#7c766c" },
    { left: "80%", top: "29%", text: "filedBy", color: "#7c766c" },
  ];
}

function darkLegend(): readonly LegendEntry[] {
  return [
    { label: "Asserted", line: "1.5px solid #5cc7d8" },
    { label: "Inferred", line: "1.5px dashed #d3a35c" },
    { label: "Derived", line: "1.5px dotted #8b939f" },
    { label: "Contradicted", line: "1.5px double #b8543f" },
  ];
}

function lightLegend(): readonly LegendEntry[] {
  return [
    { label: "Asserted", line: "1.5px solid #0f7f92" },
    { label: "Inferred", line: "1.5px dashed #a3641a" },
    { label: "Derived", line: "1.5px dotted #6b665d" },
    { label: "Contradicted", line: "1.5px double #9c3a24" },
  ];
}

const PINNED = ["gst:2026-07", "erp.supplier_master", "gst:Invoice/INV-1024"];

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

export function MockGraph() {
  const mode = useMemo(() => resolveInitialTheme(), []);
  const dark = mode === "dark";

  const nodes = dark ? darkNodes() : lightNodes();
  const edges = dark ? darkEdges() : lightEdges();
  const edgeLabels = dark ? darkEdgeLabels() : lightEdgeLabels();
  const legend = dark ? darkLegend() : lightLegend();

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Toolbar */}
      <div className="flex h-[46px] flex-none items-center gap-2.5 border-b border-gowl-line bg-gowl-panel px-4">
        <span className="text-[14.5px] font-semibold text-gowl-t1">Explore</span>
        <span className="text-gowl-dim">/</span>
        <span className="font-mono text-[13px] text-gowl-accent">gst:Supplier/27AABCU9603R1ZM</span>
        <div className="ml-auto flex items-center gap-1.5">
          {["Relationship: all", "Confidence \u2265 0.5", "Depth 2", "Filters"].map((f) => (
            <span key={f} className="cursor-pointer rounded-md border border-gowl-line-2 px-2.5 py-1 text-[13px] text-gowl-t4 hover:border-gowl-hover hover:text-gowl-t1">
              {f}<span className="text-gowl-t8">{"\u25BE"}</span>
            </span>
          ))}
          <span className="cursor-pointer rounded-md bg-gowl-accent px-2.5 py-1 text-[13px] font-semibold text-gowl-accent-on">
            Save investigation
          </span>
        </div>
      </div>

      {/* Graph canvas */}
      <div className="relative min-h-0 flex-1" style={{ backgroundImage: "radial-gradient(var(--gowl-track) 1px, transparent 1px)", backgroundSize: "26px 26px" }}>

        {/* Investigation badge */}
        <div className="absolute left-4 top-4 rounded-lg border border-gowl-line bg-gowl-panel p-2.5" style={{ maxWidth: 250 }}>
          <div className="mb-1.5 font-mono text-[11px] tracking-widest text-gowl-t6">INVESTIGATION &middot; FROM RECO NOW</div>
          <div className="mb-2 text-[13.5px] text-gowl-t2">INV-1025 &middot; Aug amount mismatch</div>
          <div className="flex flex-wrap gap-1.5">
            {PINNED.map((p) => (
              <span key={p} className="rounded border border-gowl-line-2 bg-gowl-row px-1.5 py-0.5 font-mono text-[11px] text-gowl-t4">{p}</span>
            ))}
          </div>
        </div>

        {/* Zoom controls */}
        <div className="absolute right-4 top-4 flex flex-col gap-px overflow-hidden rounded-md border border-gowl-line" style={{ background: "var(--gowl-line)" }}>
          <button type="button" className="flex h-7 w-7 items-center justify-center bg-gowl-panel text-[14.5px] text-gowl-t5 hover:text-gowl-t1">+</button>
          <button type="button" className="flex h-7 w-7 items-center justify-center bg-gowl-panel text-[14.5px] text-gowl-t5 hover:text-gowl-t1">&minus;</button>
          <button type="button" className="flex h-7 w-7 items-center justify-center bg-gowl-panel text-[12.5px] text-gowl-t5 hover:text-gowl-t1">⤢</button>
        </div>

        {/* Legend */}
        <div className="absolute bottom-4 left-4 flex gap-4 rounded-lg border border-gowl-line bg-gowl-panel px-3.5 py-2.5">
          {legend.map((g) => (
            <div key={g.label} className="flex items-center gap-1.5 text-[12.5px] text-gowl-t5">
              <span aria-hidden className="inline-block w-[18px] border-t" style={{ borderTop: g.line }} />
              {g.label}
            </div>
          ))}
        </div>

        {/* SVG edges + HTML nodes */}
        <div className="absolute inset-0">
          {/* Edges as SVG lines */}
          <svg className="absolute inset-0 h-full w-full">
            {edges.map((e, i) => (
              <line
                key={i}
                x1={e.x1}
                y1={e.y1}
                x2={e.x2}
                y2={e.y2}
                style={{ stroke: e.stroke, strokeWidth: e.w, strokeDasharray: e.dash }}
              />
            ))}
          </svg>

          {/* Edge labels */}
          {edgeLabels.map((l, i) => (
            <div
              key={`el-${i}`}
              className="pointer-events-none absolute whitespace-nowrap rounded bg-gowl-bg px-1.5 py-px font-mono text-[11px] tracking-wide"
              style={{ left: l.left, top: l.top, transform: "translate(-50%, -50%)", color: l.color }}
            >
              {l.text}
            </div>
          ))}

          {/* Nodes */}
          {nodes.map((n) => (
            <Link
              key={n.label}
              to={n.to}
              className="absolute flex flex-col items-center gap-1.5"
              style={{ left: n.left, top: n.top, transform: "translate(-50%, -50%)", cursor: "pointer" }}
            >
              <div
                className="flex items-center justify-center rounded-full font-mono text-[11.5px]"
                style={{ width: n.size, height: n.size, background: n.fill, border: n.border, boxShadow: n.glow, color: n.glyphColor }}
              >
                {n.glyph}
              </div>
              <div className="text-center whitespace-nowrap">
                <div className="text-[13px] font-medium" style={{ color: n.labelColor }}>{n.label}</div>
                <div className="font-mono text-[11px] text-gowl-t7">{n.type}</div>
              </div>
            </Link>
          ))}
        </div>
      </div>
    </div>
  );
}
