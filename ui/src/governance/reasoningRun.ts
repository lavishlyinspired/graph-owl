/** What a reasoning run tells a reader — Epic 97/41 Slice G.
 *
 *  "Overlay staleness is shown... a derived fact whose age is invisible is a
 *  derived fact nobody can weigh." `maintainedTo` carries no wall-clock
 *  companion (the same reason `PartitionHealthPanel` shows a raw transaction
 *  time rather than a fabricated age), so this reports it as what it is — a
 *  transaction watermark, not an "N minutes ago" this project cannot compute
 *  honestly.
 */

export interface ReasoningRunInput {
  readonly technique: "full" | "incremental";
  readonly derived: number;
  readonly replaced: number;
  readonly maintainedTo: number;
  readonly partial: boolean;
  readonly ignoredAxioms: readonly { readonly subject: string; readonly reason: string }[];
}

export interface ReasoningRunDescription {
  readonly headline: string;
  /** `null` when nothing was ignored — silence is the signal that there is
   *  nothing to explain, matching `SparqlOutcome.qlRewrite`'s own
   *  `None`-vs-empty convention rather than an empty string a caller would
   *  have to check for separately. */
  readonly warning: string | null;
}

/** Which strategy ran, what it did, and how current its overlay is —
 *  everything Epic 97's own acceptance criterion asks a reader be able to
 *  weigh, composed into the one line and one optional warning a console
 *  actually renders.
 */
export function describeReasoningRun(run: ReasoningRunInput): ReasoningRunDescription {
  const technique = run.technique === "incremental" ? "Incremental" : "Full";
  const headline = `${technique} run — ${run.derived} derived, ${run.replaced} replaced, maintained to t=${run.maintainedTo}`;
  const singular = run.ignoredAxioms.length === 1;
  const warning = run.partial
    ? `Partial: ${run.ignoredAxioms.length} axiom${singular ? "" : "s"} outside this profile ${singular ? "was" : "were"} ignored to produce this result.`
    : null;
  return { headline, warning };
}
