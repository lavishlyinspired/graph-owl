import { useOutletContext } from "react-router-dom";
import type { WorkspaceState } from "../lib/workspace";
const PIPELINE = [
  { tag: "GRAPH · EXTRACT", name: "Supplier identity", detail: "GSTIN → legal name, PAN, state", color: "#5b6bb5" },
  { tag: "GRAPH · MATCH", name: "Invoice matcher", detail: "Books ↔ portal by GSTIN + number", color: "#5b6bb5" },
  { tag: "GRAPH · FLAG", name: "Rule evaluator", detail: "SPARQL findings per pack rule", color: "#5b6bb5" },
  { tag: "RECO", name: "Case builder", detail: "Groups findings into actionable cases", color: "#2f6b4d" },
  { tag: "RECO", name: "Follow-up drafter", detail: "Drafts supplier emails from case facts", color: "#2f6b4d" },
];

const ASSISTANTS = [
  {
    name: "Identity resolver",
    mode: "AUTO",
    surface: "register · suppliers",
    desc: "Merges duplicate GSTIN records into one canonical identity",
    trigger: "On ingest",
    next: "Next: 12:00",
    runs: "1,247",
    tokens: "18K tok",
    accepted: "99%",
  },
  {
    name: "Invoice matcher",
    mode: "AUTO",
    surface: "register · queue",
    desc: "Pairs books rows with GSTR-2B rows by (GSTIN, invoice no)",
    trigger: "On ingest",
    next: "Next: 12:00",
    runs: "843",
    tokens: "42K tok",
    accepted: "97%",
  },
  {
    name: "Rule evaluator",
    mode: "AUTO",
    surface: "exceptions · cases",
    desc: "Runs SPARQL findings against loaded packs (GST, hospitality)",
    trigger: "After matcher",
    next: "Next: 12:00",
    runs: "843",
    tokens: "31K tok",
    accepted: "—",
  },
  {
    name: "Case summariser",
    mode: "ON DEMAND",
    surface: "case detail",
    desc: "Writes one-sentence case explanation from cited evidence",
    trigger: "On case creation",
    next: "Next: on demand",
    runs: "327",
    tokens: "9K tok",
    accepted: "100%",
  },
  {
    name: "Follow-up drafter",
    mode: "ON DEMAND",
    surface: "follow-ups · agents",
    desc: "Drafts supplier email from case + invoice facts",
    trigger: "On request",
    next: "Next: on demand",
    runs: "86",
    tokens: "14K tok",
    accepted: "91%",
  },
];

const AI_MAP = [
  { surface: "Upload & map", use: "Column auto-mapping from header names", mode: "LOCAL" },
  { surface: "Identity resolver", use: "GSTIN → legal name, PAN, state", mode: "AUTO" },
  { surface: "Invoice matcher", use: "Pairs books ↔ portal by GSTIN + number", mode: "AUTO" },
  { surface: "Rule evaluator", use: "SPARQL findings against loaded packs", mode: "AUTO" },
  { surface: "Case summariser", use: "One-sentence case explanation", mode: "ON DEMAND" },
  { surface: "Follow-up drafter", use: "Supplier email from case facts", mode: "ON DEMAND" },
  { surface: "Ask about this period", use: "Grounded QA over case + evidence", mode: "ON DEMAND" },
];

const DRAFTS = [
  {
    supplier: "Wipro Ltd",
    reason: "PotentialMismatch",
    amount: "₹1.2L",
    body: "Your invoice WIP/2024/118 for ₹1,20,000 does not appear in your GSTR-2B. Please confirm filing status.",
    cites: "fact:inv:WIP/2024/118",
  },
  {
    supplier: "HCL Tech",
    reason: "PotentialMismatch",
    amount: "₹30K",
    body: "Invoice HCL/2024/077 for ₹30,000 is absent from your GSTR-2B. Kindly check and file if pending.",
    cites: "fact:inv:HCL/2024/077",
  },
  {
    supplier: "Flipkart Internet",
    reason: "MissingInBooks",
    amount: "₹45K",
    body: "GSTR-2B shows invoice FLIP/2024/012 for ₹45,000 which is not in our purchase register. Please clarify.",
    cites: "fact:inv:FLIP/2024/012",
  },
];

const TOKEN_SPLIT = [
  { label: "Identity resolver", pct: "38%", color: "#5b6bb5" },
  { label: "Invoice matcher", pct: "31%", color: "#5b6bb5" },
  { label: "Rule evaluator", pct: "18%", color: "#5b6bb5" },
  { label: "Case summariser", pct: "8%", color: "#2f6b4d" },
  { label: "Follow-up drafter", pct: "5%", color: "#2f6b4d" },
];

