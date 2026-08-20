import { strings } from "../lib/strings";

/** Mockup's analytics screen — bespoke layout with KPI sparklines,
 *  stacked bar chart, funnel, heatmap, and model spend breakdown.
 *  All data is mock, matching the mockup's `V` object exactly. */

interface SparkBar {
  readonly height: string;
  readonly color: string;
}

interface KpiCard {
  readonly label: string;
  readonly value: string;
  readonly delta: string;
  readonly sub: string;
  readonly spark: readonly SparkBar[];
}

const KPI_CARDS: readonly KpiCard[] = [
  {
    label: "FACTS IN GRAPH",
    value: "1.84 M",
    delta: "+63 k",
    sub: "this month",
    spark: [
      { height: "54%", color: "var(--gowl-accent)" },
      { height: "62%", color: "var(--gowl-accent)" },
      { height: "71%", color: "var(--gowl-accent)" },
      { height: "78%", color: "var(--gowl-accent)" },
      { height: "100%", color: "var(--gowl-accent)" },
    ],
  },
  {
    label: "INFERENCE YIELD",
    value: "1.54×",
    delta: "+0.11",
    sub: "inferred per asserted fact",
    spark: [
      { height: "70%", color: "var(--gowl-accent)" },
      { height: "74%", color: "var(--gowl-accent)" },
      { height: "79%", color: "var(--gowl-accent)" },
      { height: "84%", color: "var(--gowl-accent)" },
      { height: "100%", color: "var(--gowl-accent)" },
    ],
  },
  {
    label: "CONTRADICTION HALF-LIFE",
    value: "5.2 d",
    delta: "−3.1 d",
    sub: "time to a human decision",
    spark: [
      { height: "100%", color: "var(--gowl-amber)" },
      { height: "88%", color: "var(--gowl-amber)" },
      { height: "72%", color: "var(--gowl-amber)" },
      { height: "61%", color: "var(--gowl-amber)" },
      { height: "42%", color: "var(--gowl-amber)" },
    ],
  },
  {
    label: "EVIDENCE COVERAGE",
    value: "96.2%",
    delta: "+2.4 pts",
    sub: "facts with a document",
    spark: [
      { height: "72%", color: "var(--gowl-ok)" },
      { height: "80%", color: "var(--gowl-ok)" },
      { height: "86%", color: "var(--gowl-ok)" },
      { height: "91%", color: "var(--gowl-ok)" },
      { height: "100%", color: "var(--gowl-ok)" },
    ],
  },
];

interface GrowthColumn {
  readonly week: string;
  readonly segments: readonly { readonly height: string; readonly color: string }[];
  readonly total: string;
}

const GROWTH_DATA: readonly GrowthColumn[] = [
  { week: "W28", segments: [{ height: "34%", color: "var(--gowl-accent)" }, { height: "20%", color: "var(--gowl-amber)" }, { height: "8%", color: "var(--gowl-t7)" }], total: "41 k" },
  { week: "W29", segments: [{ height: "38%", color: "var(--gowl-accent)" }, { height: "24%", color: "var(--gowl-amber)" }, { height: "9%", color: "var(--gowl-t7)" }], total: "48 k" },
  { week: "W30", segments: [{ height: "44%", color: "var(--gowl-accent)" }, { height: "28%", color: "var(--gowl-amber)" }, { height: "10%", color: "var(--gowl-t7)" }], total: "56 k" },
  { week: "W31", segments: [{ height: "41%", color: "var(--gowl-accent)" }, { height: "31%", color: "var(--gowl-amber)" }, { height: "12%", color: "var(--gowl-t7)" }], total: "58 k" },
  { week: "W32", segments: [{ height: "46%", color: "var(--gowl-accent)" }, { height: "34%", color: "var(--gowl-amber)" }, { height: "13%", color: "var(--gowl-t7)" }], total: "64 k" },
  { week: "W33", segments: [{ height: "40%", color: "var(--gowl-accent)" }, { height: "30%", color: "var(--gowl-amber)" }, { height: "11%", color: "var(--gowl-t7)" }], total: "57 k" },
];

const GROWTH_LEGEND = [
  { label: "Asserted", color: "var(--gowl-accent)" },
  { label: "Inferred", color: "var(--gowl-amber)" },
  { label: "Derived", color: "var(--gowl-t7)" },
];

interface FunnelStep {
  readonly label: string;
  readonly value: string;
  readonly pct: string;
  readonly width: string;
  readonly color: string;
  readonly note: string;
}

const FUNNEL_DATA: readonly FunnelStep[] = [
  { label: "Source rows ingested", value: "412,884", pct: "100%", width: "100%", color: "var(--gowl-line-3)", note: "27 sources" },
  { label: "Mapped to the ontology", value: "318,204", pct: "77%", width: "77%", color: "var(--gowl-accent)", note: "94,680 unmapped, in Sources" },
  { label: "Asserted as facts", value: "1,558,899", pct: "62%", width: "62%", color: "var(--gowl-accent)", note: "one row can carry many facts" },
  { label: "Inferred by rules", value: "284,321", pct: "38%", width: "38%", color: "var(--gowl-amber)", note: "1.54× yield" },
  { label: "Certified by a human", value: "9,204", pct: "7%", width: "7%", color: "var(--gowl-ok)", note: "the honest number" },
];

