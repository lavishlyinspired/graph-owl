import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import {
  confirmDatasetMapping,
  fetchDataset,
  fetchDatasets,
  runReconcile,
  uploadDataset,
  type DatasetSummary,
  type DatasetUploadResult,
  type DataIssue,
} from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

/** `optional: true` means the reconciliation runs without it — but some
 *  statutory checks cannot, and the Reconcile screen names which. */
const DATASET_KINDS = [
  { kind: "books", label: "Purchase register", optional: false, enables: "" },
  { kind: "gstr2b", label: "GSTR-2B", optional: false, enables: "" },
  { kind: "gstr1", label: "GSTR-2A / GSTR-1", optional: true, enables: "supplier-declaration checks" },
  { kind: "payments", label: "Payment ledger", optional: true, enables: "Rule 37 — 180-day reversal" },
  { kind: "grn", label: "Goods receipt (GRN)", optional: true, enables: "s.16(2)(b) — goods received" },
] as const;

export default function PipelineRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [datasets, setDatasets] = useState<readonly DatasetSummary[]>([]);
  const [activeKind, setActiveKind] = useState<string>("books");
  const [upload, setUpload] = useState<DatasetUploadResult | null>(null);
  const [reconcileMessage, setReconcileMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [view, setView] = useState<"mapping" | "data">("mapping");

  const refreshDatasets = () => {
    if (!clientId || !periodId) return;
    fetchDatasets(clientId, periodId)
      .then(setDatasets)
      .catch(() => setDatasets([]));
  };

  useEffect(refreshDatasets, [clientId, periodId]);

  // Uploads are persisted, so the file the user is looking at is whatever
  // `activeKind` names — not merely the one uploaded in this page visit.
  // Reloading it on every switch is what makes a file reviewable again after
  // navigating away and coming back.
  useEffect(() => {
    if (!clientId || !periodId || !activeKind) return;
    let cancelled = false;
    fetchDataset(clientId, periodId, activeKind)
      .then((d) => {
        if (!cancelled) setUpload(d);
      })
      .catch(() => {
        // 404 simply means this kind has not been uploaded for this period.
        if (!cancelled) setUpload(null);
      });
    return () => {
      cancelled = true;
    };
  }, [clientId, periodId, activeKind]);

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
            Two files in is a reconciliation; the optional three add the statutory checks that need them.
            Mapping is the only place you tell the system what a column means.
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
          {DATASET_KINDS.map(({ kind, label, optional, enables }) => {
            const summary = datasets.find((d) => d.kind === kind);
            return (
              <div key={kind} className={`mx-2 mb-0.5 rounded-md px-2.5 py-2 ${activeKind === kind ? "bg-reco-row" : ""}`}>
                <button type="button" onClick={() => setActiveKind(kind)} className="flex w-full items-center gap-1.5 text-left">
                  <span className="text-[12.5px] font-medium text-reco-t1">{label}</span>
                  {summary?.confirmed && <span className="ml-auto text-[10px] text-reco-ok">✓</span>}
                </button>
                <div className="mt-0.5 font-mono text-[9px] text-reco-t5">
                  {summary ? `${summary.total_rows} rows` : optional ? "optional" : "not uploaded"}
                </div>
                {!summary && enables && (
                  <div className="mt-0.5 text-[9.5px] leading-snug text-reco-t5">
                    enables {enables}
                  </div>
                )}
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
          {upload && activeKind && upload.kind === activeKind && (
            <DataIssues issues={upload.issues ?? []} rows={upload.total_rows} />
          )}

          {upload && activeKind && upload.kind === activeKind && (
            <div className="mb-3 flex items-center gap-2">
              <button
                type="button"
                onClick={() => setView("mapping")}
                className={`rounded-md border px-2.5 py-1 text-[11.5px] ${
                  view === "mapping"
                    ? "border-reco-accent-border bg-reco-accent-bg text-reco-accent-hi"
                    : "border-reco-line bg-white text-reco-t2"
                }`}
              >
                Mapping
              </button>
              <button
                type="button"
                onClick={() => setView("data")}
                className={`rounded-md border px-2.5 py-1 text-[11.5px] ${
                  view === "data"
                    ? "border-reco-accent-border bg-reco-accent-bg text-reco-accent-hi"
                    : "border-reco-line bg-white text-reco-t2"
                }`}
              >
                Data
              </button>
              <span className="ml-1 font-mono text-[10.5px] text-reco-t5">
                {upload.name ?? activeKind} · {upload.total_rows} rows
              </span>
            </div>
          )}

          {upload && activeKind && upload.kind === activeKind && view === "data" ? (
            <DataTable upload={upload} />
          ) : upload && activeKind && upload.kind === activeKind ? (
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

/** The uploaded file as it actually is — one column per header, one row per
 *  record. The mapping view answers "what does this column mean"; this
 *  answers "what is in the file", which is the question you have when a
 *  reconciliation result looks wrong.
 *
 *  Horizontally scrollable in its own container: a GST purchase register has
 *  17 columns and the page itself must never scroll sideways. */
function DataTable({ upload }: { readonly upload: DatasetUploadResult }) {
  const rows = upload.rows ?? upload.preview;
  const limit = upload.row_limit ?? rows.length;
  const truncated = upload.total_rows > rows.length;

  if (rows.length === 0) {
    return (
      <div className="rounded-lg border border-reco-line bg-reco-panel px-4.5 py-9 text-center text-[12.5px] text-reco-t4">
        This file has no rows.
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-reco-line bg-reco-panel">
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left">
          <thead>
            <tr className="border-b border-reco-line bg-reco-panel-2">
              <th className="px-3 py-2.5 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5">#</th>
              {upload.headers.map((h) => (
                <th
                  key={h}
                  className="whitespace-nowrap px-3 py-2.5 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5"
                >
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i} className="border-b border-reco-line-2 last:border-b-0 hover:bg-reco-panel-2">
                <td className="px-3 py-2 font-mono text-[10.5px] text-reco-t5">{i + 1}</td>
                {upload.headers.map((h) => {
                  const value = row[h];
                  // An empty cell is genuinely empty in the source file, and
                  // showing "—" says so rather than leaving a blank a reader
                  // could mistake for a rendering fault.
                  const text =
                    value === null || value === undefined || value === "" ? "—" : String(value);
                  return (
                    <td
                      key={h}
                      className={`whitespace-nowrap px-3 py-2 text-[11.5px] ${
                        text === "—" ? "text-reco-t5" : "text-reco-t1"
                      }`}
                    >
                      {text}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {truncated && (
        <div className="border-t border-reco-line px-4.5 py-2.5 font-mono text-[10.5px] text-reco-t5">
          Showing the first {limit} of {upload.total_rows} rows. Reconciliation reads every row.
        </div>
      )}
    </div>
  );
}


/** What is wrong with this file, at the moment it is uploaded.
 *
 *  The ingestion already skipped a payment row with no date — correctly, since
 *  an event with no time cannot answer "how many days apart" and treating it
 *  as never-paid would manufacture a reversal the client does not owe. But it
 *  skipped it *silently*: nothing told the person who uploaded the file that
 *  seven of their eight payments would not be counted.
 *
 *  Blocking first, because those are the rows that will not reach the graph at
 *  all. Nothing here refuses the file — a file with problems is still the best
 *  information available. */
function DataIssues({
  issues,
  rows,
}: {
  readonly issues: readonly DataIssue[];
  readonly rows: number;
}) {
  if (issues.length === 0) {
    return (
      <div className="mb-3 rounded-md border border-reco-line bg-white px-3 py-2 text-[11.5px] text-reco-t4">
        <span className="text-reco-ok">✓</span> {rows} rows read with no problems found.
      </div>
    );
  }

  const blocking = issues.filter((i) => i.severity === "blocking");

  return (
    <div
      className="mb-3 overflow-hidden rounded-[10px]"
      style={{
        border: `${blocking.length ? 2 : 1}px solid ${blocking.length ? "#eed7d1" : "#f0dcc2"}`,
        background: blocking.length ? "#fdf1ee" : "#fdf3e7",
      }}
    >
      <div
        className="px-3.5 py-2"
        style={{ borderBottom: `1px solid ${blocking.length ? "#eed7d1" : "#f0dcc2"}` }}
      >
        <span
          className="font-mono text-[10px] font-semibold tracking-[0.1em]"
          style={{ color: blocking.length ? "#a13f28" : "#a86a2c" }}
        >
          {blocking.length > 0 ? "⚠ ROWS THAT WILL NOT BE COUNTED" : "⚠ WORTH A LOOK"}
        </span>
        <span className="ml-2 text-[11.5px] text-reco-t2">
          {issues.length} issue{issues.length === 1 ? "" : "s"} in {rows} rows
        </span>
      </div>
      <div className="px-3.5 py-2">
        {issues.map((issue) => (
          <div key={issue.code} className="border-b border-reco-row py-1.5 last:border-b-0">
            <div className="flex items-baseline gap-2">
              <span
                className="font-mono text-[10px]"
                style={{ color: issue.severity === "blocking" ? "#a13f28" : "#a86a2c" }}
              >
                {issue.severity === "blocking" ? "✕" : "⚠"}
              </span>
              <span className="font-mono text-[11px] text-reco-t1">{issue.code}</span>
              <span className="font-mono text-[10.5px] text-reco-t5">
                {issue.rows} row{issue.rows === 1 ? "" : "s"} · e.g. row {issue.example_row}
              </span>
            </div>
            <div className="ml-5 text-[11.5px] leading-snug text-reco-t2">{issue.detail}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
