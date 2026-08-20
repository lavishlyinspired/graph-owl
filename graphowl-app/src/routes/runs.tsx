import { useState } from "react";
import { strings } from "../lib/strings";

interface AgentRun {
  readonly id: string;
  readonly agent: string;
  readonly kind: string;
  readonly status: string;
  readonly statusColor: string;
  readonly input: string;
  readonly output: string;
  readonly tokens: number;
  readonly latency: string;
  readonly trigger: string;
  readonly tools: readonly string[];
  readonly cites: readonly string[];
  readonly destination: string;
}

const RUNS: readonly AgentRun[] = [
  {
    id: "run-8f3a",
    agent: "Contradiction reviewer",
    kind: "batch",
    status: "DONE",
    statusColor: "text-gowl-ok",
    input: "848 open exceptions across 3 suppliers",
    output: "17 contradictions confirmed, 312 dismissed as overlapping. Remaining 519 need human review — all flagged with source disagreement.",
    tokens: 14200,
    latency: "4.2 s",
    trigger: "nightly",
    tools: ["search_graph", "get_evidence", "get_entity"],
    cites: ["Supplier ABC · state: Maharashtra vs Gujarat", "XYZ Pvt Ltd · PAN: duplicate entry", "PQR Industries · invoice date out of period"],
    destination: "Exceptions",
  },
  {
    id: "run-2c91",
    agent: "Ontology suggester",
    kind: "scheduled",
    status: "DONE",
    statusColor: "text-gowl-ok",
    input: "14 unmapped columns from supplier_master.csv",
    output: "Proposed 8 new class mappings and 3 property links. All marked SUGGEST ONLY — awaiting human confirmation.",
    tokens: 8400,
    latency: "2.1 s",
    trigger: "nightly 02:00",
    tools: ["search_graph"],
    cites: ["column 'gstin' → Organization.gstin", "column 'supplier_state' → Organization.state"],
    destination: "Studio",
  },
  {
    id: "run-7e4b",
    agent: "Drift summariser",
    kind: "event",
    status: "DONE",
    statusColor: "text-gowl-ok",
    input: "Schema change detected in erp_invoices.xlsx",
    output: "3 columns renamed, 1 column type changed (string→date). Impact: 847 facts may need re-validation.",
    tokens: 6200,
    latency: "1.8 s",
    trigger: "drift signal",
    tools: ["get_entity", "get_history"],
    cites: ["erp_invoices.total → erp_invoices.total_amount (renamed)", "erp_invoices.date type change"],
    destination: "Pipeline",
  },
  {
    id: "run-1d5f",
    agent: "Contradiction reviewer",
    kind: "batch",
    status: "RUNNING",
    statusColor: "text-gowl-accent",
    input: "Processing period 2026-08 filings",
    output: "",
    tokens: 3100,
    latency: "1.2 s",
    trigger: "manual",
    tools: ["search_graph", "get_evidence"],
    cites: [],
    destination: "Exceptions",
  },
];

const FILTER_CHIPS = ["all", "batch", "scheduled", "event", "manual"];