interface HeatmapCell {
  readonly predicate: string;
  readonly values: readonly string[];
  readonly levels: readonly number[];
}

const HEATMAP_DATA: readonly HeatmapCell[] = [
  { predicate: "locatedIn", values: ["0.98", "0.96", "0.91", "0.86", "0.84"], levels: [0, 0, 2, 3, 3] },
  { predicate: "hasGSTIN", values: ["0.97", "0.98", "0.99", "0.99", "0.99"], levels: [0, 0, 0, 0, 0] },
  { predicate: "igstAmount", values: ["0.99", "0.99", "0.97", "0.94", "0.92"], levels: [0, 0, 1, 2, 2] },
  { predicate: "sameAs", values: ["0.88", "0.90", "0.92", "0.94", "0.94"], levels: [1, 1, 0, 0, 0] },
  { predicate: "filingPeriod", values: ["0.94", "0.92", "0.88", "0.81", "0.78"], levels: [0, 1, 2, 3, 4] },
];

const HEATMAP_MONTHS = ["Apr", "May", "Jun", "Jul", "Aug"];

const HEATMAP_COLORS = [
  "bg-gowl-ok-bg text-gowl-ok",
  "bg-gowl-accent-bg text-gowl-accent",
  "bg-[var(--gowl-amber-deep)] text-gowl-amber",
  "bg-gowl-amber-bg text-gowl-amber",
  "bg-gowl-bad-bg text-gowl-bad",
];

interface Hotspot {
  readonly label: string;
  readonly value: string;
  readonly width: string;
  readonly color: string;
}

const HOTSPOTS: readonly Hotspot[] = [
  { label: "suppliedBy", value: "184 k traversals", width: "100%", color: "var(--gowl-accent)" },
  { label: "filingPeriod", value: "121 k", width: "66%", color: "var(--gowl-accent)" },
  { label: "hasGSTIN", value: "96 k", width: "52%", color: "var(--gowl-accent)" },
  { label: "sameAs", value: "44 k", width: "24%", color: "var(--gowl-amber)" },
  { label: "locatedIn", value: "12 k", width: "7%", color: "var(--gowl-t7)" },
];

interface ModelSpend {
  readonly name: string;
  readonly tokens: string;
  readonly width: string;
  readonly color: string;
  readonly use: string;
}

const MODEL_SPEND: readonly ModelSpend[] = [
  { name: "Reasoning model · explanations", tokens: "4.1 M", width: "64%", color: "var(--gowl-accent)", use: "case explanations, path narration" },
  { name: "Small model · classification", tokens: "1.8 M", width: "28%", color: "var(--gowl-ok)", use: "reason-code tagging, routing" },
  { name: "Embedding model · retrieval", tokens: "0.5 M", width: "8%", color: "var(--gowl-t5)", use: "entity search, blocking candidates" },
];

