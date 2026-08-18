import { useEffect, useState } from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import { fetchDashboard, fetchGraphOwlStatus, type Dashboard, type GraphOwlStatus } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

/** What actually runs when a period is reconciled, and what does not.
 *
 *  This screen previously reported per-assistant run counts (1,247 / 843 /
 *  327 / 86), acceptance rates (99% / 97% / 100% / 91%), a token-spend
 *  breakdown, and — most seriously — three drafted supplier emails
 *  "awaiting your approval", naming Wipro Ltd and HCL Tech and quoting
 *  invoices WIP/2024/118 and HCL/2024/077 for ₹1.2L and ₹30K. None of those
 *  suppliers, invoices or amounts exist in any uploaded data. A queue of
 *  outbound emails that were never drafted, about invoices that were never
 *  seen, sitting under a button labelled Approve, is the most dangerous
 *  thing this console could show.
 *
 *  The reconciliation pipeline is real and is shown. The assistant layer is
 *  reported by `/api/health` as unavailable, and this says so rather than
 *  illustrating what it would look like. */

interface Stage {
  readonly owner: "GRAPHOWL" | "RECO";
  readonly name: string;
  readonly detail: string;
  readonly real: boolean;
}

const PIPELINE: readonly Stage[] = [
  {
    owner: "RECO",
    name: "Column mapping",
    detail: "Each uploaded file's columns are bound to GST fields, checked against the file's own headers.",
    real: true,
  },
  {
    owner: "RECO",
    name: "Ingest to graph",
    detail: "Mapped rows become RDF facts under a source scoped to this client and period.",
    real: true,
  },
  {
    owner: "GRAPHOWL",
    name: "Rule evaluation",
    detail: "The GST pack's SPARQL rules run over the graph and record findings with their evidence.",
    real: true,
  },
  {
    owner: "RECO",
    name: "Case creation",
    detail: "Each finding becomes a case carrying its rule, its evidence count and both amounts.",
    real: true,
  },
  {
    owner: "RECO",
    name: "Follow-up drafting",
    detail: "Writing supplier emails from case facts. Needs an assistant model.",
    real: false,
  },
];

export default function AgentsRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [status, setStatus] = useState<GraphOwlStatus | null>(null);
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [aiAvailable, setAiAvailable] = useState<boolean | null>(null);
  const navigate = useNavigate();

  useEffect(() => {
    fetchGraphOwlStatus().then(setStatus).catch(() => setStatus(null));
    fetch("/api/health")
      .then((r) => r.json())
      .then((h) => setAiAvailable(Boolean(h?.ai?.available)))
      .catch(() => setAiAvailable(null));
  }, []);

  useEffect(() => {
    if (!clientId || !periodId) return;
    let cancelled = false;
    fetchDashboard(clientId, periodId)
      .then((d) => !cancelled && setDashboard(d))
      .catch(() => !cancelled && setDashboard(null));
    return () => {
      cancelled = true;
    };
  }, [clientId, periodId]);

  return (
    <div className="p-6 pb-11">
      <div className="mb-4">
        <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">Assistants</h1>
        <p className="text-[12.5px] text-reco-t4">
          What runs when you reconcile a period. Every stage hands on fact ids, so a sentence on a
          case traces to the same evidence the matcher used.
        </p>
      </div>

      <div className="mb-3.5 rounded-[10px] border border-reco-line bg-white p-4">
        <div className="mb-3.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">PIPELINE</div>
        <div className="flex flex-wrap items-stretch gap-2">
          {PIPELINE.map((s, i) => (
            <div key={s.name} className="flex items-stretch">
              <div
                className={`w-[188px] rounded-lg border p-3 ${
                  s.real ? "border-reco-line-3 bg-[#fbfaf8]" : "border-dashed border-reco-line-3 bg-white"
                }`}
              >
                <div
                  className={`mb-1 font-mono text-[9px] tracking-[0.12em] ${
                    s.owner === "GRAPHOWL" ? "text-reco-accent-hi" : "text-reco-t5"
                  }`}
                >
                  {s.owner}
                </div>
                <div className={`text-[12.5px] ${s.real ? "text-reco-t1" : "text-reco-t4"}`}>
                  {s.name}
                </div>
                <div className="mt-1 text-[10.5px] leading-snug text-reco-t5">{s.detail}</div>
                {!s.real && (
                  <div className="mt-1.5 font-mono text-[9px] tracking-[0.1em] text-reco-amber">
                    NOT CONFIGURED
                  </div>
                )}
              </div>
              {i < PIPELINE.length - 1 && (
                <span className="self-center px-1.5 text-[12px] text-reco-t6">→</span>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-[1fr_320px] items-start gap-3.5">
        <div className="rounded-[10px] border border-reco-line bg-white p-4">
          <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
            LAST RECONCILIATION
          </div>
          {dashboard && dashboard.datasets.length > 0 ? (
            <>
              <div className="flex items-center justify-between border-b border-reco-row py-[7px]">
                <span className="text-[12px] text-reco-t2">Files read</span>
                <span className="font-mono text-[11px] text-reco-t4">
                  {dashboard.datasets.map((d) => `${d.kind} (${d.total_rows})`).join(" · ")}
                </span>
              </div>
              <div className="flex items-center justify-between border-b border-reco-row py-[7px]">
                <span className="text-[12px] text-reco-t2">Cases raised by the rules</span>
                <span className="font-mono text-[11px] text-reco-t1">{dashboard.case_count}</span>
              </div>
              <div className="flex items-center justify-between py-[7px]">
                <span className="text-[12px] text-reco-t2">Pack evaluating them</span>
                <span className="font-mono text-[11px] text-reco-t4">
                  {status?.pack ? `${status.pack.id} v${status.pack.version}` : "unknown"}
                </span>
              </div>
            </>
          ) : (
            <div className="py-6 text-center text-[12.5px] text-reco-t4">
              Nothing reconciled for this period yet.
              <button
                type="button"
                onClick={() => navigate("/pipeline")}
                className="ml-2 text-reco-accent"
              >
                Upload &amp; map →
              </button>
            </div>
          )}
        </div>

        <div className="flex flex-col gap-3.5">
          <div className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-2.5 flex items-center gap-2">
              <span
                className={`h-[7px] w-[7px] rounded-full ${
                  aiAvailable ? "bg-reco-ok" : "bg-reco-amber"
                }`}
              />
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                ASSISTANT LAYER
              </span>
            </div>
            <div className="text-[12.5px] leading-relaxed text-reco-t2">
              {aiAvailable === null
                ? "Checking whether an assistant model is configured…"
                : aiAvailable
                  ? "An assistant model is configured. Drafts it produces still queue for your approval; nothing leaves Reco Now on its own."
                  : "No assistant model is configured, so nothing is drafting text. The stages above that need one are marked NOT CONFIGURED, and there are no drafts waiting."}
            </div>
          </div>

          <div className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
              WAITING ON YOU
            </div>
            <div className="text-[12.5px] leading-relaxed text-reco-t2">
              {dashboard
                ? dashboard.pending_approvals > 0
                  ? `${dashboard.pending_approvals} item(s) need a decision before anything is sent.`
                  : "Nothing is waiting for approval. Anything that would leave Reco Now or change a number queues here first."
                : "—"}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
