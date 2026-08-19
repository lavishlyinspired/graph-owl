import { useEffect, useState } from "react";
import {
  fetchAgentReport,
  fetchAgentRun,
  fetchAgentRuns,
  type AgentRun,
  type AgentRunDetail,
} from "../lib/api";

/** What the agents actually did — Plan 123 §5's "seeing what is running".
 *
 *  **The screen that did not exist.** There was no history of what an agent
 *  did, no way to see what was running or had completed, and nothing to report
 *  on. The runtime recorded that an agent *would have* run.
 *
 *  The shape follows current agent-observability practice: every step is a
 *  typed span, and what is shown is tool input/output and decisions — not only
 *  the model calls. A trace of model calls alone cannot answer the question
 *  anyone actually asks after a bad outcome, which is *what did it look at
 *  before it decided that*. */
const KIND_STYLE: Record<string, string> = {
  tool: "bg-sky-100 text-sky-800",
  // Amber wherever a model was involved, matching GeneratedBadge — one
  // concept, one colour, across every screen.
  model: "bg-amber-100 text-amber-800",
  decision: "bg-amber-100 text-amber-800",
  refusal: "bg-red-100 text-red-700",
};

const STATUS_STYLE: Record<string, string> = {
  running: "bg-sky-100 text-sky-800",
  completed: "bg-emerald-100 text-emerald-800",
  failed: "bg-red-100 text-red-700",
  skipped: "bg-reco-line text-reco-t4",
};

