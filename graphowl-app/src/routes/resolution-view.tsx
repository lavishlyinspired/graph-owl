import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { evidenceSummary } from "../lib/govern";
import {
  bulkDecideReview,
  confirmReview,
  fetchResolutionQueue,
  rejectReview,
  type ReviewQueueEntry,
} from "../lib/api";
import { strings } from "../lib/strings";

export default function ResolutionRoute() {
  const [pending, setPending] = useState<readonly ReviewQueueEntry[] | null>(null);
  const [confirmedCount, setConfirmedCount] = useState<number | null>(null);
  const [rejectedCount, setRejectedCount] = useState<number | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<ReviewQueueEntry | null>(null);
  const [checked, setChecked] = useState<ReadonlySet<string>>(new Set());
  const [rejectReason, setRejectReason] = useState("");
  const [bulkAction, setBulkAction] = useState<"confirm" | "reject" | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    Promise.all([
      fetchResolutionQueue({ status: "pending" }),
      fetchResolutionQueue({ status: "confirmed", limit: 1 }),
      fetchResolutionQueue({ status: "rejected", limit: 1 }),
    ])
      .then(([queue, confirmedPage, rejectedPage]) => {
        setPending(queue.data);
        setConfirmedCount(confirmedPage.total);
        setRejectedCount(rejectedPage.total);
        setChecked(new Set());
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  if (error) {
    return <div className="p-8 text-[17px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!pending || confirmedCount === null || rejectedCount === null) {
    return <div className="p-8 text-[17px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const toggle = (id: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const runConfirm = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await confirmReview(selected.id);
      setSelected(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runReject = async () => {
    if (!selected || rejectReason.trim().length === 0) return;
    setBusy(true);
    try {
      await rejectReview(selected.id, rejectReason.trim());
      setSelected(null);
      setRejectReason("");
      load();
    } finally {
      setBusy(false);
    }
  };

  const runBulk = async () => {
    if (!bulkAction) return;
    setBusy(true);
    try {
      await bulkDecideReview(Array.from(checked), bulkAction, bulkAction === "reject" ? rejectReason.trim() : undefined);
      setBulkAction(null);
      setRejectReason("");
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.resolutionTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.resolutionDescription}</p>

        <KpiGrid
          kpis={[
            { label: strings.resolutionKpiPending, value: String(pending.length) },
            { label: strings.resolutionKpiConfirmed, value: String(confirmedCount) },
            { label: strings.resolutionKpiRejected, value: String(rejectedCount) },
            { label: strings.resolutionKpiSelected, value: String(checked.size) },
          ]}
        />

        {checked.size > 0 && (
          <div className="mb-4 flex items-center gap-3 rounded-lg border border-gowl-accent-border bg-gowl-accent-bg px-4 py-2.5">
            <span className="text-[16.5px] text-gowl-t2">
              {checked.size} {strings.governSelectedSuffix}
            </span>
            <button
              type="button"
              onClick={() => setBulkAction("confirm")}
              className="rounded-md bg-gowl-accent px-3 py-1 text-[16px] font-semibold text-gowl-accent-on"
            >
              {strings.resolutionBulkConfirm}
            </button>
            <button
              type="button"
              onClick={() => setBulkAction("reject")}
              className="rounded-md border border-gowl-line-2 px-3 py-1 text-[16px] text-gowl-t2"
            >
              {strings.resolutionBulkReject}
            </button>
          </div>
        )}

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-[24px_1fr_1fr_80px_1.5fr] gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[13.5px] tracking-wider text-gowl-t6">
            <span />
            <span>{strings.resolutionColTarget}</span>
            <span>{strings.resolutionColCandidate}</span>
            <span>{strings.resolutionColScore}</span>
            <span>{strings.resolutionColEvidence}</span>
          </div>
          {pending.length === 0 ? (
            <div className="p-6 text-[16.5px] text-gowl-t5">{strings.resolutionEmpty}</div>
          ) : (
            pending.map((entry) => (
              <div
                key={entry.id}
                className="grid grid-cols-[24px_1fr_1fr_80px_1.5fr] items-center gap-3 border-b border-gowl-row px-4 py-2.5 last:border-b-0 hover:bg-gowl-row"
              >
                <input
                  type="checkbox"
                  checked={checked.has(entry.id)}
                  onChange={() => toggle(entry.id)}
                  aria-label={`select ${entry.id}`}
                />
                <button
                  type="button"
                  onClick={() => setSelected(entry)}
                  className="truncate text-left font-mono text-[15.5px] text-gowl-t2 hover:text-gowl-accent"
                >
                  {entry.target}
                </button>
                <span className="truncate font-mono text-[15.5px] text-gowl-t2">{entry.candidate}</span>
                <span className="font-mono text-[16px] text-gowl-t1">{entry.score.toFixed(2)}</span>
                <span className="truncate text-[16px] text-gowl-t5">{evidenceSummary(entry.evidence)}</span>
              </div>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[380px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div className="text-[17px] font-semibold text-gowl-t1">
              {selected.target} {strings.governBothArrow} {selected.candidate}
            </div>
            <button type="button" onClick={() => setSelected(null)} className="text-[16px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>
          <div className="mb-1 font-mono text-[24px] text-gowl-t1">{selected.score.toFixed(2)}</div>
          <p className="mb-4 text-[16.5px] text-gowl-t3">{evidenceSummary(selected.evidence)}</p>

          <button
            type="button"
            disabled={busy}
            onClick={runConfirm}
            className="mb-2 w-full rounded-md bg-gowl-accent px-3 py-2 text-[16.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {strings.resolutionConfirm}
          </button>
          <input
            value={rejectReason}
            onChange={(e) => setRejectReason(e.target.value)}
            placeholder={strings.resolutionRejectReason}
            className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <button
            type="button"
            disabled={busy || rejectReason.trim().length === 0}
            onClick={runReject}
            className="w-full rounded-md border border-gowl-line-2 px-3 py-2 text-[16.5px] text-gowl-t2 disabled:opacity-40"
          >
            {strings.resolutionReject}
          </button>
        </div>
      )}

      {bulkAction && (
        <div className="fixed inset-0 flex items-center justify-center bg-black/50">
          <div className="w-[420px] rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-3 text-[17px] font-semibold text-gowl-t1">{strings.resolutionBulkPreviewTitle}</div>
            <div className="mb-4 max-h-[240px] overflow-y-auto rounded-md border border-gowl-line-2">
              {Array.from(checked).map((id) => {
                const entry = pending.find((p) => p.id === id);
                return (
                  <div key={id} className="border-b border-gowl-row px-3 py-2 text-[16px] text-gowl-t2 last:border-b-0">
                    {entry ? `${entry.target} ${strings.governBothArrow} ${entry.candidate}` : id}
                  </div>
                );
              })}
            </div>
            {bulkAction === "reject" && (
              <input
                value={rejectReason}
                onChange={(e) => setRejectReason(e.target.value)}
                placeholder={strings.resolutionRejectReason}
                className="mb-3 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
              />
            )}
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setBulkAction(null)}
                className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2"
              >
                {strings.governCancel}
              </button>
              <button
                type="button"
                disabled={busy || (bulkAction === "reject" && rejectReason.trim().length === 0)}
                onClick={runBulk}
                className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
              >
                {bulkAction === "confirm" ? strings.resolutionBulkConfirmGo : strings.resolutionBulkRejectGo}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
