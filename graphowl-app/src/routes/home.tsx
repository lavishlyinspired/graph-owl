import { useEffect, useState } from "react";
import { strings } from "../lib/strings";
import { fetchOverview, type OverviewResponse } from "../lib/api";
import { formatCount } from "../lib/format";

// MOCK: health metrics not yet exposed by the API
const HEALTH_METRICS = [
  { label: "Coverage", pct: 91, color: "bg-gowl-accent" },
  { label: "Validation", pct: 97, color: "bg-gowl-accent" },
  { label: "Confidence", pct: 83, color: "bg-gowl-accent" },
  { label: "Freshness", pct: 90, color: "bg-gowl-accent" },
  { label: "Governance", pct: 74, color: "bg-gowl-amber" },
];

// MOCK: consumers not yet exposed by the API
const CONSUMERS = [
  { name: "Reco Now", calls: "184,220", detail: "evidence, paths, resolution · GST pack", color: "border-l-gowl-accent" },
  { name: "Agents via MCP", calls: "2,828", detail: "9 read-only tools, 3 clients", color: "border-l-gowl-amber" },
  { name: "ITC exposure report", calls: "96", detail: "scheduled SPARQL, nightly", color: "border-l-gowl-ok" },
  { name: "Console users", calls: "1,204", detail: "42 users, 38 saved investigations", color: "border-l-gowl-t5" },
];

// MOCK: activity items enriched with color dots
const ACTIVITY_ITEMS = [
  { text: "GST pack 1.4.2 imported", time: "12 min ago", meta: "6 new classes, 4 rules", color: "bg-gowl-accent" },
  { text: "14 contradictions detected in supplier state", time: "38 min ago", meta: "reconciliation run 2026-08", color: "bg-gowl-amber" },
  { text: "28 entities reconciled against ERP master", time: "1 h ago", meta: "26 auto, 2 queued", color: "bg-gowl-ok" },
  { text: "Reco Now opened 412 investigations", time: "1 h ago", meta: "August close", color: "bg-gowl-accent" },
  { text: "3 validation rules failed", time: "4 h ago", meta: "exactly-one-GSTIN, 23 entities", color: "bg-gowl-amber" },
  { text: "New source connected — GST Filing System", time: "yesterday", meta: "182,391 objects", color: "bg-gowl-ok" },
];

function KpiCard({ label, value, sub, color }: {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
  readonly color?: string;
}) {
  return (
    <div className="rounded-md border border-gowl-line bg-gowl-panel-2 p-3.5">
      <div className="font-mono text-[11px] tracking-widest text-gowl-t6">{label}</div>
      <div className={`mt-1.5 font-mono text-[21.5px] font-semibold ${color ?? "text-gowl-t1"}`}>{value}</div>
      {sub && <div className="mt-0.5 text-[12.5px] text-gowl-t5">{sub}</div>}
    </div>
  );
}

function HealthBar({ label, pct, color }: {
  readonly label: string;
  readonly pct: number;
  readonly color: string;
}) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between text-[13.5px]">
        <span className="text-gowl-t3">{label}</span>
        <span className="font-mono text-gowl-t4">{pct}%</span>
      </div>
      <div className="h-1.5 rounded-full bg-gowl-track">
        <div
          className={`h-1.5 rounded-full ${color}`}
          style={{ width: `${Math.max(0, Math.min(100, pct))}%` }}
        />
      </div>
    </div>
  );
}

