import { useEffect, useState } from "react";
import { useNavigate, useOutletContext, useSearchParams } from "react-router-dom";
import { fetchCaseDetail, recordImsDecision, type CaseDetail } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number | null): string {
  return amount != null ? `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}` : "—";
}

export default function CaseRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const caseId = params.get("id");
  const [detail, setDetail] = useState<CaseDetail | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = () => {
    if (!clientId || !periodId || !caseId) return;
    fetchCaseDetail(clientId, periodId, caseId)
      .then(setDetail)
      .catch(() => setDetail(null));
  };

  useEffect(refresh, [clientId, periodId, caseId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }
  if (!caseId) {
    return <div className="p-8 text-[13px] text-reco-t4">Open a case from the register to see it here.</div>;
  }
  if (!detail) {
    return <div className="p-8 text-[13px] text-reco-t4">Loading…</div>;
  }

  const decide = async (decision: "accept" | "reject" | "pending") => {
    setBusy(true);
    try {
      await recordImsDecision(clientId, periodId, caseId, decision);
      refresh();
    } finally {
      setBusy(false);
    }
  };

  const latestDecision = detail.ims_decisions.at(-1)?.decision;

  return (
    <div className="p-6">
      <div className="mb-3 flex items-center gap-3 text-[11.5px] text-reco-t4">
        <span>{detail.group_reason_code ?? "case"}</span>
        {detail.prev_id && (
          <button
            type="button"
            onClick={() => navigate(`/case?id=${detail.prev_id}`)}
            className="rounded-md border border-reco-line bg-reco-panel px-2 py-1 text-reco-t2"
          >
            ‹ Previous
          </button>
        )}
        {detail.next_id && (
          <button
            type="button"
            onClick={() => navigate(`/case?id=${detail.next_id}`)}
            className="rounded-md border border-reco-line bg-reco-panel px-2 py-1 text-reco-t2"
          >
            Next ›
          </button>
        )}
      </div>

      <div className="mb-3.5 flex items-start gap-6 rounded-lg border border-reco-line bg-reco-panel px-5 py-4.5">
        <div className="flex-1">
          <div className="mb-1.5 flex items-center gap-2.5">
            <h1 className="text-[20px] font-bold text-reco-t1">{detail.invoice_no}</h1>
            {detail.reason_code && (
              <span className="rounded border border-reco-amber-border bg-reco-amber-bg px-2 py-0.5 font-mono text-[9.5px] text-reco-amber">
                {detail.reason_code.toUpperCase()}
              </span>
            )}
          </div>
          <div className="text-[12.5px] text-reco-t4">
            {detail.supplier_name ?? "—"} · <span className="font-mono">{detail.supplier_gstin ?? "—"}</span>
          </div>
        </div>
        <div className="flex gap-6">
          <div>
            <div className="font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">BOOKS</div>
            <div className="mt-1 font-mono text-[19px] text-reco-t1">{formatRupees(detail.books_amount)}</div>
          </div>
          <div>
            <div className="font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">2B</div>
            <div className="mt-1 font-mono text-[19px] text-reco-t1">{formatRupees(detail.portal_amount)}</div>
          </div>
          <div>
            <div className="font-mono text-[9.5px] tracking-[0.1em] text-reco-amber">DIFFERENCE</div>
            <div className="mt-1 font-mono text-[19px] text-reco-bad">{formatRupees(detail.exposure)}</div>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-[1fr_320px] gap-3.5">
        <div className="flex flex-col gap-3.5">
          <div className="rounded-lg border border-reco-line bg-reco-panel px-4.5 py-4">
            <div className="mb-3 flex items-center justify-between">
              <span className="text-[13px] font-semibold text-reco-t1">Why this case exists</span>
              <span className="font-mono text-[10px] text-reco-t4">rule + evidence from GraphOWL</span>
            </div>
            <p className="text-[12.5px] leading-relaxed text-reco-t2">
              {detail.summary ?? "No summary recorded for this case."}
            </p>
            {/* The facts themselves, not just how many there are. A case
                that cites "4 facts" without showing them asks to be trusted;
                these are what a reviewer defends the number with. */}
            {detail.evidence.length > 0 && (
              <div className="mt-3 border-t border-reco-line-2 pt-3">
                <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                  FACTS CITED
                </div>
                <div className="overflow-hidden rounded-md border border-reco-line-2">
                  {detail.evidence.map((f, i) => (
                    <div
                      key={`${f.predicate}-${f.var}-${i}`}
                      className="grid grid-cols-[150px_90px_1fr] gap-2 border-b border-reco-row px-2.5 py-1.5 last:border-b-0"
                    >
                      <span className="font-mono text-[10.5px] text-reco-t4">{f.predicate ?? "—"}</span>
                      <span className="font-mono text-[10.5px] text-reco-accent">{f.var ?? ""}</span>
                      <span className="font-mono text-[11px] text-reco-t1">{f.value ?? "—"}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
            {detail.evidence.length === 0 && detail.evidence_count != null && !detail.graph_reachable && (
              <div className="mt-3 border-t border-reco-line-2 pt-3 text-[11.5px] text-reco-bad">
                {detail.evidence_count} fact(s) were cited when this case was
                raised, but GraphOWL is not reachable, so they cannot be shown.
              </div>
            )}
            <div className="mt-3 flex items-center gap-3 border-t border-reco-line-2 pt-3">
              <span className="font-mono text-[10.5px] text-reco-t4">
                {detail.governed_by ?? "no rule reference recorded"}
                {detail.evidence_count != null && ` · ${detail.evidence_count} fact(s) cited`}
              </span>
              {detail.subject && (
                <a
                  href={`${detail.graphowl_url}/entity/${encodeURIComponent(detail.subject)}`}
                  target="_blank"
                  rel="noreferrer"
                  className="ml-auto text-[12px] font-medium text-reco-accent"
                >
                  Open investigation in GraphOWL →
                </a>
              )}
            </div>
          </div>
        </div>

        <div className="flex flex-col gap-3.5">
          <div className="rounded-lg border border-reco-line bg-reco-panel px-4.5 py-4">
            <div className="mb-2.5 flex items-center justify-between">
              <span className="text-[13px] font-semibold text-reco-t1">IMS decision</span>
              {latestDecision && (
                <span className="rounded border border-reco-purple-border bg-reco-purple-bg px-2 py-0.5 font-mono text-[9.5px] text-reco-purple">
                  {latestDecision.toUpperCase()}
                </span>
              )}
            </div>
            <p className="mb-3 text-[12px] leading-relaxed text-reco-t4">
              Accepting records the supplier's value. It changes GSTR-2B on recompute, not just this case.
            </p>
            <div className="flex gap-1.5">
              <button
                type="button"
                disabled={busy}
                onClick={() => decide("accept")}
                className="flex-1 rounded-md border border-reco-ok-border bg-reco-ok-bg px-2 py-1.5 text-[11.5px] text-reco-ok"
              >
                Accept
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => decide("reject")}
                className="flex-1 rounded-md border border-reco-bad-border bg-reco-bad-bg px-2 py-1.5 text-[11.5px] text-reco-bad"
              >
                Reject
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => decide("pending")}
                className="flex-1 rounded-md border border-reco-line px-2 py-1.5 text-[11.5px] text-reco-t2"
              >
                Pending
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
