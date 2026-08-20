import { reasoningSteps } from "../lib/exploreSeeds";
import type { Finding } from "../lib/api";
import { strings } from "../lib/strings";

/** **The mock's "why GraphOWL believes this" and "evidence", built from what
 *  the engine actually recorded.**
 *
 *  The design shows a confidence score, a four-step derivation and a set of
 *  supporting/contradicting documents. `GraphEdge` carries a relationship,
 *  two endpoints and `derived`; `/reasoning/explain` answers only for derived
 *  facts, and this data has none. What the engine *does* record against a
 *  subject is the rules that fired on it and the exact bindings that
 *  satisfied them — the same question, answered from data that exists rather
 *  than from a number nobody computed.
 *
 *  Shared between Explore's node/edge detail panel and Entity's overview for
 *  a graph-only subject — both are answering the identical question ("why is
 *  this subject flagged, and on what") for identical data, and a subject
 *  opened from either screen must not read as though it has two different
 *  explanations depending on which one it was reached from. */
export function SubjectFindings({
  findings,
  semanticType,
}: {
  readonly findings: readonly Finding[];
  readonly semanticType: string | undefined;
}) {
  if (findings.length === 0) {
    return (
      <div className="mb-4 text-[15px] text-gowl-t6">{strings.exploreNoFindingsForSubject}</div>
    );
  }

  return (
    <>
      {findings.map((finding) => {
        const steps = reasoningSteps(finding, semanticType);
        return (
          <div key={finding.id} className="mb-5">
            <div className="mb-2.5 flex items-center gap-2">
              <span className="rounded border border-gowl-amber-border bg-gowl-amber-bg px-1.5 py-0.5 font-mono text-[13.5px] text-gowl-amber">
                {finding.label}
              </span>
              <span className="font-mono text-[13.5px] text-gowl-t6">{finding.status}</span>
            </div>

            <div className="mb-2.5 font-mono text-[13.5px] tracking-[0.14em] text-gowl-t6">
              {strings.exploreReasoningTitle}
            </div>
            <div className="mb-4 flex flex-col">
              {steps.map((step, i) => (
                <div key={`${step.text}-${i}`} className="flex gap-3 pb-3">
                  <div className="flex flex-none flex-col items-center">
                    <div className="flex h-[18px] w-[18px] items-center justify-center rounded-full border border-gowl-line-2 bg-gowl-panel-2 font-mono text-[13.5px] text-gowl-t5">
                      {i + 1}
                    </div>
                    {i < steps.length - 1 && (
                      <div className="mt-1 w-px flex-1 bg-gowl-line-2" />
                    )}
                  </div>
                  <div className="pt-px">
                    <div className="text-[16px] leading-[1.45] text-gowl-t2">{step.text}</div>
                    <div className="mt-1 font-mono text-[14px] text-gowl-t7">{step.source}</div>
                  </div>
                </div>
              ))}
            </div>

            <div className="overflow-hidden rounded-md border border-gowl-line-2">
              <div className="border-b border-gowl-line-2 bg-gowl-panel-2 px-3 py-2 font-mono text-[13.5px] tracking-[0.14em] text-gowl-t5">
                {`${strings.exploreEvidenceTitle} · ${finding.evidence.length}`}
              </div>
              {finding.evidence.map((item, i) => (
                <div
                  key={`${item.predicate}-${i}`}
                  className="flex flex-col gap-1 border-b border-gowl-row px-3 py-2.5 last:border-b-0"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-mono text-[15px] text-gowl-t2">{item.predicate}</span>
                    <span className="font-mono text-[14px] text-gowl-ok">{item.value}</span>
                  </div>
                  {item.var && (
                    <div className="font-mono text-[14px] text-gowl-t7">{`bound to ?${item.var}`}</div>
                  )}
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </>
  );
}