export default function AgentsRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="flex gap-4 p-6">
      <div className="flex flex-1 flex-col gap-4">
        <div>
          <h1 className="text-[20px] font-bold text-reco-t1">Assistants</h1>
          <p className="mt-1 text-[12.5px] text-reco-t4">
            The first three stages are GraphOWL agents; Reco owns the last two. Each hands over
            fact ids, so the sentence you read on a case is traceable to the same evidence the
            matcher used.
          </p>
        </div>

        {/* Pipeline */}
        <div className="rounded-lg border border-reco-line bg-reco-panel px-5 py-4">
          <div className="mb-3 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
            PIPELINE
          </div>
          <div className="flex items-stretch gap-1">
            {PIPELINE.map((p, i) => (
              <div key={p.name} className="flex flex-1 items-center">
                <div
                  className="flex-1 rounded-lg border px-3 py-2.5"
                  style={{ borderColor: p.color + "40", background: p.color + "0a" }}
                >
                  <div
                    className="mb-1 font-mono text-[8.5px] tracking-[0.12em]"
                    style={{ color: p.color }}
                  >
                    {p.tag}
                  </div>
                  <div className="text-[11.5px] text-reco-t1">{p.name}</div>
                  <div className="mt-1 text-[10px] leading-snug text-reco-t4">{p.detail}</div>
                </div>
                {i < PIPELINE.length - 1 && (
                  <span className="w-4 text-center text-[11px] text-reco-t5">→</span>
                )}
              </div>
            ))}
          </div>
        </div>

        {/* Assistants table */}
        <div className="overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
          <div className="grid grid-cols-[1.5fr_150px_118px_92px_140px] gap-3 border-b border-reco-line bg-reco-panel-2 px-4 py-2 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5">
            <span>ASSISTANT · WHERE IT SHOWS UP</span>
            <span>TRIGGER</span>
            <span>RUNS</span>
            <span className="text-right">ACCEPTED</span>
            <span>CONTROL</span>
          </div>
          {ASSISTANTS.map((a) => (
            <div
              key={a.name}
              className="grid grid-cols-[1.5fr_150px_118px_92px_140px] items-center gap-3 border-b border-reco-line-2 px-4 py-3"
            >
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-[12.5px] font-medium text-reco-t1">{a.name}</span>
                  <span
                    className="rounded px-1.5 py-0.5 font-mono text-[9px]"
                    style={{
                      background: a.mode === "AUTO" ? "#f4f6fb" : "#fdf3e7",
                      color: a.mode === "AUTO" ? "#41508f" : "#a86a2c",
                      border: `1px solid ${a.mode === "AUTO" ? "#dfe3f2" : "#f0dcc2"}`,
                    }}
                  >
                    {a.mode}
                  </span>
                </div>
                <div className="mt-0.5 text-[11.5px] text-reco-t4">{a.desc}</div>
                <div className="mt-0.5 font-mono text-[9.5px] text-reco-accent">{a.surface}</div>
              </div>
              <div>
                <div className="text-[11.5px] text-reco-t2">{a.trigger}</div>
                <div className="mt-0.5 font-mono text-[9.5px] text-reco-t5">{a.next}</div>
              </div>
              <div>
                <div className="font-mono text-[12px] text-reco-t1">{a.runs}</div>
                <div className="mt-0.5 font-mono text-[9.5px] text-reco-t5">{a.tokens}</div>
              </div>
              <span className="text-right font-mono text-[12px] text-reco-t1">{a.accepted}</span>
              <div className="flex items-center gap-2">
                <div className="h-[18px] w-[32px] rounded-full border border-reco-line bg-reco-panel-2 px-0.5">
                  <div className="h-[12px] w-[12px] rounded-full bg-reco-t1" />
                </div>
                <span className="cursor-pointer text-[11px] text-reco-accent">See output</span>
              </div>
            </div>
          ))}
        </div>

        {/* Latest output */}
        <div className="rounded-lg border border-reco-accent-border bg-reco-panel px-5 py-4">
          <div className="mb-3 flex items-center gap-2.5">
            <span className="h-2 w-2 rounded-full bg-reco-accent" />
            <span className="text-[13px] font-semibold text-reco-t1">
              Latest output · Invoice matcher
            </span>
            <span className="rounded border border-reco-accent-border bg-reco-accent-bg px-1.5 py-0.5 font-mono text-[9px] text-reco-accent">
              AUTO
            </span>
            <span className="ml-auto font-mono text-[10px] text-reco-t5">12 Aug 12:00</span>
          </div>
          <div className="rounded-lg border border-reco-accent-border bg-reco-accent-bg p-4 text-[13px] text-reco-t1 leading-relaxed">
            Matched 8 of 10 books rows to portal rows by (GSTIN, invoice_no). 2 unmatched: WIP/2024/118 (Wipro), HCL/2024/077 (HCL Tech). 1 portal-only: FLIP/2024/012 (Flipkart).
          </div>
          <div className="mt-3 flex items-center gap-3">
            <span className="font-mono text-[10px] text-reco-t4">you already see this at</span>
            <span className="text-[12px] text-reco-accent">Review queue →</span>
            <span className="ml-auto font-mono text-[9.5px] text-reco-accent">AUTO</span>
          </div>
          <div className="mt-4 border-t border-reco-line-2 pt-3">
            <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
              READ FROM
            </div>
            {["fact:inv:WIP/2024/118", "fact:inv:HCL/2024/077", "fact:inv:FLIP/2024/012"].map((c) => (
              <div key={c} className="flex items-center gap-2.5 border-b border-reco-line-2 py-1.5">
                <span className="text-[10px] text-reco-ok">✓</span>
                <span className="font-mono text-[11px] text-reco-t2">{c}</span>
              </div>
            ))}
          </div>
        </div>

        {/* Where AI appears */}
        <div className="overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
          <div className="flex items-center justify-between border-b border-reco-line px-4 py-2.5">
            <span className="text-[13px] font-semibold text-reco-t1">
              Where AI appears in Reco Now
            </span>
            <span className="text-[11.5px] text-reco-t4">
              no chat window — it works inside the screens you already use
            </span>
          </div>
          {AI_MAP.map((m) => (
            <div
              key={m.surface}
              className="grid grid-cols-[190px_1fr_110px] items-center gap-3.5 border-b border-reco-line-2 px-4 py-2.5"
            >
              <span className="text-[12.5px] font-medium text-reco-t1">{m.surface}</span>
              <span className="text-[12px] text-reco-t2">{m.use}</span>
              <span className="w-fit rounded bg-reco-accent-bg px-1.5 py-0.5 font-mono text-[9.5px] text-reco-accent border border-reco-accent-border">
                {m.mode}
              </span>
            </div>
          ))}
        </div>
      </div>

      {/* Right column */}
      <div className="flex w-[280px] flex-col gap-4">
        {/* Token split */}
        <div className="rounded-lg border border-reco-line bg-reco-panel px-4 py-4">
          <div className="mb-3 text-[13px] font-semibold text-reco-t1">Where the tokens went</div>
          {TOKEN_SPLIT.map((t) => (
            <div key={t.label} className="mb-3">
              <div className="mb-1 flex justify-between">
                <span className="text-[11.5px] text-reco-t2">{t.label}</span>
                <span className="font-mono text-[10.5px] text-reco-t1">{t.pct}</span>
              </div>
              <div className="h-1.5 w-full rounded-full bg-reco-line-2">
                <div className="h-full rounded-full" style={{ width: t.pct, background: t.color }} />
              </div>
            </div>
          ))}
        </div>

        {/* Awaiting approval */}
        <div className="rounded-lg border border-reco-line bg-reco-panel px-4 py-4">
          <div className="mb-3 flex items-center justify-between">
            <span className="text-[13px] font-semibold text-reco-t1">Awaiting your approval</span>
            <span className="text-[11.5px] text-reco-t4">3 drafts · ₹1.95L</span>
          </div>
          {DRAFTS.map((d) => (
            <div
              key={d.supplier}
              className="mb-2.5 rounded-lg border border-reco-line-2 bg-reco-surface px-3.5 py-3"
            >
              <div className="mb-1.5 flex items-center gap-2">
                <span className="text-[12.5px] font-semibold text-reco-t1">{d.supplier}</span>
                <span
                  className="rounded border px-1.5 py-0.5 font-mono text-[9.5px]"
                  style={{
                    background: "#fdf3e7",
                    color: "#a86a2c",
                    borderColor: "#f0dcc2",
                  }}
                >
                  {d.reason}
                </span>
                <span className="ml-auto font-mono text-[11.5px] text-reco-bad">{d.amount}</span>
              </div>
              <div className="border-l-2 border-reco-accent-border pl-2.5 text-[12px] text-reco-t2 leading-relaxed">
                {d.body}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <span className="font-mono text-[10px] text-reco-t5">{d.cites}</span>
                <div className="ml-auto flex gap-1.5">
                  <span className="cursor-pointer rounded border border-reco-line px-2.5 py-1 text-[11.5px] text-reco-t2">
                    Edit
                  </span>
                  <span className="cursor-pointer rounded bg-reco-t1 px-2.5 py-1 text-[11.5px] text-reco-surface">
                    Approve &amp; send
                  </span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
