import { useState } from "react";
import { fetchCaseExplanation, type CaseExplanation } from "../lib/api";

/** "Why did this fire, for this invoice?" — answered from the real row.
 *
 *  **On demand, not on hover.** The model round trip is several seconds; a
 *  hover that hangs is worse than a button that says what it is about to do.
 *  The pack's guidance and the computed narrative are already on screen
 *  instantly — this is the deeper answer, asked for deliberately.
 *
 *  The reply always says which produced it. A model explanation and a computed
 *  fallback carry different weight, and a reader deciding how far to trust a
 *  sentence needs to know which they are reading — especially when the
 *  fallback happened *because* the model tried to state a figure the data does
 *  not carry. */
export function ExplainCase({
  clientId,
  periodId,
  caseId,
}: {
  readonly clientId: string;
  readonly periodId: string;
  readonly caseId: string;
}) {
  const [state, setState] = useState<"idle" | "asking" | "done" | "failed">("idle");
  const [explanation, setExplanation] = useState<CaseExplanation | null>(null);

  const ask = () => {
    setState("asking");
    fetchCaseExplanation(clientId, periodId, caseId)
      .then((result) => {
        setExplanation(result);
        setState("done");
      })
      .catch(() => setState("failed"));
  };

  if (state === "idle") {
    return (
      <button
        type="button"
        onClick={ask}
        className="rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
      >
        Explain this from the data
      </button>
    );
  }

  if (state === "asking") {
    return (
      <p className="text-[12px] text-reco-t4">
        Reading both sides of this invoice…
      </p>
    );
  }

  if (state === "failed" || !explanation) {
    return (
      <p className="text-[12px] text-reco-bad">
        Could not produce an explanation. The computed summary above still applies.
      </p>
    );
  }

  return (
    <div className="rounded border border-reco-line bg-reco-panel-2 p-4">
      <div className="mb-2 flex items-center gap-2">
        <span className="font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
          Why this fired
        </span>
        <span
          className={`rounded px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider ${
            explanation.source === "model"
              ? "bg-reco-accent/10 text-reco-accent"
              : "bg-reco-line text-reco-t4"
          }`}
          title={
            explanation.source === "model"
              ? "Written by an inference model from your own data. Every figure in it was checked against your row."
              : "Computed directly from your data — no model was involved."
          }
        >
          {explanation.source === "model" ? "model" : "computed"}
        </span>
      </div>

      <p className="text-[12.5px] leading-relaxed text-reco-t2">{explanation.text}</p>

      {explanation.note && (
        <p className="mt-2 text-[11.5px] leading-relaxed text-reco-t4">{explanation.note}</p>
      )}

      {/* Shown rather than hidden: an agent that tried to state an unsupported
          figure is exactly what a reviewer of an AI feature wants to see. */}
      {explanation.refusal && (
        <p className="mt-2 text-[11px] leading-relaxed text-reco-t5">
          Refused: {explanation.refusal}
        </p>
      )}
    </div>
  );
}
