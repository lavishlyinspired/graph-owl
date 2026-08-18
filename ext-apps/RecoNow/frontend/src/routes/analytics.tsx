import { useOutletContext } from "react-router-dom";
import type { WorkspaceState } from "../lib/workspace";
const MONTHS = [
  { label: "Apr", rate: "62%", exposure: "₹8.2L", h: "62%" },
  { label: "May", rate: "71%", exposure: "₹5.4L", h: "71%" },
  { label: "Jun", rate: "79%", exposure: "₹6.7L", h: "79%" },
  { label: "Jul", rate: "76%", exposure: "₹4.9L", h: "76%" },
  { label: "Aug", rate: "88%", exposure: "₹4.2L", h: "88%" },
];

const INSIGHT = {
  text: "Match rate improved 6 points since April, and follow-up volume fell 75%. Most of the gain came from cross-period auto-matching.",
  action: "Add to the client report",
};

export default function AnalyticsRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      <div>
        <h1 className="text-[20px] font-bold text-reco-t1">Analytics</h1>
        <p className="mt-1 text-[12.5px] text-reco-t4">Match rate vs ITC at risk over time.</p>
      </div>

      <div className="rounded-lg border border-reco-line bg-reco-panel px-5 py-5">
        <div className="mb-4 flex items-baseline justify-between">
          <span className="text-[13px] font-semibold text-reco-t1">MATCH RATE VS ITC AT RISK</span>
          <span className="font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">
            bar height = match rate, label = exposure that month
          </span>
        </div>

        <div className="flex items-end gap-6" style={{ height: 180 }}>
          {MONTHS.map((m) => (
            <div key={m.label} className="flex flex-1 flex-col items-center gap-2">
              <span className="font-mono text-[10.5px] text-reco-t1">{m.exposure}</span>
              <div
                className="w-full rounded-t-sm"
                style={{ height: m.h, background: m.label === "Aug" ? "#1c1b18" : "#dcd7cc" }}
              />
              <span className="font-mono text-[10px] text-reco-t4">{m.label}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="rounded-lg border border-reco-accent-border bg-reco-accent-bg px-5 py-4">
        <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
          INSIGHT
        </div>
        <p className="text-[12.5px] leading-relaxed text-reco-t1">{INSIGHT.text}</p>
        <button
          type="button"
          className="mt-3 rounded-md border border-reco-accent-border bg-reco-panel px-3 py-1.5 text-[11.5px] font-medium text-reco-accent"
        >
          {INSIGHT.action}
        </button>
      </div>

      <div className="rounded-lg border border-reco-line bg-reco-panel px-5 py-4">
        <div className="mb-3 font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">
          ROW ACTIONS
        </div>
        <div className="flex flex-wrap gap-2">
          {["Drill into period", "Export series", "Pin to dashboard"].map((a) => (
            <span
              key={a}
              className="rounded border border-reco-line px-2.5 py-1 text-[11.5px] text-reco-t2"
            >
              {a}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