export default function AnalyticsRoute() {
  return (
    <div className="overflow-y-auto p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.analyticsTitle}</h1>
          <p className="text-[14px] text-gowl-t5">{strings.analyticsDescription}</p>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            className="rounded-md bg-gowl-accent px-4 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on"
          >
            {strings.analyticsShowFacts}
          </button>
          <button
            type="button"
            className="rounded-md border border-gowl-line-2 px-4 py-1.5 text-[13.5px] text-gowl-t2"
          >
            {strings.analyticsBuildReport}
          </button>
        </div>
      </div>

      {/* KPI Sparkline Cards */}
      <div className="mb-6 grid grid-cols-4 gap-3">
        {KPI_CARDS.map((kpi) => (
          <div key={kpi.label} className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">{kpi.label}</div>
            <div className="flex items-baseline gap-2">
              <span className="font-mono text-[22.5px] text-gowl-t1">{kpi.value}</span>
              <span className="font-mono text-[13px] text-gowl-ok">{kpi.delta}</span>
            </div>
            <div className="my-2 flex items-end gap-px" style={{ height: 26 }}>
              {kpi.spark.map((bar, i) => (
                <div
                  key={i}
                  className="flex-1 rounded-sm"
                  style={{ height: bar.height, backgroundColor: bar.color }}
                />
              ))}
            </div>
            <div className="text-[12.5px] text-gowl-t7">{kpi.sub}</div>
          </div>
        ))}
      </div>

      {/* 2-up charts row */}
      <div className="mb-6 grid grid-cols-2 gap-6">
        {/* Graph Growth by Fact State */}
        <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
            {strings.analyticsGrowthTitle}
          </div>
          <div className="mb-3 flex items-end gap-2" style={{ height: 140 }}>
            {GROWTH_DATA.map((col) => (
              <div key={col.week} className="flex flex-1 flex-col justify-end gap-px">
                {col.segments.map((seg, i) => (
                  <div
                    key={i}
                    className="w-full rounded-sm"
                    style={{ height: seg.height, backgroundColor: seg.color }}
                  />
                ))}
              </div>
            ))}
          </div>
          <div className="mb-3 flex justify-between font-mono text-[11.5px] text-gowl-t6">
            {GROWTH_DATA.map((col) => (
              <span key={col.week}>{col.week}</span>
            ))}
          </div>
          <div className="mb-3 flex gap-4">
            {GROWTH_LEGEND.map((item) => (
              <div key={item.label} className="flex items-center gap-1.5 text-[12.5px] text-gowl-t4">
                <div className="h-2 w-2 rounded-sm" style={{ backgroundColor: item.color }} />
                {item.label}
              </div>
            ))}
          </div>
          <p className="text-[13px] leading-relaxed text-gowl-t5">
            {strings.analyticsGrowthNote}
          </p>
        </div>

        {/* Funnel: From Source Row to Certified Fact */}
        <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
            {strings.analyticsFunnelTitle}
          </div>
          <div className="space-y-3">
            {FUNNEL_DATA.map((step) => (
              <div key={step.label}>
                <div className="mb-1 flex items-baseline justify-between">
                  <span className="text-[13.5px] text-gowl-t2">{step.label}</span>
                  <span className="font-mono text-[12.5px] text-gowl-t5">{step.value}</span>
                </div>
                <div className="relative h-2 rounded-full bg-gowl-track">
                  <div
                    className="absolute inset-y-0 left-0 rounded-full"
                    style={{ width: step.width, backgroundColor: step.color }}
                  />
                </div>
                <div className="mt-0.5 flex justify-between text-[11.5px] text-gowl-t6">
                  <span>{step.pct}</span>
                  <span>{step.note}</span>
                </div>
              </div>
            ))}
          </div>
          <p className="mt-4 text-[13px] leading-relaxed text-gowl-t5">
            {strings.analyticsFunnelNote}
          </p>
        </div>
      </div>

      {/* 2-up bottom row */}
      <div className="grid grid-cols-[1fr_380px] gap-6">
        {/* Left: Heatmap + Hotspots */}
        <div className="space-y-6">
          {/* Confidence Decay Heatmap */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.analyticsHeatmapTitle}
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-[12.5px]">
                <thead>
                  <tr>
                    <th className="pb-2 pr-3 text-left font-mono text-[11px] tracking-wider text-gowl-t6">
                      PREDICATE
                    </th>
                    {HEATMAP_MONTHS.map((month) => (
                      <th key={month} className="pb-2 px-2 text-center font-mono text-[11px] tracking-wider text-gowl-t6">
                        {month}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {HEATMAP_DATA.map((row) => (
                    <tr key={row.predicate}>
                      <td className="pr-3 py-1 font-mono text-gowl-t2">{row.predicate}</td>
                      {row.values.map((val, i) => (
                        <td key={i} className="px-2 py-1">
                          <div
                            className={`flex h-8 items-center justify-center rounded-md font-mono text-[11.5px] ${HEATMAP_COLORS[row.levels[i] ?? 0]}`}
                          >
                            {val}
                          </div>
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* Most-Traversed Relationships */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.analyticsHotspotsTitle}
            </div>
            <div className="space-y-2.5">
              {HOTSPOTS.map((item) => (
                <div key={item.label}>
                  <div className="mb-1 flex items-baseline justify-between">
                    <span className="font-mono text-[13.5px] text-gowl-t2">{item.label}</span>
                    <span className="font-mono text-[12.5px] text-gowl-t5">{item.value}</span>
                  </div>
                  <div className="h-2 rounded-full bg-gowl-track">
                    <div
                      className="h-2 rounded-full"
                      style={{ width: item.width, backgroundColor: item.color }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Right: Narrative + Model Spend */}
        <div className="space-y-4">
          {/* Read of the Period */}
          <div className="rounded-lg border border-gowl-accent-border bg-gowl-accent-deep p-5">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-accent">
              {strings.analyticsNarrativeTitle}
            </div>
            <p className="mb-3 text-[14px] leading-relaxed text-gowl-t2">
              {strings.analyticsNarrativeBody}
            </p>
            <div className="text-[12.5px] text-gowl-t5">{strings.analyticsNarrativeCite}</div>
          </div>

          {/* Model Spend vs Graph Work */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.analyticsModelSpendTitle}
            </div>
            <div className="space-y-3">
              {MODEL_SPEND.map((model) => (
                <div key={model.name}>
                  <div className="mb-1 flex items-baseline justify-between">
                    <span className="text-[13.5px] text-gowl-t2">{model.name}</span>
                    <span className="font-mono text-[12.5px] text-gowl-t5">{model.tokens}</span>
                  </div>
                  <div className="h-2 rounded-full bg-gowl-track">
                    <div
                      className="h-2 rounded-full"
                      style={{ width: model.width, backgroundColor: model.color }}
                    />
                  </div>
                  <div className="mt-0.5 text-[11.5px] text-gowl-t6">{model.use}</div>
                </div>
              ))}
            </div>
            <p className="mt-4 text-[13px] leading-relaxed text-gowl-t5">
              {strings.analyticsModelSpendNote}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
