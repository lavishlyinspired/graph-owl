import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import {
  confirmDatasetMapping,
  fetchDatasets,
  runReconcile,
  uploadDataset,
  type DatasetSummary,
  type DatasetUploadResult,
} from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

const DATASET_KINDS = [
  { kind: "books", label: "Purchase register" },
  { kind: "gstr2b", label: "GSTR-2B" },
  { kind: "gstr1", label: "GSTR-2A / GSTR-1" },
] as const;

export default function PipelineRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [datasets, setDatasets] = useState<readonly DatasetSummary[]>([]);
  const [activeKind, setActiveKind] = useState<string>("books");
  const [upload, setUpload] = useState<DatasetUploadResult | null>(null);
  const [reconcileMessage, setReconcileMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshDatasets = () => {
    if (!clientId || !periodId) return;
    fetchDatasets(clientId, periodId)
      .then(setDatasets)
      .catch(() => setDatasets([]));
  };

  useEffect(refreshDatasets, [clientId, periodId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  const onFileChosen = async (kind: string, file: File) => {
    setBusy(true);
    try {
      const result = await uploadDataset(clientId, periodId, kind, file);
      setUpload(result);
      setActiveKind(kind);
      refreshDatasets();
    } finally {
      setBusy(false);
    }
  };

  const onMappingChange = (field: string, columnIndex: number) => {
    if (!upload) return;
    setUpload({ ...upload, mapping: { ...upload.mapping, [field]: columnIndex } });
  };

  const onConfirm = async () => {
    if (!upload) return;
    setBusy(true);
    try {
      await confirmDatasetMapping(clientId, periodId, activeKind, upload.mapping, 1.0);
      refreshDatasets();
    } finally {
      setBusy(false);
    }
  };

  const allConfirmed = datasets.length > 0 && datasets.every((d) => d.confirmed);

  const onReconcile = async () => {
    setBusy(true);
    setReconcileMessage(null);
    try {
      const result = await runReconcile(clientId, periodId);
      setReconcileMessage(
        result.ok
          ? `Reconciled: ${result.evaluated ?? 0} evaluated, ${result.found ?? 0} found, ${result.cases_created ?? 0} new case(s).`
          : `Reconcile could not reach GraphOWL: ${result.error}`,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="p-6">
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold text-reco-t1">Upload &amp; map</h1>
          <p className="text-[12.5px] text-reco-t4">
            Three files in, one reconciliation out. Mapping is the only place you tell the system what a column
            means.
          </p>
        </div>
        <button
          type="button"
          onClick={onReconcile}
          disabled={!allConfirmed || busy}
          className="rounded-lg bg-reco-t0 px-3.5 py-1.5 text-[12.5px] font-semibold text-white disabled:opacity-40"
        >
          {allConfirmed ? "Reconcile" : "Confirm all files first"}
        </button>
      </div>

      {reconcileMessage && (
        <div className="mb-4 rounded-lg border border-reco-accent-border bg-reco-accent-bg px-3.5 py-2.5 text-[12.5px] text-reco-t1">
          {reconcileMessage}
        </div>
      )}

      <div className="grid grid-cols-[236px_1fr] gap-3.5">
        <div className="rounded-lg border border-reco-line bg-reco-panel py-3">
          <div className="px-4 pb-2 font-mono text-[9px] tracking-[0.14em] text-reco-t5">FILES IN THIS PERIOD</div>
          {DATASET_KINDS.map(({ kind, label }) => {
            const summary = datasets.find((d) => d.kind === kind);
            return (
              <div key={kind} className={`mx-2 mb-0.5 rounded-md px-2.5 py-2 ${activeKind === kind ? "bg-reco-row" : ""}`}>
                <button type="button" onClick={() => setActiveKind(kind)} className="flex w-full items-center gap-1.5 text-left">
                  <span className="text-[12.5px] font-medium text-reco-t1">{label}</span>
                  {summary?.confirmed && <span className="ml-auto text-[10px] text-reco-ok">✓</span>}
                </button>
                <div className="mt-0.5 font-mono text-[9px] text-reco-t5">
                  {summary ? `${summary.total_rows} rows` : "not uploaded"}
                </div>
                <label className="mt-1.5 block cursor-pointer text-[11px] text-reco-accent">
                  {summary ? "Replace file" : "Upload file"}
                  <input
                    type="file"
                    accept=".csv,.xlsx,.xls,.json"
                    className="hidden"
                    onChange={(e) => {
                      const file = e.target.files?.[0];
                      if (file) onFileChosen(kind, file);
                    }}
                  />
                </label>
              </div>
            );
          })}
        </div>

        <div>
          {upload && activeKind && upload.kind === activeKind ? (
            <div className="rounded-lg border border-reco-line bg-reco-panel">
              <div className="grid grid-cols-[1.2fr_1.2fr_96px] gap-3 border-b border-reco-line bg-reco-panel-2 px-4.5 py-2.5 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5">
                <span>FIELD</span>
                <span>COLUMN</span>
                <span className="text-right">SAMPLE</span>
              </div>
              {Object.entries(upload.mapping).map(([field, colIndex]) => (
                <div key={field} className="grid grid-cols-[1.2fr_1.2fr_96px] items-center gap-3 border-b border-reco-line-2 px-4.5 py-2.5">
                  <span className="text-[12.5px] font-medium text-reco-t1">{field}</span>
                  <select
                    value={colIndex ?? ""}
                    onChange={(e) => onMappingChange(field, e.target.value === "" ? -1 : Number(e.target.value))}
                    aria-label={`Column for ${field}`}
                    className="rounded-md border border-reco-line px-2 py-1 font-mono text-[11px] text-reco-t1"
                  >
                    <option value="">Not mapped</option>
                    {upload.headers.map((h, i) => (
                      <option key={h} value={i}>
                        {h}
                      </option>
                    ))}
                  </select>
                  <span className="text-right font-mono text-[11px] text-reco-t3">
                    {colIndex != null && colIndex >= 0
                      ? String(upload.preview[0]?.[upload.headers[colIndex] ?? ""] ?? "—")
                      : "—"}
                  </span>
                </div>
              ))}
              <div className="flex items-center justify-end px-4.5 py-3">
                <button
                  type="button"
                  onClick={onConfirm}
                  disabled={busy}
                  className="rounded-md border border-reco-accent-border bg-reco-accent-bg px-3 py-1.5 text-[12px] text-reco-accent-hi"
                >
                  Confirm mapping
                </button>
              </div>
            </div>
          ) : (
            <div className="rounded-lg border border-reco-line bg-reco-panel p-6 text-[12.5px] text-reco-t4">
              Upload a file on the left to map its columns.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
