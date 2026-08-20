import { strings } from "../lib/strings";

/** Mockup's agents screen — pipeline visualization, agent roster with
 *  toggles, model usage breakdown, guardrails, and MCP tool cloud.
 *  All data is mock, matching the mockup's `V` object exactly. */

interface KpiStat {
  readonly label: string;
  readonly value: string;
  readonly sub: string;
  readonly color?: string;
}

const KPI_STATS: readonly KpiStat[] = [
  { label: "AGENT RUNS 24H", value: "2,828", sub: "94% from Reco Now" },
  { label: "TOKENS 24H", value: "6.4 M", sub: "in 5.1M / out 1.3M" },
  { label: "GRAPH-GROUNDED", value: "98.2%", sub: "answers with citations", color: "text-gowl-ok" },
  { label: "MEDIAN LATENCY", value: "2.8 s", sub: "p95 6.1 s" },
  { label: "SPEND MTD", value: "$1,284", sub: "64% of budget", color: "text-gowl-amber" },
];

interface PipelineStage {
  readonly tag: string;
  readonly name: string;
  readonly detail: string;
  readonly borderColor: string;
  readonly bgColor: string;
}

const PIPELINE_STAGES: readonly PipelineStage[] = [
  { tag: "INGEST", name: "Finding raised", detail: "Rule or query fires in the engine", borderColor: "border-gowl-line-2", bgColor: "bg-gowl-panel-2" },
  { tag: "STAGE 1", name: "Retriever", detail: "Pulls entity, neighbours, evidence", borderColor: "border-gowl-accent-border", bgColor: "bg-gowl-accent-deep" },
  { tag: "STAGE 2", name: "Reasoner", detail: "Builds the chain, scores confidence", borderColor: "border-gowl-accent-border", bgColor: "bg-gowl-accent-deep" },
  { tag: "STAGE 3", name: "Explainer", detail: "Writes the sentence, cites fact ids", borderColor: "border-gowl-accent-border", bgColor: "bg-gowl-accent-deep" },
  { tag: "STAGE 4", name: "Actioner", detail: "Drafts the follow-up or suggestion", borderColor: "border-gowl-amber-border", bgColor: "bg-[var(--gowl-amber-deep)]" },
  { tag: "GATE", name: "Human decision", detail: "In Reco Now, or here for graph edits", borderColor: "border-gowl-ok-border", bgColor: "bg-gowl-ok-bg" },
];

type AgentMode = "AUTOMATIC" | "HUMAN GATE" | "SUGGEST ONLY";

interface AgentRow {
  readonly name: string;
  readonly desc: string;
  readonly trigger: string;
  readonly next: string;
  readonly runs: string;
  readonly tokens: string;
  readonly grounding: string;
  readonly mode: AgentMode;
  readonly on: boolean;
}

const AGENT_ROWS: readonly AgentRow[] = [
  { name: "Retriever", desc: "Stage 1 · pulls entity, neighbours and evidence", trigger: "On every finding", next: "continuous · last 40 s ago", runs: "1,204", tokens: "2.9 M", grounding: "99.4%", mode: "AUTOMATIC", on: true },
  { name: "Reasoner", desc: "Stage 2 · builds the chain and scores confidence", trigger: "On every finding", next: "continuous · last 40 s ago", runs: "1,188", tokens: "2.1 M", grounding: "99.1%", mode: "AUTOMATIC", on: true },
  { name: "Explainer", desc: "Stage 3 · writes the cited sentence Reco shows", trigger: "On every finding", next: "continuous · last 1 min ago", runs: "988", tokens: "1.4 M", grounding: "98.6%", mode: "AUTOMATIC", on: true },
  { name: "Actioner", desc: "Stage 4 · drafts the follow-up from case facts", trigger: "On finding + exposure > 10k", next: "queued · 6 waiting", runs: "412", tokens: "0.9 M", grounding: "96.0%", mode: "HUMAN GATE", on: true },
  { name: "Ontology suggester", desc: "Off-pipeline · proposes classes for unmapped fields", trigger: "Nightly 02:00", next: "next run in 5 h", runs: "186", tokens: "0.4 M", grounding: "88.2%", mode: "SUGGEST ONLY", on: true },
  { name: "Drift summariser", desc: "Off-pipeline · explains what a schema change breaks", trigger: "On drift signal", next: "event · 8 open", runs: "38", tokens: "0.1 M", grounding: "97.3%", mode: "SUGGEST ONLY", on: true },
];