export default function RunsRoute() {
  const [selectedRun, setSelectedRun] = useState<AgentRun | null>(null);
  const [activeChip, setActiveChip] = useState("all");

  const filteredRuns = activeChip === "all" ? RUNS : RUNS.filter((r) => r.kind === activeChip);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="flex-none border-b border-gowl-line bg-gowl-panel px-6 py-4">
        <div className="mb-3.5 flex items-end justify-between">
          <div>
            <h1 className="mb-1 text-[24px] font-semibold text-gowl-t1">{strings.runsTitle}</h1>
            <p className="text-[16.5px] text-gowl-t5">{strings.runsDescription}</p>
          </div>
          <div className="flex gap-2">
            <button type="button" className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t3 hover:border-gowl-hover">
              {strings.runsRunBatch}
            </button>
            <button type="button" className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t3 hover:border-gowl-hover">
              {strings.runsRegenerate}
            </button>
            <button type="button" className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on">
              {strings.runsOpenLanded}
            </button>
          </div>
        </div>

        {/* Filter chips */}
        <div className="flex flex-wrap gap-1.5">
          {FILTER_CHIPS.map((chip) => (
            <button
              key={chip}
              type="button"
              onClick={() => setActiveChip(chip)}
              className={`rounded-md border px-2.5 py-1 font-mono text-[14px] ${
                activeChip === chip
                  ? "border-gowl-accent-border bg-gowl-accent-bg text-gowl-accent"
                  : "border-gowl-line-2 bg-gowl-input text-gowl-t5 hover:border-gowl-hover"
              }`}
            >
              {chip}
            </button>
          ))}
        </div>
      </div>

      {/* Split layout: runs list + detail */}
      <div className="flex flex-1 min-h-0">
        {/* Runs list */}
        <div className="w-[396px] flex-none overflow-auto border-r border-gowl-line bg-gowl-panel">
          {filteredRuns.map((run) => (
            <button
              key={run.id}
              type="button"
              onClick={() => setSelectedRun(run)}
              className={`flex w-full border-b border-gowl-row text-left ${
                selectedRun?.id === run.id ? "bg-gowl-panel-2" : "hover:bg-gowl-panel-2"
              }`}
            >
              <div className={`w-[3px] flex-none ${
                run.status === "RUNNING" ? "bg-gowl-accent" : "bg-gowl-ok"
              }`} />
              <div className="min-w-0 flex-1 p-3">
                <div className="mb-1 flex items-center gap-2">
                  <span className="font-mono text-[14.5px] text-gowl-accent">{run.id}</span>
                  <span className="rounded border border-gowl-line-2 bg-gowl-input px-1.5 py-0.5 font-mono text-[13px] text-gowl-t5">{run.kind}</span>
                  <span className={`ml-auto font-mono text-[13px] ${run.statusColor}`}>{run.status}</span>
                </div>
                <div className="mb-0.5 text-[16.5px] text-gowl-t2">{run.agent}</div>
                <div className="mb-1.5 truncate font-mono text-[13.5px] text-gowl-t7">{run.input}</div>
                <div className="flex gap-3">
                  <span className="font-mono text-[13.5px] text-gowl-t7">{run.tokens.toLocaleString()} tok</span>
                  <span className="font-mono text-[13.5px] text-gowl-t7">{run.latency}</span>
                  <span className="font-mono text-[13.5px] text-gowl-t7">{run.trigger}</span>
                </div>
              </div>
            </button>
          ))}
        </div>

        {/* Detail panel */}
        <div className="flex-1 min-w-0 overflow-auto p-5">
          {selectedRun ? (
            <>
              {/* Run header */}
              <div className="mb-4 flex items-center gap-3">
                <span className="text-[20px] font-semibold text-gowl-t1">{selectedRun.agent}</span>
                <span className="font-mono text-[14.5px] text-gowl-t6">{selectedRun.id}</span>
                <span className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-2 py-0.5 font-mono text-[13px] text-gowl-accent">{selectedRun.kind}</span>
                <span className={`ml-auto font-mono text-[14px] ${selectedRun.statusColor}`}>{selectedRun.status}</span>
              </div>

              <div className="grid grid-cols-[1fr_300px] gap-4">
                {/* Left: output + cited facts */}
                <div className="flex flex-col gap-3.5">
                  {/* Output */}
                  <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
                    <div className="mb-3 flex items-baseline justify-between">
                      <span className="font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.runsOutputTitle}</span>
                      <span className="text-[15px] text-gowl-t7">{strings.runsOutputNote}</span>
                    </div>
                    <div className="rounded-md border border-gowl-accent-border bg-gowl-accent-bg p-3 text-[17px] text-gowl-t2 leading-relaxed">
                      {selectedRun.output || "Run in progress..."}
                    </div>
                    {selectedRun.destination && (
                      <div className="mt-3 flex items-center gap-2">
                        <span className="font-mono text-[14px] text-gowl-t6">landed in</span>
                        <span className="text-[16px] text-gowl-accent">{selectedRun.destination} →</span>
                      </div>
                    )}
                  </div>

                  {/* Cited facts */}
                  <div className="rounded-lg border border-gowl-line bg-gowl-panel p-4">
                    <div className="mb-3 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.runsCitedFactsTitle}</div>
                    {selectedRun.cites.length === 0 ? (
                      <div className="text-[16px] text-gowl-t5">No facts cited yet.</div>
                    ) : (
                      selectedRun.cites.map((cite, i) => (
                        <div key={i} className="flex gap-2.5 border-b border-gowl-row py-2 last:border-b-0">
                          <span className="mt-0.5 text-gowl-ok">✓</span>
                          <span className="font-mono text-[15px] text-gowl-t3 leading-relaxed">{cite}</span>
                        </div>
                      ))
                    )}
                    <div className="mt-3 text-[15.5px] leading-relaxed text-gowl-t6">{strings.runsCitedFactsNote}</div>
                  </div>
                </div>

                {/* Right: run detail + tools */}
                <div className="flex flex-col gap-3.5">
                  {/* Run detail */}
                  <div className="rounded-lg border border-gowl-line bg-gowl-panel p-3.5">
                    <div className="mb-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.runsDetailTitle}</div>
                    {[
                      { label: "trigger", value: selectedRun.trigger },
                      { label: "input", value: selectedRun.input },
                      { label: "tokens", value: selectedRun.tokens.toLocaleString() },
                      { label: "latency", value: selectedRun.latency },
                    ].map((row, i) => (
                      <div key={row.label} className={`grid grid-cols-[78px_1fr] gap-2.5 py-1.5 ${
                        i < 3 ? "border-b border-gowl-row" : ""
                      }`}>
                        <span className="font-mono text-[13.5px] text-gowl-t6">{row.label}</span>
                        <span className="text-[15.5px] text-gowl-t3">{row.value}</span>
                      </div>
                    ))}
                  </div>

                  {/* Tools called */}
                  <div className="rounded-lg border border-gowl-line bg-gowl-panel p-3.5">
                    <div className="mb-2.5 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.runsToolsTitle}</div>
                    <div className="flex flex-wrap gap-1.5">
                      {selectedRun.tools.map((tool) => (
                        <span key={tool} className="rounded bg-gowl-input border border-gowl-line-2 px-1.5 py-0.5 font-mono text-[14px] text-gowl-t4">
                          {tool}
                        </span>
                      ))}
                    </div>
                    <div className="mt-3 text-[15.5px] leading-relaxed text-gowl-t6">{strings.runsToolsNote}</div>
                  </div>
                </div>
              </div>
            </>
          ) : (
            <div className="flex h-full items-center justify-center text-[17px] text-gowl-t5">
              {strings.runsSelectRun}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