export default function AgentsRoute() {
  const [runs, setRuns] = useState<readonly AgentRun[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [scope, setScope] = useState("");
  const [open, setOpen] = useState<AgentRunDetail | null>(null);
  const [report, setReport] = useState<string | null>(null);
  // Populated from whichever run is open — a list row carries span counts, not
  // span names, so the tool identities only exist once a run is expanded.
  const [mcpNames, setMcpNames] = useState<string[]>([]);

  const refresh = () =>
    fetchAgentRuns()
      .then((d) => {
        setRuns(d.runs);
        setCounts(d.counts);
        setScope(d.scope);
      })
      .catch(() => setRuns([]));

  useEffect(() => {
    refresh();
    // Polled rather than pushed: a run finishing is not worth a socket, and a
    // list that never updates is the thing this screen exists to replace.
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  }, []);

  // Aggregated across every run held. `tokens` stays null rather than 0 when
  // nothing was measured — a zero claims the fleet was free.
  const totals = runs.reduce(
    (acc, run) => {
      const counts = run.span_counts ?? {};
      acc.model += counts.model ?? 0;
      acc.refusals += counts.refusal ?? 0;
      if (run.tokens !== null) acc.tokens = (acc.tokens ?? 0) + run.tokens;
      return acc;
    },
    { mcp: 0, model: 0, refusals: 0, tokens: null as number | null },
  );
  // MCP calls are tool spans whose name carries the `mcp:` prefix, so the
  // count is of graph calls specifically rather than of every tool step.
  const mcpTools = [...new Set(mcpNames)];
  totals.mcp = mcpNames.length;

  const openRun = (id: string) => {
    setReport(null);
    fetchAgentRun(id)
      .then((detail) => {
        setOpen(detail);
        setMcpNames(
          detail.spans
            .filter((span) => span.name.startsWith("mcp:"))
            .map((span) => span.name.slice(4)),
        );
      })
      .catch(() => setOpen(null));
  };

  return (
    <div className="space-y-6 p-6">
      <header>
        <h1 className="text-[19px] font-medium text-reco-t1">Agent activity</h1>
        <p className="mt-1 text-[13px] text-reco-t4">
          Every run, what it looked at, what it decided, and what it was refused.
        </p>
      </header>

      {/* **What the fleet is actually made of.** The run list showed status
          counts and step counts, which told a reader an agent ran but nothing
          about what it reached for. These three are the questions asked after
          an incident: did it call the graph, did it call a model, and was it
          ever refused. */}
      <div className="grid gap-3 sm:grid-cols-3">
        <Tile
          label="MCP tool calls"
          value={totals.mcp}
          hint={mcpTools.length ? mcpTools.join(" · ") : "no graph calls yet"}
        />
        <Tile
          label="Model calls"
          value={totals.model}
          hint={
            totals.tokens === null
              ? "usage not measured"
              : `${totals.tokens.toLocaleString()} tokens`
          }
        />
        <Tile
          label="Refused writes"
          value={totals.refusals}
          hint="a grant not held at the moment of the write"
        />
      </div>

      <div className="flex gap-2 text-[11px]">
        {Object.entries(counts).map(([status, n]) => (
          <span
            key={status}
            className={`rounded px-2 py-1 font-mono ${STATUS_STYLE[status] ?? "bg-reco-line"}`}
          >
            {n} {status}
          </span>
        ))}
      </div>

      {runs.length === 0 && (
        <p className="text-[12.5px] text-reco-t4">
          No agent has run yet. They wake on events — reconcile a period to see one.
        </p>
      )}

      <div className="overflow-hidden rounded border border-reco-line">
        {runs.map((run) => (
          <button
            key={run.id}
            type="button"
            onClick={() => openRun(run.id)}
            className="grid w-full grid-cols-[110px_1fr_120px_90px_1fr] items-center gap-3 border-b border-reco-line-2 px-4 py-2.5 text-left text-[12.5px] hover:bg-reco-panel-2"
          >
            <span className="font-mono text-reco-t1">{run.agent}</span>
            <span className="font-mono text-[10.5px] text-reco-t5">{run.event}</span>
            <span
              className={`justify-self-start rounded px-1.5 py-0.5 font-mono text-[9px] uppercase ${
                STATUS_STYLE[run.status] ?? ""
              }`}
            >
              {run.status}
            </span>
            <span className="text-right font-mono text-[11px] text-reco-t4">
              {run.ms === null ? "—" : `${run.ms} ms`}
            </span>
            <span className="text-[11px] text-reco-t5">
              {Object.entries(run.span_counts ?? {})
                .map(([k, n]) => `${n} ${k}`)
                .join(" · ") || "no steps"}
            </span>
          </button>
        ))}
      </div>

      <p className="text-[11px] text-reco-t5">{scope}</p>

      {open && (
        <section className="rounded border border-reco-line p-4">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-[14px] text-reco-t1">
              {open.agent} · <span className="font-mono text-[11px] text-reco-t5">{open.id}</span>
            </h2>
            <button
              type="button"
              onClick={() => fetchAgentReport(open.id).then((r) => setReport(r.report))}
              className="rounded border border-reco-line px-3 py-1 text-[11.5px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
            >
              Generate report
            </button>
          </div>

          <ol className="space-y-1">
            {open.spans.map((span, index) => (
              <li key={index} className="flex items-baseline gap-2 text-[12px]">
                <span className="w-5 text-right font-mono text-[10px] text-reco-t5">
                  {index + 1}
                </span>
                <span
                  className={`rounded px-1.5 py-0.5 font-mono text-[9px] uppercase ${
                    KIND_STYLE[span.kind] ?? ""
                  }`}
                >
                  {span.kind}
                </span>
                <span className="font-mono text-reco-t2">{span.name}</span>
                <span className="font-mono text-[10px] text-reco-t5">{span.ms} ms</span>
                {span.because && <span className="text-reco-t4">— {span.because}</span>}
                {span.error && <span className="text-reco-bad">— {span.error}</span>}
              </li>
            ))}
          </ol>

          {report && (
            <pre className="mt-4 overflow-x-auto whitespace-pre-wrap rounded bg-reco-panel-2 p-3 text-[11.5px] leading-relaxed text-reco-t2">
              {report}
            </pre>
          )}
        </section>
      )}
    </div>
  );
}


function Tile({
  label,
  value,
  hint,
}: {
  readonly label: string;
  readonly value: number;
  readonly hint: string;
}) {
  return (
    <div className="rounded border border-reco-line bg-white p-3">
      <div className="font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">{label}</div>
      <div className="font-mono text-[20px] text-reco-t1">{value}</div>
      <div className="mt-0.5 truncate text-[10.5px] text-reco-t5" title={hint}>
        {hint}
      </div>
    </div>
  );
}