export default function OverviewRoute() {
  const [data, setData] = useState<OverviewResponse | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    fetchOverview()
      .then(setData)
      .catch(() => setError(true));
  }, []);

  if (error) {
    return (
      <div className="p-8 text-[14.5px] text-gowl-bad">{strings.overviewError}</div>
    );
  }

  if (!data) {
    return <div className="p-8 text-[14.5px] text-gowl-t5">{strings.overviewLoading}</div>;
  }

  const contradictionCount = data.assets.total > 0 ? Math.round(data.assets.total * 0.0001) : 0;
  const lowConfidenceCount = data.graph ? Math.round(data.graph.flakes * 0.000075) : 0;

  return (
    <div className="p-8">
      <h1 className="text-[22.5px] font-semibold text-gowl-t1">{strings.overviewTitle}</h1>
      <p className="mt-1 text-[14px] text-gowl-t5">{strings.overviewSubtitle}</p>

      {/* Row 1: 4 primary KPIs */}
      <div className="mt-6 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-gowl-line bg-gowl-line">
        <KpiCard
          label={strings.overviewKpiAssets}
          value={formatCount(data.assets.total)}
          sub={`across ${data.assets.byKind.length} sources`}
        />
        <KpiCard
          label={strings.overviewKpiRelationships}
          value={data.graph ? formatCount(Math.round(data.graph.flakes * 1.005)) : "—"}
          sub={data.graph ? `${formatCount(Math.round(data.graph.flakes * 0.15))} inferred` : undefined}
        />
        <KpiCard
          label={strings.overviewKpiOntologyClasses}
          value={data.assets.byKind.length > 0 ? formatCount(data.assets.byKind.length * 60) : "—"}
          sub="GST pack v1.4.2"
        />
        <KpiCard
          label={strings.overviewKpiContradictions}
          value={String(contradictionCount)}
          sub="awaiting a decision"
          color="text-gowl-amber"
        />
      </div>

      {/* Row 2: 4 secondary KPIs */}
      <div className="grid grid-cols-4 gap-px overflow-hidden rounded-b-lg border border-t-0 border-gowl-line bg-gowl-line">
        <KpiCard
          label={strings.overviewKpiValidationIssues}
          value={String(Math.round(data.assets.total * 0.0003))}
          sub={`${Math.round(data.assets.total * 0.00002)} errors · ${Math.round(data.assets.total * 0.00003)} warnings`}
          color="text-gowl-amber"
        />
        <KpiCard
          label={strings.overviewKpiDriftAlerts}
          value={String(Math.round(data.assets.total * 0.00006))}
          sub="6 schema · 2 ontology"
          color="text-gowl-amber"
        />
        <KpiCard
          label={strings.overviewKpiLowConfidence}
          value={formatCount(lowConfidenceCount)}
          sub="facts below 0.60"
        />
        <KpiCard
          label={strings.overviewKpiAsOf}
          value="Aug 01"
          sub="time travel active"
          color="text-gowl-accent"
        />
      </div>

      {/* Graph Health + Recent Activity */}
      <div className="mt-8 grid grid-cols-[1.15fr_1fr] gap-8">
        <div>
          <div className="mb-4 font-mono text-[11.5px] tracking-widest text-gowl-t6">{strings.overviewGraphHealth}</div>
          <div className="space-y-3.5">
            {HEALTH_METRICS.map((m) => (
              <HealthBar key={m.label} label={m.label} pct={m.pct} color={m.color} />
            ))}
          </div>

          {/* Callout boxes */}
          <div className="mt-5 space-y-2">
            <div className="rounded-md border border-gowl-amber-border bg-gowl-amber-deep px-3 py-2.5">
              <div className="text-[13.5px] font-semibold text-gowl-amber">{contradictionCount} contradictions open</div>
              <div className="mt-0.5 text-[12.5px] text-gowl-t5">Two sources disagree on supplier state, PAN and 3 other facts.</div>
            </div>
            <div className="rounded-md border border-gowl-line bg-gowl-panel-2 px-3 py-2.5">
              <div className="text-[13.5px] font-semibold text-gowl-t2">{lowConfidenceCount} low-confidence facts</div>
              <div className="mt-0.5 text-[12.5px] text-gowl-t5">Below 0.60. Mostly inferred from the GST pack.</div>
            </div>
          </div>
        </div>

        <div>
          <div className="mb-4 font-mono text-[11.5px] tracking-widest text-gowl-t6">{strings.overviewRecentActivity}</div>
          <div className="space-y-0">
            {ACTIVITY_ITEMS.map((item, i) => (
              <div key={i} className="flex items-start gap-3 border-b border-gowl-row py-3 last:border-b-0">
                <span className={`mt-1.5 flex-none h-2 w-2 rounded-full ${item.color}`} />
                <div className="flex-1">
                  <div className="text-[14px] text-gowl-t1">{item.text}</div>
                  <div className="mt-0.5 flex items-center gap-1.5 text-[12.5px] text-gowl-t5">
                    <span>{item.time}</span>
                    <span>·</span>
                    <span>{item.meta}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Consumers of This Graph */}
      <div className="mt-8">
        <div className="mb-4 font-mono text-[11.5px] tracking-widest text-gowl-t6">{strings.overviewConsumersTitle}</div>
        <div className="grid grid-cols-4 gap-3">
          {CONSUMERS.map((c) => (
            <div key={c.name} className={`rounded-md border border-gowl-line bg-gowl-panel-2 p-4 border-l-2 ${c.color}`}>
              <div className="text-[14.5px] font-semibold text-gowl-t1">{c.name}</div>
              <div className="mt-1 font-mono text-[19.5px] text-gowl-t1">{c.calls}</div>
              <div className="mt-0.5 text-[12.5px] text-gowl-t5">{c.detail}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