const MODE_STYLES: Record<AgentMode, { bg: string; text: string }> = {
  AUTOMATIC: { bg: "bg-gowl-ok-bg", text: "text-gowl-ok" },
  "HUMAN GATE": { bg: "bg-gowl-amber-bg", text: "text-gowl-amber" },
  "SUGGEST ONLY": { bg: "bg-gowl-row", text: "text-gowl-t5" },
};

interface TraceStep {
  readonly tool: string;
  readonly args: string;
  readonly tokens: string;
  readonly latency: string;
}

const TRACE_STEPS: readonly TraceStep[] = [
  { tool: "search_graph", args: 'q="Supplier ABC"', tokens: "210", latency: "180 ms" },
  { tool: "get_entity", args: "gst:Supplier/27AABCU9603R1ZM", tokens: "640", latency: "96 ms" },
  { tool: "find_path", args: "from=Supplier ABC to=Company XYZ maxHops=6", tokens: "820", latency: "420 ms" },
  { tool: "get_evidence", args: "edge=sameAs · 3 documents", tokens: "610", latency: "140 ms" },
  { tool: "reason_explain", args: "chain for sameAs · 5 steps", tokens: "560", latency: "310 ms" },
  { tool: "llm.compose", args: "phrase answer from 6 retrieved facts", tokens: "340", latency: "1.9 s" },
];

interface ModelUsage {
  readonly name: string;
  readonly tokens: string;
  readonly width: string;
  readonly color: string;
  readonly use: string;
}

const MODEL_USAGE: readonly ModelUsage[] = [
  { name: "Reasoning model · explanations", tokens: "4.1 M", width: "64%", color: "var(--gowl-accent)", use: "case explanations, path narration" },
  { name: "Small model · classification", tokens: "1.8 M", width: "28%", color: "var(--gowl-ok)", use: "reason-code tagging, routing" },
  { name: "Embedding model · retrieval", tokens: "0.5 M", width: "8%", color: "var(--gowl-t5)", use: "entity search, blocking candidates" },
];

interface Guardrail {
  readonly ok: boolean;
  readonly text: string;
  readonly meta: string;
}

const GUARDRAILS: readonly Guardrail[] = [
  { ok: true, text: "Agents may read the graph, never write it", meta: "6 write attempts denied this week" },
  { ok: true, text: "Every claim must carry a fact id", meta: "uncited sentences are dropped before display" },
  { ok: true, text: "Supplier emails require human approval", meta: "412 drafted, 388 sent after review" },
  { ok: false, text: "Ontology suggestions below 0.90 stay in review", meta: "22 pending" },
];

const MCP_TOOLS = [
  "search_graph", "get_entity", "get_neighbors", "find_path",
  "query_sparql", "query_cypher", "get_evidence", "get_lineage", "get_history",
];

