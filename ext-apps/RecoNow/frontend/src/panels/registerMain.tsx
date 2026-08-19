import { useEffect, useState } from "react";
import { Link, useOutletContext, useSearchParams } from "react-router-dom";
import { fetchRegister, type Register } from "../lib/api";
import { ExplainCase } from "../components/ExplainCase";
import { WhyPopover } from "../components/WhyPopover";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number): string {
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

export default function RegisterRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [params] = useSearchParams();
  const reasonCode = params.get("reason_code") ?? undefined;
  const [register, setRegister] = useState<Register | null>(null);
  // Case detail is merged in here rather than living on its own route: you
  // open a case *from* this list, and a route arrived at without a selection
  // shows an empty screen.
  const [openCase, setOpenCase] = useState<string | null>(null);

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchRegister(clientId, periodId, reasonCode)
      .then(setRegister)
      .catch(() => setRegister(null));
  }, [clientId, periodId, reasonCode]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="p-6">
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold text-reco-t1">Reconciliation register</h1>
          {reasonCode && (
            <p className="text-[12.5px] text-reco-t4">
              Filtered to <span className="font-mono text-reco-t2">{reasonCode}</span> —{" "}
              <Link to="/register" className="text-reco-accent">
                clear filter
              </Link>
            </p>
          )}
        </div>
        {register && (
          <div className="text-[12.5px] text-reco-t4">
            {register.rows.length} row(s) · {formatRupees(register.total_exposure)} exposure in this filter
          </div>
        )}
      </div>

      <div className="overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
        <div className="grid grid-cols-[130px_1fr_120px_120px_110px_140px] gap-3 border-b border-reco-line bg-reco-panel-2 px-4 py-2 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5">
          <span>INVOICE</span>
          <span>SUPPLIER</span>
          <span className="text-right">BOOKS</span>
          <span className="text-right">2B</span>
          <span className="text-right">DELTA</span>
          <span>REASON</span>
        </div>
        {register?.rows.length === 0 && (
          <div className="px-4 py-4 text-[12.5px] text-reco-t4">No cases in this filter yet.</div>
        )}
        {register?.rows.map((row) => (
          <div key={row.id} className="border-b border-reco-line-2">
          <button
            type="button"
            aria-expanded={openCase === row.id}
            onClick={() => setOpenCase(openCase === row.id ? null : row.id)}
            className="grid w-full grid-cols-[130px_1fr_120px_120px_110px_140px] items-center gap-3 px-4 py-2.5 text-left text-[12.5px] hover:bg-reco-panel-2"
          >
            <span className="font-mono text-reco-t1">{row.invoice_no}</span>
            <div>
              <div className="text-reco-t1">{row.supplier_name ?? "—"}</div>
              <div className="font-mono text-[10px] text-reco-t5">{row.supplier_gstin ?? ""}</div>
            </div>
            <span className="text-right font-mono text-reco-t2">
              {row.books_amount != null ? formatRupees(row.books_amount) : "—"}
            </span>
            <span className="text-right font-mono text-reco-t2">
              {row.portal_amount != null ? formatRupees(row.portal_amount) : "—"}
            </span>
            <span className="text-right font-mono text-reco-bad">{formatRupees(row.exposure)}</span>
            <span className="text-[11px] text-reco-t4">
              {row.title ?? row.reason_code ?? "—"}
              <WhyPopover
                title={row.title ?? row.reason_code ?? ""}
                explanation={{ meaning: row.meaning, next_action: row.next_action }}
                align="right"
              />
            </span>
          </button>

          {openCase === row.id && (
            <div className="space-y-3 border-t border-reco-line-2 bg-reco-panel-2/40 px-4 py-4">
              {/* Instant: computed from this invoice's own figures. */}
              {row.narrative && (
                <p className="text-[12.5px] leading-relaxed text-reco-t2">{row.narrative}</p>
              )}
              {/* Deeper, on demand: the model reads the whole row. */}
              {clientId && periodId && (
                <ExplainCase clientId={clientId} periodId={periodId} caseId={row.id} />
              )}
            </div>
          )}
          </div>
        ))}
      </div>
    </div>
  );
}
