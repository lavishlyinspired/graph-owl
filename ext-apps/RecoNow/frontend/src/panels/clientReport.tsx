import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { GeneratedBadge } from "../components/GeneratedBadge";
import { fetchClientReport, type ClientReport } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

/** The monthly report a CA sends a client — the delivered mockup's own shape.
 *
 *  **Generated on request, not on load.** It costs a model round trip and a
 *  reader who opened this screen to check something else should not pay for
 *  one. Regenerate is offered for the same reason it is in the mockup: the
 *  first draft is a draft.
 *
 *  Copy and Download are the point of the screen. A report that can only be
 *  read where it was produced is not a deliverable. */
export default function ClientReportPanel() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [state, setState] = useState<"idle" | "working" | "done" | "failed">("idle");
  const [report, setReport] = useState<ClientReport | null>(null);
  const [copied, setCopied] = useState(false);

  const generate = () => {
    if (!clientId || !periodId) return;
    setState("working");
    setCopied(false);
    fetchClientReport(clientId, periodId)
      .then((result) => {
        setReport(result);
        setState("done");
      })
      .catch(() => setState("failed"));
  };

  const copy = () => {
    if (!report) return;
    void navigator.clipboard.writeText(report.report).then(() => setCopied(true));
  };

  const download = () => {
    if (!report) return;
    const blob = new Blob([report.report], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "client-report.txt";
    link.click();
    URL.revokeObjectURL(url);
  };

  if (!clientId || !periodId) {
    return <div className="p-6 text-[13px] text-reco-t4">Choose a client and a period.</div>;
  }

  return (
    <div className="space-y-4 p-6">
      <header>
        <h1 className="text-[19px] font-medium text-reco-t1">Client report</h1>
        <p className="mt-1 text-[13px] text-reco-t4">
          The month's reconciliation, written up for the client. Every figure comes from your
          own data.
        </p>
      </header>

      {state === "idle" && (
        <div className="rounded border border-reco-line p-8 text-center">
          <p className="mb-4 text-[12.5px] text-reco-t4">
            Summarises this period's findings, what is at risk, and what to do next.
          </p>
          <button
            type="button"
            onClick={generate}
            className="rounded border border-reco-line px-4 py-2 text-[12.5px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
          >
            Generate report
          </button>
        </div>
      )}

      {state === "working" && (
        <p className="text-[12.5px] text-reco-t4">Reading the period and writing it up…</p>
      )}

      {state === "failed" && (
        <p className="text-[12.5px] text-reco-bad">Could not produce a report.</p>
      )}

      {state === "done" && report && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <GeneratedBadge source={report.source} />
            <span className="flex-1" />
            <button type="button" onClick={copy} className={actionClass}>
              {copied ? "Copied" : "Copy"}
            </button>
            <button type="button" onClick={download} className={actionClass}>
              Download .txt
            </button>
            <button type="button" onClick={generate} className={actionClass}>
              Regenerate
            </button>
          </div>

          {report.note && (
            <p className="rounded border border-reco-line bg-reco-panel-2 px-3 py-2 text-[11.5px] leading-relaxed text-reco-t4">
              {report.note}
              {/* Shown, not hidden: a model that tried to state an unsupported
                  figure is exactly what a reviewer of an AI feature wants to
                  see, and this document leaves the building. */}
              {report.refusal && <span className="block mt-1">Refused: {report.refusal}</span>}
            </p>
          )}

          <pre className="overflow-x-auto whitespace-pre-wrap rounded border border-reco-line bg-white p-5 text-[12.5px] leading-relaxed text-reco-t2">
            {report.report}
          </pre>
        </>
      )}
    </div>
  );
}

const actionClass =
  "rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent";