export default function AgentsRoute() {
  return (
    <div className="overflow-y-auto p-8">
      <div className="mb-5 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[22.5px] font-semibold text-gowl-t1">{strings.agentsTitle}</h1>
          <p className="text-[14px] text-gowl-t5">{strings.agentsDescription}</p>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            className="rounded-md border border-gowl-line-2 px-4 py-1.5 text-[13.5px] text-gowl-t2"
          >
            {strings.agentsPolicies}
          </button>
          <button
            type="button"
            className="rounded-md bg-gowl-accent px-4 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on"
          >
            {strings.agentsNewGrant}
          </button>
        </div>
      </div>

      {/* KPI Stats */}
      <div className="mb-6 grid grid-cols-5 gap-px overflow-hidden rounded-lg border border-gowl-line bg-gowl-line">
        {KPI_STATS.map((stat) => (
          <div key={stat.label} className="bg-gowl-panel p-4">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">{stat.label}</div>
            <div className={`font-mono text-[21.5px] ${stat.color ?? "text-gowl-t1"}`}>{stat.value}</div>
            <div className="mt-1 text-[12.5px] text-gowl-t7">{stat.sub}</div>
          </div>
        ))}
      </div>

      {/* 2-column layout */}
      <div className="grid grid-cols-[1fr_400px] gap-6">
        {/* Left column */}
        <div className="space-y-6">
          {/* Pipeline Visualization */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsPipelineTitle}
            </div>
            <p className="mb-4 text-[13px] text-gowl-t5">{strings.agentsPipelineSubtitle}</p>
            <div className="flex items-center gap-2 overflow-x-auto">
              {PIPELINE_STAGES.map((stage, i) => (
                <div key={stage.tag} className="flex items-center gap-2">
                  <div className={`flex-none rounded-lg border ${stage.borderColor} ${stage.bgColor} p-3`}>
                    <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{stage.tag}</div>
                    <div className="text-[13.5px] font-semibold text-gowl-t1">{stage.name}</div>
                    <div className="mt-0.5 text-[11.5px] text-gowl-t5">{stage.detail}</div>
                  </div>
                  {i < PIPELINE_STAGES.length - 1 && (
                    <span className="flex-none font-mono text-[15.5px] text-gowl-t6">→</span>
                  )}
                </div>
              ))}
            </div>
            <p className="mt-3 text-[12px] leading-relaxed text-gowl-t5">
              {strings.agentsPipelineNote}
            </p>
          </div>

          {/* Agents Table */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-2 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsRosterTitle}
            </div>
            <p className="mb-4 text-[12.5px] text-gowl-t5">{strings.agentsRosterSubtitle}</p>
            <div className="overflow-hidden rounded-md border border-gowl-line-2">
              <div className="grid grid-cols-[1.5fr_150px_118px_92px_150px] gap-2 border-b border-gowl-line bg-gowl-panel-2 px-3 py-2 font-mono text-[10px] tracking-wider text-gowl-t6">
                <span>{strings.agentsColAgent}</span>
                <span>{strings.agentsColTrigger}</span>
                <span>{strings.agentsColRuns}</span>
                <span>{strings.agentsColGrounding}</span>
                <span>{strings.agentsColControl}</span>
              </div>
              {AGENT_ROWS.map((agent) => {
                const ms = MODE_STYLES[agent.mode];
                return (
                  <div
                    key={agent.name}
                    className="grid grid-cols-[1.5fr_150px_118px_92px_150px] items-center gap-2 border-b border-gowl-row px-3 py-2.5 last:border-b-0"
                  >
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="text-[14px] text-gowl-t1">{agent.name}</span>
                        <span className={`rounded-full px-1.5 py-0.5 font-mono text-[9.5px] ${ms.bg} ${ms.text}`}>
                          {agent.mode}
                        </span>
                      </div>
                      <div className="mt-0.5 text-[12px] text-gowl-t5">{agent.desc}</div>
                    </div>
                    <div>
                      <div className="text-[13px] text-gowl-t2">{agent.trigger}</div>
                      <div className="font-mono text-[11.5px] text-gowl-t5">{agent.next}</div>
                    </div>
                    <div>
                      <div className="font-mono text-[13.5px] text-gowl-t1">{agent.runs}</div>
                      <div className="font-mono text-[11.5px] text-gowl-t5">{agent.tokens} tokens</div>
                    </div>
                    <div className={`font-mono text-[13.5px] ${parseFloat(agent.grounding) > 95 ? "text-gowl-ok" : "text-gowl-amber"}`}>
                      {agent.grounding}
                    </div>
                    <div className="flex items-center gap-3">
                      <div
                        className={`flex h-[18px] w-[32px] cursor-pointer items-center rounded-full border ${
                          agent.on
                            ? "border-gowl-accent-border bg-gowl-accent-bg"
                            : "border-gowl-line-3 bg-gowl-track"
                        }`}
                      >
                        <div
                          className={`ml-0.5 h-3.5 w-3.5 rounded-full ${
                            agent.on ? "bg-gowl-accent" : "bg-gowl-t7"
                          }`}
                          style={{ marginLeft: agent.on ? "auto" : "2px", marginRight: agent.on ? "2px" : "auto" }}
                        />
                      </div>
                      <button type="button" className="text-[12.5px] text-gowl-accent">
                        Run now
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Run Trace */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsTraceTitle}
            </div>
            <div className="mb-3 font-mono text-[11.5px] text-gowl-t5">
              {strings.agentsTraceMeta}
            </div>
            <p className="mb-4 text-[14px] text-gowl-t2">
              {strings.agentsTraceQuestion}
            </p>
            <div className="space-y-2">
              {TRACE_STEPS.map((step, i) => (
                <div key={i} className="flex items-center gap-3 rounded-md border border-gowl-line-2 bg-gowl-panel-2 px-3 py-2">
                  <span className="flex-none font-mono text-[11.5px] text-gowl-t6">{i + 1}</span>
                  <span className="flex-none font-mono text-[12.5px] text-gowl-accent">{step.tool}</span>
                  <span className="flex-1 truncate font-mono text-[12px] text-gowl-t4">{step.args}</span>
                  <span className="flex-none font-mono text-[11.5px] text-gowl-t5">{step.tokens} tok</span>
                  <span className="flex-none font-mono text-[11.5px] text-gowl-t5">{step.latency}</span>
                </div>
              ))}
            </div>
            <div className="mt-4 rounded-md border border-gowl-accent-border bg-gowl-accent-deep p-3">
              <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-accent">
                {strings.agentsAnswerTitle}
              </div>
              <p className="text-[13.5px] leading-relaxed text-gowl-t2">
                {strings.agentsAnswerBody}
              </p>
              <div className="mt-2 text-[12px] text-gowl-t5">{strings.agentsAnswerCite}</div>
            </div>
          </div>
        </div>

        {/* Right column */}
        <div className="space-y-6">
          {/* Model Usage */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsModelTitle}
            </div>
            <div className="space-y-3">
              {MODEL_USAGE.map((model) => (
                <div key={model.name}>
                  <div className="mb-1 flex items-baseline justify-between">
                    <span className="text-[13.5px] text-gowl-t2">{model.name}</span>
                    <span className="font-mono text-[12.5px] text-gowl-t5">{model.tokens}</span>
                  </div>
                  <div className="h-2 rounded-full bg-gowl-track">
                    <div
                      className="h-2 rounded-full"
                      style={{ width: model.width, backgroundColor: model.color }}
                    />
                  </div>
                  <div className="mt-0.5 text-[11.5px] text-gowl-t6">{model.use}</div>
                </div>
              ))}
            </div>
            <div className="mt-4 border-t border-gowl-line pt-3">
              <div className="flex justify-between text-[13.5px]">
                <span className="text-gowl-t2">{strings.agentsSpendLabel}</span>
                <span className="font-mono text-gowl-t1">$1,284</span>
              </div>
              <div className="mt-1 flex justify-between text-[13.5px]">
                <span className="text-gowl-t2">{strings.agentsBudgetLabel}</span>
                <span className="font-mono text-gowl-amber">64% of $2,000</span>
              </div>
            </div>
          </div>

          {/* Guardrails */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsGuardrailsTitle}
            </div>
            <div className="space-y-3">
              {GUARDRAILS.map((item, i) => (
                <div key={i} className="flex items-start gap-2.5">
                  <span className={`mt-0.5 flex-none text-[15.5px] ${item.ok ? "text-gowl-ok" : "text-gowl-amber"}`}>
                    {item.ok ? "✓" : "!"}
                  </span>
                  <div>
                    <div className="text-[13.5px] text-gowl-t2">{item.text}</div>
                    <div className="mt-0.5 text-[12px] text-gowl-t5">{item.meta}</div>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* MCP Tools Cloud */}
          <div className="rounded-lg border border-gowl-line bg-gowl-panel p-5">
            <div className="mb-4 font-mono text-[11px] tracking-widest text-gowl-t6">
              {strings.agentsMcpTitle}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {MCP_TOOLS.map((tool) => (
                <span
                  key={tool}
                  className="rounded border border-gowl-line-2 bg-gowl-input px-2 py-1 font-mono text-[11.5px] text-gowl-t4"
                >
                  {tool}
                </span>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
