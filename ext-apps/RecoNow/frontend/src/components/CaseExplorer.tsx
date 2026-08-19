import { useState } from "react";
import { fetchCaseGraph, type CaseGraph } from "../lib/api";
import { GraphExplorer } from "./GraphExplorer";

/** "What else in the graph is this invoice connected to?" — on demand, next
 *  to `ExplainCase`'s "why did this fire": one answers from the row's own
 *  fields, this one walks outward from it. Same on-demand shape as
 *  `ExplainCase` for the same reason — a graph walk is a real request, not
 *  something to fire on hover. */
export function CaseExplorer({
  clientId,
  periodId,
  caseId,
}: {
  readonly clientId: string;
  readonly periodId: string;
  readonly caseId: string;
}) {
  const [state, setState] = useState<"idle" | "loading" | "done" | "failed">("idle");
  const [graph, setGraph] = useState<CaseGraph | null>(null);

  const explore = () => {
    setState("loading");
    fetchCaseGraph(clientId, periodId, caseId)
      .then((result) => {
        setGraph(result);
        setState("done");
      })
      .catch(() => setState("failed"));
  };

  if (state === "idle") {
    return (
      <button
        type="button"
        onClick={explore}
        className="rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
      >
        Explore in the graph
      </button>
    );
  }

  if (state === "loading") {
    return <p className="text-[12px] text-reco-t4">Walking outward from this invoice…</p>;
  }

  if (state === "failed" || !graph) {
    return <p className="text-[12px] text-reco-bad">Could not load the graph neighbourhood.</p>;
  }

  return <GraphExplorer graph={graph} />;
}
