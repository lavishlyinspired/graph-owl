import { useEffect, useState } from "react";
import { KpiGrid } from "./KpiGrid";
import { acceptProposal, fetchProposals, rejectProposal, type Proposal } from "../lib/api";
import { strings } from "../lib/strings";

export default function RunsRoute() {
  const [open, setOpen] = useState<readonly Proposal[] | null>(null);
  const [acceptedCount, setAcceptedCount] = useState<number | null>(null);
  const [rejectedCount, setRejectedCount] = useState<number | null>(null);
  const [error, setError] = useState(false);
  const [selected, setSelected] = useState<Proposal | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    Promise.all([
      fetchProposals({ status: "open" }),
      fetchProposals({ status: "accepted", limit: 1 }),
      fetchProposals({ status: "rejected", limit: 1 }),
    ])
      .then(([openPage, acceptedPage, rejectedPage]) => {
        setOpen(openPage.data);
        setAcceptedCount(acceptedPage.data.length);
        setRejectedCount(rejectedPage.data.length);
      })
      .catch(() => setError(true));
  };

  useEffect(load, []);

  if (error) {
    return <div className="p-8 text-[13px] text-gowl-bad">{strings.governError}</div>;
  }
  if (!open || acceptedCount === null || rejectedCount === null) {
    return <div className="p-8 text-[13px] text-gowl-t5">{strings.governLoading}</div>;
  }

  const runAccept = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await acceptProposal(selected.id);
      setSelected(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  const runReject = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await rejectProposal(selected.id);
      setSelected(null);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full">
      <div className="flex-1 overflow-y-auto p-8">
        <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.runsTitle}</h1>
        <p className="mb-5 text-[12.5px] text-gowl-t5">{strings.runsDescription}</p>

        <KpiGrid
          kpis={[
            { label: strings.runsKpiOpen, value: String(open.length) },
            { label: strings.runsKpiAccepted, value: String(acceptedCount) },
            { label: strings.runsKpiRejected, value: String(rejectedCount) },
          ]}
        />

        <div className="mb-4 rounded-lg border border-gowl-line-2 bg-gowl-panel p-3">
          <p className="text-[12px] text-gowl-t5">{strings.runsGapNote}</p>
        </div>

        <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="grid grid-cols-5 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-4 py-2 font-mono text-[9.5px] tracking-wider text-gowl-t6">
            <span>{strings.runsColProposedBy}</span>
            <span>{strings.runsColTarget}</span>
            <span>{strings.runsColCapability}</span>
            <span>{strings.runsColConfidence}</span>
            <span>{strings.runsColStatus}</span>
          </div>
          {open.length === 0 ? (
            <div className="p-6 text-[12.5px] text-gowl-t5">{strings.runsEmpty}</div>
          ) : (
            open.map((proposal) => (
              <button
                key={proposal.id}
                type="button"
                onClick={() => setSelected(proposal)}
                className="grid w-full grid-cols-5 items-center gap-3 border-b border-gowl-row px-4 py-2.5 text-left last:border-b-0 hover:bg-gowl-row"
              >
                <span className="truncate text-[12.5px] text-gowl-t1">{proposal.proposedBy.displayName || proposal.proposedBy.id}</span>
                <span className="truncate font-mono text-[11.5px] text-gowl-t2">{proposal.targetFqn}</span>
                <span className="font-mono text-[11px] text-gowl-t2">{proposal.capability}</span>
                <span className="font-mono text-[12px] text-gowl-t1">{proposal.confidence.toFixed(2)}</span>
                <span className="text-[12px] text-gowl-t5">{proposal.status}</span>
              </button>
            ))
          )}
        </div>
      </div>

      {selected && (
        <div className="w-[400px] flex-none overflow-y-auto border-l border-gowl-line bg-gowl-panel p-5">
          <div className="mb-4 flex items-start justify-between">
            <div>
              <div className="font-mono text-[11px] text-gowl-t6">{selected.targetFqn}</div>
              <div className="text-[15px] font-semibold text-gowl-t1">{selected.capability}</div>
            </div>
            <button type="button" onClick={() => setSelected(null)} className="text-[12px] text-gowl-t5">
              {strings.governClose}
            </button>
          </div>

          <div className="mb-3">
            <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.runsRationale}</div>
            <p className="text-[12.5px] text-gowl-t2">{selected.rationale}</p>
          </div>
          <div className="mb-3">
            <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.runsChange}</div>
            <pre className="overflow-x-auto rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-2 font-mono text-[11px] text-gowl-t2">
              {JSON.stringify(selected.change, null, 2)}
            </pre>
          </div>

          <button
            type="button"
            disabled={busy}
            onClick={runAccept}
            className="mb-2 w-full rounded-md bg-gowl-accent px-3 py-2 text-[12.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {strings.runsAccept}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={runReject}
            className="w-full rounded-md border border-gowl-line-2 px-3 py-2 text-[12.5px] text-gowl-t2 disabled:opacity-40"
          >
            {strings.runsReject}
          </button>
        </div>
      )}
    </div>
  );
}
