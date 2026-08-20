import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { applyDrift, fetchDrift, ignoreDrift, type DriftItem } from "../lib/api";
import { strings } from "../lib/strings";

export default function DriftRoute() {
  const [pending, setPending] = useState<readonly DriftItem[] | null>(null);
  const [appliedCount, setAppliedCount] = useState<number | null>(null);
  const [ignoredCount, setIgnoredCount] = useState<number | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<DriftItem | null>(null);
  const [ignoreReason, setIgnoreReason] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    Promise.all([
      fetchDrift({ status: "pending" }),
      fetchDrift({ status: "applied", limit: 1 }),
      fetchDrift({ status: "ignored", limit: 1 }),
    ])
      .then(([queue, appliedPage, ignoredPage]) => {
        setPending(queue.data);
        setAppliedCount(appliedPage.total);
        setIgnoredCount(ignoredPage.total);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  useEffect(() => {
    if (!selected) return;
    setSelected(pending?.find((item) => item.id === selected.id) ?? null);
  }, [pending, selected]);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!pending || appliedCount === null || ignoredCount === null) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const canApply = (item: DriftItem) => item.field === "description";

  const runApply = async () => {
    if (!selected || !canApply(selected)) return;
    setBusy(true);
    try {
      await applyDrift(selected.id);
      setSelected(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runIgnore = async () => {
    if (!selected || ignoreReason.trim().length === 0) return;
    setBusy(true);
    try {
      await ignoreDrift(selected.id, ignoreReason.trim());
      setSelected(null);
      setIgnoreReason("");
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.driftTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.driftDescription}</p>

        <KpiGrid
          kpis={[
            { label: strings.driftKpiPending, value: String(pending.length) },
            { label: strings.driftKpiApplied, value: String(appliedCount) },
            { label: strings.driftKpiIgnored, value: String(ignoredCount) },
            { label: strings.driftKpiAssets, value: String(new Set(pending.map((item) => item.assetId)).size) },
          ]}
        />

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-5 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
            <span>{strings.driftColField}</span>
            <span>{strings.driftColLive}</span>
            <span>{strings.driftColDeclared}</span>
            <span>{strings.driftColKind}</span>
            <span />
          </div>
          {pending.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{strings.driftEmpty}</div>
          ) : (
            pending.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => setSelected(item)}
                className="grid w-full grid-cols-5 items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <span className="truncate text-[16.5px] text-gowl-t1">{item.field}</span>
                <span className="truncate font-mono text-[15.5px] text-gowl-t2">{item.liveValue ?? "—"}</span>
                <span className="truncate font-mono text-[15.5px] text-gowl-t2">{item.declaredValue ?? "—"}</span>
                <span className="text-[16px] text-gowl-t5">{item.kind}</span>
                <span className="truncate text-[15px] text-gowl-t6">{item.fullyQualifiedName}</span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[380px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div>
              <div className="font-mono text-[15px] text-gowl-t6">{selected.fullyQualifiedName}</div>
              <div className="text-[19px] font-semibold text-gowl-t1">{selected.field}</div>
            </div>
            <button type="button" onClick={() => setSelected(null)} className="text-[16px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>
          <div className="mb-4 space-y-2 text-[16.5px]">
            <div className="flex justify-between">
              <span className="text-gowl-t5">{strings.driftColLive}</span>
              <span className="font-mono text-gowl-t1">{selected.liveValue ?? "—"}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gowl-t5">{strings.driftColDeclared}</span>
              <span className="font-mono text-gowl-t1">{selected.declaredValue ?? "—"}</span>
            </div>
          </div>

          {canApply(selected) ? (
            <button
              type="button"
              disabled={busy}
              onClick={runApply}
              className="mb-2 w-full rounded-md bg-gowl-accent px-3 py-2 text-[16.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
            >
              {strings.driftApply}
            </button>
          ) : (
            <p className="mb-2 text-[15.5px] text-gowl-t6">{strings.driftApplyUnavailable}</p>
          )}
          <input
            value={ignoreReason}
            onChange={(e) => setIgnoreReason(e.target.value)}
            placeholder={strings.driftIgnoreReason}
            className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <button
            type="button"
            disabled={busy || ignoreReason.trim().length === 0}
            onClick={runIgnore}
            className="w-full rounded-md border border-gowl-line-2 px-3 py-2 text-[16.5px] text-gowl-t2 disabled:opacity-40"
          >
            {strings.driftIgnore}
          </button>
        </div>
      )}
    </div>
  );
}
