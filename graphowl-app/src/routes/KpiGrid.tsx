/** The four-tile KPI header shared by every GOVERN screen (Plan 122a A5)
 *  and by `TraceDetail` — pulled out once it had four call sites (Validation,
 *  Resolution, Drift, Governance), matching the pattern already used for the
 *  TRACE group. */

export interface Kpi {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
}

export function KpiGrid({ kpis }: { readonly kpis: readonly Kpi[] }) {
  return (
    <div className="mb-4 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-gowl-line bg-gowl-line">
      {kpis.map((kpi) => (
        <div key={kpi.label} className="bg-gowl-panel p-4">
          <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">{kpi.label}</div>
          <div className="font-mono text-[21.5px] text-gowl-t1">{kpi.value}</div>
          {kpi.sub && <div className="mt-1 text-[12.5px] text-gowl-t7">{kpi.sub}</div>}
        </div>
      ))}
    </div>
  );
}
