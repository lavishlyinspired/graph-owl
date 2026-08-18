import { useEffect, useState } from "react";
import { strings } from "../lib/strings";
import { fetchOverview, type OverviewResponse } from "../lib/api";
import { formatCount, formatPct, relativeTime } from "../lib/format";

interface StatTileProps {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
}

function StatTile({ label, value, sub }: StatTileProps) {
  return (
    <div className="rounded-md border border-gowl-line bg-gowl-panel-2 p-3">
      <div className="font-mono text-[10px] tracking-widest text-gowl-t6">{label}</div>
      <div className="mt-1 text-[20px] font-semibold text-gowl-t1">{value}</div>
      {sub && <div className="mt-0.5 text-[11px] text-gowl-t5">{sub}</div>}
    </div>
  );
}

function HealthBar({ label, pct }: { readonly label: string; readonly pct: number }) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between text-[12px]">
        <span className="text-gowl-t3">{label}</span>
        <span className="font-mono text-gowl-t4">{formatPct(pct)}</span>
      </div>
      <div className="h-1.5 rounded-full bg-gowl-track">
        <div
          className="h-1.5 rounded-full bg-gowl-accent"
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
      <div className="p-8 text-[13px] text-gowl-bad">{strings.overviewError}</div>
    );
  }

  if (!data) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.overviewLoading}</div>;
  }

  const now = new Date();

  return (
    <div className="p-8">
      <h1 className="text-[18px] font-semibold text-gowl-t1">{strings.overviewTitle}</h1>

      <div className="mt-6 grid grid-cols-4 gap-3">
        <StatTile
          label={strings.overviewAssetsTotal}
          value={formatCount(data.assets.total)}
          sub={data.assets.byKind.map((k) => `${k.kind} ${k.count}`).join(" · ")}
        />
        <StatTile
          label={strings.overviewDocumentation}
          value={formatCount(data.documentation.described)}
          sub={`of ${formatCount(data.documentation.total)}`}
        />
        {data.graph && (
          <>
            <StatTile label={strings.overviewGraphFlakes} value={formatCount(data.graph.flakes)} />
            <StatTile label={strings.overviewGraphNodes} value={formatCount(data.graph.nodes)} />
          </>
        )}
      </div>

      <div className="mt-8 grid grid-cols-2 gap-8">
        <div>
          <div className="mb-3 font-mono text-[10px] tracking-widest text-gowl-t6">{strings.overviewGraphHealth}</div>
          <div className="space-y-4">
            <HealthBar label={strings.overviewCoverage} pct={data.health.coveragePct} />
            <HealthBar label={strings.overviewGovernance} pct={data.health.governancePct} />
          </div>
        </div>

        <div>
          <div className="mb-3 font-mono text-[10px] tracking-widest text-gowl-t6">{strings.overviewRecentActivity}</div>
          {data.recentlyChanged.length === 0 ? (
            <div className="text-[13px] text-gowl-t5">{strings.overviewNoActivity}</div>
          ) : (
            <div className="space-y-2">
              {data.recentlyChanged.map((asset) => (
                <div key={asset.id} className="border-b border-gowl-line pb-2">
                  <div className="text-[13px] text-gowl-t1">{asset.name}</div>
                  <div className="mt-0.5 font-mono text-[11px] text-gowl-t5">
                    {`${relativeTime(asset.updatedAt, now)} · ${asset.updatedBy}`}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
