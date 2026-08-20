import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { rollupSources, type SourceHealth, type SourceRollup } from "../lib/sources";
import { fetchConnectorRuns, type ConnectorRun } from "../lib/api";
import { relativeTime } from "../lib/format";
import { strings } from "../lib/strings";

const HEALTH_LABEL: Record<SourceHealth, string> = {
  healthy: strings.sourcesHealthHealthy,
  stale: strings.sourcesHealthStale,
  degraded: strings.sourcesHealthDegraded,
};

const HEALTH_COLOR: Record<SourceHealth, string> = {
  healthy: "text-gowl-ok",
  stale: "text-gowl-amber",
  degraded: "text-gowl-bad",
};

export default function SourcesRoute() {
  const [runs, setRuns] = useState<readonly ConnectorRun[] | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<SourceRollup | null>(null);

  useEffect(() => {
    fetchConnectorRuns()
      .then(setRuns)
      .catch(() => setError(true));
  }, []);

  if (error) {
    return <div className="p-8 text-[14.5px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!runs) {
    return <div className="p-8 text-[14.5px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const now = new Date();
  const sources = rollupSources(runs, now);
  const totalObjects = sources.reduce((sum, s) => sum + s.objects, 0);
  const staleCount = sources.filter((s) => s.health === "stale").length;

  const historyFor = (serviceName: string) =>
    [...runs]
      .filter((r) => r.serviceName === serviceName)
      .sort((a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime());

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.sourcesTitle}</h1>
        <p className="mb-5 text-[14px] text-gowl-t5">{strings.sourcesDescription}</p>

        <KpiGrid
          kpis={[
            { label: strings.sourcesKpiCount, value: String(sources.length) },
            { label: strings.sourcesKpiObjects, value: totalObjects.toLocaleString() },
            { label: strings.sourcesKpiStale, value: String(staleCount) },
            { label: strings.sourcesKpiRuns, value: String(runs.length) },
          ]}
        />

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-5 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[11px] tracking-wider text-gowl-t6">
            <span>{strings.sourcesColSource}</span>
            <span>{strings.sourcesColConnector}</span>
            <span>{strings.sourcesColObjects}</span>
            <span>{strings.sourcesColLastSync}</span>
            <span>{strings.sourcesColHealth}</span>
          </div>
          {sources.length === 0 ? (
            <div className="p-6 text-[14px] text-gowl-t5">{strings.sourcesEmpty}</div>
          ) : (
            sources.map((source) => (
              <button
                key={source.serviceName}
                type="button"
                onClick={() => setSelected(source)}
                className="grid w-full grid-cols-5 items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <span className="truncate text-[14px] text-gowl-t1">{source.serviceName}</span>
                <span className="font-mono text-[13px] text-gowl-t2">{source.connector}</span>
                <span className="font-mono text-[13.5px] text-gowl-t1">{source.objects.toLocaleString()}</span>
                <span className="text-[13.5px] text-gowl-t5">{relativeTime(source.lastSyncAt, now)}</span>
                <span className={`text-[13.5px] font-semibold ${HEALTH_COLOR[source.health]}`}>
                  {HEALTH_LABEL[source.health]}
                </span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[420px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div>
              <div className="font-mono text-[12.5px] text-gowl-t6">{selected.connector}</div>
              <div className="text-[16.5px] font-semibold text-gowl-t1">{selected.serviceName}</div>
            </div>
            <button type="button" onClick={() => setSelected(null)} className="text-[13.5px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>

          <div className="mb-4 rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.sourcesNoteTitle}</div>
            <div className="text-[13.5px] leading-relaxed text-gowl-t3">{strings.sourcesNoteBody}</div>
          </div>

          <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.sourcesRunHistory}</div>
          <div className="overflow-hidden rounded-md border border-gowl-line-2">
            {historyFor(selected.serviceName).map((run) => (
              <div key={run.id} className="border-b border-gowl-row px-3 py-2 text-[13px] last:border-b-0">
                <div className="flex justify-between text-gowl-t2">
                  <span>{relativeTime(run.startedAt, now)}</span>
                  <span className="text-gowl-t6">{run.triggeredBy}</span>
                </div>
                <div className="mt-0.5 font-mono text-[12px] text-gowl-t5">
                  {strings.runPrefixCreated}
                  {run.created} {strings.runPrefixSkipped}
                  {run.skipped}{" "}
                  {run.failed > 0 && (
                    <span className="text-gowl-bad">
                      {strings.runPrefixFailed}
                      {run.failed}
                    </span>
                  )}{" "}
                  {strings.runPrefixDeleted}
                  {run.deleted}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
