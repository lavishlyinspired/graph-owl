import { strings } from "../lib/strings";

interface QualityMetric {
  readonly name: string;
  readonly score: string;
  readonly trend: string;
  readonly description: string;
  readonly width: string;
  readonly color: string;
}

const METRICS: readonly QualityMetric[] = [
  { name: "Entity Resolution Precision", score: "94.2%", trend: "+1.3% vs last month", description: "Share of merges that are correct when reviewed by a steward", width: "94%", color: "var(--gowl-ok)" },
  { name: "Citation Coverage", score: "98.7%", trend: "+0.2% vs last month", description: "Share of graph-grounded answers that cite at least one fact id", width: "99%", color: "var(--gowl-ok)" },
  { name: "Ontology Completeness", score: "87.1%", trend: "+3.4% vs last month", description: "Share of source fields mapped to a glossary term and domain class", width: "87%", color: "var(--gowl-accent)" },
  { name: "Schema Drift Resolution Time", score: "2.1 days", trend: "−0.4 days vs last month", description: "Median time from drift alert to confirmed or dismissed", width: "72%", color: "var(--gowl-accent)" },
  { name: "Contradiction Backlog", score: "18 open", trend: "+4 vs last week", description: "Open contradiction pairs awaiting steward review", width: "36%", color: "var(--gowl-amber)" },
];

export default function QualityRoute() {
  return (
    <div className="p-8">
      <div className="mb-5">
        <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.qualityTitle}</h1>
        <p className="text-[14px] text-gowl-t5">{strings.qualityDescription}</p>
      </div>

      <div className="space-y-4">
        {METRICS.map((metric) => (
          <div key={metric.name} className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-2 flex items-baseline justify-between">
              <span className="text-[14.5px] font-semibold text-gowl-t1">{metric.name}</span>
              <div className="flex items-baseline gap-3">
                <span className="font-mono text-[13.5px] text-gowl-t5">{metric.trend}</span>
                <span className="font-mono text-[19.5px] text-gowl-t1">{metric.score}</span>
              </div>
            </div>
            <div className="mb-2 h-2 rounded-full bg-gowl-track">
              <div className="h-2 rounded-full" style={{ width: metric.width, backgroundColor: metric.color }} />
            </div>
            <p className="text-[13px] text-gowl-t5">{metric.description}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
