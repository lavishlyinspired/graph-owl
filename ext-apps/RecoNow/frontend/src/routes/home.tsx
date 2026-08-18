import { useEffect, useState } from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import { fetchDashboard, type Dashboard } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number): string {
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

const BRIEFING = {
  text: "August is two decisions away from close. 24 IMS records have no action and will be deemed accepted at 3B, six of them the disputed XYZ amounts. Separately, ₹4.2 L sits with 11 suppliers who have never been contacted this cycle.",
  cites: "18 facts cited · written 07:00 today",
  a1: "Hold the 6 disputed IMS records",
  a2: "Draft the 11 follow-ups",
};

const ITC_CARDS = [
  { label: "RECONCILED · ELIGIBLE PER 2B", value: "₹2.84 Cr", sub: "9,842 invoices matched on all fields", color: "#2f6b4d", border: "#e3e0d9", pct: "88%" },
  { label: "NEEDS REVIEW", value: "₹18.7 L", sub: "327 invoices across 6 reason codes", color: "#a86a2c", border: "#f0dcc2", pct: "42%" },
  { label: "AT RISK", value: "₹12.4 L", sub: "146 invoices · 42 suppliers to chase", color: "#a13f28", border: "#eed7d1", pct: "28%" },
] as const;

const ACTIONS = [
  { title: "Supplier has not filed GSTR-1", sub: "42 invoices across 11 suppliers · oldest 38 days", amount: "₹4.2L", count: "42 inv", color: "#a13f28", route: "atrisk" as const },
  { title: "Tax amount differs from 2B", sub: "18 invoices · average delta ₹1,720", amount: "₹3.1L", count: "18 inv", color: "#c9803a", route: "case" as const },
  { title: "Present in 2B, missing in books", sub: "9 invoices · likely unrecorded purchases", amount: "₹1.8L", count: "9 inv", color: "#5b6bb5", route: "register" as const },
  { title: "Cross-period — supplier filed late", sub: "11 invoices dated July, appearing in August 2B", amount: "₹1.4L", count: "11 inv", color: "#6b4fa8", route: "crossperiod" as const },
  { title: "IMS decision pending", sub: "6 records will be deemed accepted at 3B filing", amount: "₹0.9L", count: "6 rec", color: "#2f6b4d", route: "ims" as const },
] as const;

const ENGINE = [
  { n: "47", label: "Cross-period matches", detail: "Invoice → supplier filing → period, resolved without a human." },
  { n: "96", label: "Supplier identities merged", detail: "Same vendor under two GSTINs or trade names." },
  { n: "9", label: "GSTIN typos caught", detail: "Transposition detected by blocking, fix suggested." },
  { n: "848", label: "Cases with evidence", detail: "Each carries its citations and rule reference." },
] as const;

const PERIOD_STATE = [
  { label: "Books import", value: "Imported 15 Aug", color: "#2f6b4d" },
  { label: "GSTR-1", value: "Available", color: "#2f6b4d" },
  { label: "GSTR-2B", value: "Generated 14 Aug", color: "#2f6b4d" },
  { label: "IMS", value: "24 pending actions", color: "#a86a2c" },
  { label: "Reconciliation", value: "Completed", color: "#2f6b4d" },
  { label: "GSTR-3B", value: "Not filed", color: "#6f6b62" },
] as const;

const TREND = [
  { m: "Apr", pct: "88%", h: "40px", color: "#dcd7cc" },
  { m: "May", pct: "91%", h: "47px", color: "#dcd7cc" },
  { m: "Jun", pct: "93%", h: "52px", color: "#dcd7cc" },
  { m: "Jul", pct: "92%", h: "50px", color: "#dcd7cc" },
  { m: "Aug", pct: "94%", h: "56px", color: "#1c1b18" },
] as const;

const AGENT_MINI = [
  { label: "Explanations generated", value: "848" },
  { label: "Follow-ups drafted", value: "412" },
  { label: "Sent after your review", value: "388" },
  { label: "Token spend", value: "₹9,840" },
] as const;

export default function HomeRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [bannerDismissed, setBannerDismissed] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchDashboard(clientId, periodId)
      .then(setDashboard)
      .catch(() => setDashboard(null));
  }, [clientId, periodId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  const caseCount = dashboard?.case_count ?? 38;
  const exposure = dashboard?.total_exposure ?? 420000;
  const hasBanner = !bannerDismissed;

  return (
    <div className="p-6 pb-11">
      {/* 1. Header */}
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">August 2026 close</h1>
          <p className="text-[12.5px] text-reco-t4">
            Reconciliation complete. {caseCount} invoices still need a decision before 3B.
          </p>
        </div>
        <div className="flex gap-2">
          <button type="button" className="rounded-[7px] border border-reco-line-3 bg-white px-3.5 py-[7px] text-[12.5px] text-reco-t1 hover:border-reco-t5">
            Re-run reconciliation
          </button>
          <button
            type="button"
            onClick={() => navigate("/r/register")}
            className="rounded-[7px] bg-reco-t0 px-3.5 py-[7px] text-[12.5px] font-semibold text-white"
          >
            Open work queue
          </button>
        </div>
      </div>

      {/* 2. Banner */}
      {hasBanner && (
        <div className="mb-4 flex items-center justify-between rounded-[8px] border border-reco-accent-border bg-reco-accent-bg px-4 py-3 text-[12.5px] text-reco-t2">
          <span>• Reconciliation run completed at 06:42. Briefing updated.</span>
          <button type="button" onClick={() => setBannerDismissed(true)} className="text-[11.5px] text-reco-t4 hover:text-reco-t2">
            Dismiss
          </button>
        </div>
      )}

      {/* 3. Briefing card */}
      <div className="mb-4 rounded-[10px] border border-[#e6dcf7] bg-white p-4">
        <div className="mb-2.5 flex items-center gap-2">
          <span className="h-[7px] w-[7px] rounded-[2px] bg-reco-purple" />
          <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-purple">CLOSE-READINESS BRIEFING</span>
          <span className="ml-auto font-mono text-[9.5px] text-reco-t5">{BRIEFING.cites}</span>
        </div>
        <p className="mb-3 text-[12.5px] leading-relaxed text-reco-t2">{BRIEFING.text}</p>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => navigate("/r/ims")}
            className="rounded-[7px] border border-reco-line-3 bg-white px-3 py-[6px] text-[11.5px] text-reco-t1 hover:border-reco-t5"
          >
            {BRIEFING.a1}
          </button>
          <button
            type="button"
            onClick={() => navigate("/r/followups")}
            className="rounded-[7px] border border-reco-line-3 bg-white px-3 py-[6px] text-[11.5px] text-reco-t1 hover:border-reco-t5"
          >
            {BRIEFING.a2}
          </button>
          <button
            type="button"
            onClick={() => navigate("/r/agents")}
            className="ml-auto text-[11px] text-reco-t4 hover:text-reco-t2"
          >
            How this was written
          </button>
        </div>
      </div>

      {/* 4. ITC risk cards */}
      <div className="mb-4 grid grid-cols-3 gap-3">
        {ITC_CARDS.map((card) => (
          <div key={card.label} className="rounded-[10px] border bg-white p-4" style={{ borderColor: card.border }}>
            <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">{card.label}</div>
            <div className="font-mono text-[21px] font-medium" style={{ color: card.color }}>{card.value}</div>
            <div className="mt-1.5 text-[11px] text-reco-t4">{card.sub}</div>
            <div className="mt-2.5 h-[5px] overflow-hidden rounded-full bg-reco-row">
              <div className="h-full rounded-full" style={{ width: card.pct, background: card.color }} />
            </div>
          </div>
        ))}
      </div>

      {/* 5. Two-column layout */}
      <div className="grid grid-cols-[1.25fr_1fr] items-start gap-3.5">
        {/* LEFT COLUMN */}
        <div className="flex flex-col gap-3.5">
          {/* Actions list */}
          <div className="rounded-[10px] border border-reco-line bg-white">
            <div className="flex items-center justify-between border-b border-reco-line px-4 py-2.5">
              <span className="text-[13px] font-semibold text-reco-t1">What needs a decision</span>
              <span className="font-mono text-[9.5px] text-reco-t5">sorted by ITC exposure</span>
            </div>
            {ACTIONS.map((a) => (
              <button
                key={a.title}
                type="button"
                onClick={() => navigate(`/r/${a.route}`)}
                className="flex w-full items-center gap-3.5 border-b border-reco-row px-4 py-3 text-left last:border-b-0 hover:bg-reco-panel-2"
              >
                <span className="h-[7px] w-[7px] flex-none rounded-[2px]" style={{ background: a.color }} />
                <div className="flex-1">
                  <div className="text-[12.5px] font-medium text-reco-t1">{a.title}</div>
                  <div className="mt-[2px] text-[11px] text-reco-t4">{a.sub}</div>
                </div>
                <div className="text-right">
                  <div className="font-mono text-[12.5px] text-reco-t1">{a.amount}</div>
                  <div className="font-mono text-[10px] text-reco-t5">{a.count}</div>
                </div>
                <span className="text-[11px] text-reco-t5">›</span>
              </button>
            ))}
          </div>

          {/* Engine stats */}
          <div className="rounded-[10px] border border-reco-accent-border bg-white p-4">
            <div className="mb-3 flex items-center gap-2">
              <span className="h-[7px] w-[7px] rounded-[2px] bg-reco-accent" />
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
                WHAT THE GRAPH ENGINE DID THIS PERIOD
              </span>
            </div>
            <div className="mb-3 grid grid-cols-4 gap-3">
              {ENGINE.map((e) => (
                <div key={e.label}>
                  <div className="font-mono text-[18px] font-medium text-reco-t1">{e.n}</div>
                  <div className="mt-0.5 text-[11px] font-medium text-reco-t2">{e.label}</div>
                  <div className="mt-1 text-[10px] leading-snug text-reco-t4">{e.detail}</div>
                </div>
              ))}
            </div>
            <div className="border-t border-reco-accent-border pt-2.5 text-[10.5px] text-reco-t4">
              Every number above is traceable to individual graph facts — no conclusion appears here without evidence behind it.
            </div>
          </div>
        </div>

        {/* RIGHT COLUMN */}
        <div className="flex flex-col gap-3.5">
          {/* Period state */}
          <div className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-3 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">PERIOD STATE</div>
            {PERIOD_STATE.map((ps) => (
              <div key={ps.label} className="flex items-center justify-between border-b border-reco-row py-[7px] last:border-b-0">
                <span className="text-[12px] text-reco-t2">{ps.label}</span>
                <span className="font-mono text-[11.5px]" style={{ color: ps.color }}>{ps.value}</span>
              </div>
            ))}
            <div className="mt-3 rounded-[8px] border border-reco-line bg-reco-panel-2 px-3.5 py-3">
              <div className="mb-2 text-[12px] text-reco-t1">
                {caseCount} open exceptions · {formatRupees(exposure)} unresolved exposure
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => navigate("/r/exceptions")}
                  className="rounded-[7px] border border-reco-line-3 bg-white px-3 py-[5px] text-[11px] text-reco-t1 hover:border-reco-t5"
                >
                  Review open items
                </button>
                <button
                  type="button"
                  onClick={() => navigate("/r/periods")}
                  className="rounded-[7px] bg-reco-t0 px-3 py-[5px] text-[11px] text-white"
                >
                  Close period
                </button>
              </div>
            </div>
          </div>

          {/* Trend chart */}
          <div className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-4 flex items-baseline justify-between">
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">MATCH RATE</span>
              <span className="font-mono text-[9.5px] text-reco-t5">Apr → Aug</span>
            </div>
            <div className="flex h-[120px] items-end gap-2.5">
              {TREND.map((t) => (
                <div key={t.m} className="flex flex-1 flex-col items-center gap-[7px] justify-end h-full">
                  <span className="font-mono text-[10.5px] text-reco-t2">{t.pct}</span>
                  <div className="w-full rounded-t-[4px]" style={{ height: t.h, background: t.color }} />
                  <span className="font-mono text-[10px] text-reco-t5">{t.m}</span>
                </div>
              ))}
            </div>
          </div>

          {/* Agent mini */}
          <button
            type="button"
            onClick={() => navigate("/r/agents")}
            className="rounded-[10px] border border-reco-line bg-white p-4 text-left hover:border-reco-line-3"
          >
            <div className="mb-3 flex items-center justify-between">
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">ASSISTANTS THIS MONTH</span>
              <span className="text-[11px] text-reco-accent">Open →</span>
            </div>
            <div className="grid grid-cols-2 gap-x-6 gap-y-2">
              {AGENT_MINI.map((a) => (
                <div key={a.label} className="flex items-baseline justify-between">
                  <span className="text-[11.5px] text-reco-t3">{a.label}</span>
                  <span className="font-mono text-[12px] text-reco-t1">{a.value}</span>
                </div>
              ))}
            </div>
            <div className="mt-3 border-t border-reco-row pt-2.5 text-[10.5px] text-reco-t4">
              Drafts and explanations only. No filing, no ITC decision, no email sent without you.
            </div>
          </button>
        </div>
      </div>
    </div>
  );
}
