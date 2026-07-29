/** The violations queue, as a reader works it — Epic 41.
 *
 *  `00f-ui-architecture.md` requires the part that can be wrong in a way
 *  somebody would act on to be a pure function with its own tests. For a
 *  governance queue that is: which findings belong together, how far behind the
 *  report is, and what a suggestion actually tells a steward to do.
 *
 *  What Ant Design paints from that is a drawing decision.
 */

export type Severity = "violation" | "warning" | "info";

export interface Suggestion {
  readonly action: "assertMissing" | "retractExcess" | "retypeValue";
  readonly path: string;
  readonly hint?: string;
  readonly keep?: number;
  readonly to?: string;
}

export interface Finding {
  readonly id: string;
  readonly shape: string;
  readonly focusNode: string;
  readonly path: string | null;
  readonly constraint: string;
  readonly severity: Severity;
  readonly message: string;
  readonly actual: string | null;
  readonly suggestion: Suggestion | null;
}

/** Worst first. A queue is worked from the top, so this is the order. */
export const SEVERITY_ORDER: readonly Severity[] = ["violation", "warning", "info"];

function rank(severity: Severity): number {
  const index = SEVERITY_ORDER.indexOf(severity);
  // An unrecognised severity sorts last rather than first. A future severity
  // this build does not know about must not push real violations down the page.
  return index === -1 ? SEVERITY_ORDER.length : index;
}

export interface AssetGroup {
  readonly focusNode: string;
  readonly findings: readonly Finding[];
  /** The loudest severity in the group — what the group's badge shows. */
  readonly severity: Severity;
}

/** One row per asset, not one per finding.
 *
 *  A table with forty rows about one table reads as forty problems. It is one
 *  asset that needs attention, and a steward fixes it once. Grouping is what
 *  turns the queue's length into a count of *work* rather than a count of
 *  failed assertions.
 *
 *  Groups are ordered by their loudest finding, then by name — stable, because
 *  a queue that reshuffles between polls cannot be worked from the top.
 */
export function groupByAsset(findings: readonly Finding[]): AssetGroup[] {
  const byNode = new Map<string, Finding[]>();
  for (const finding of findings) {
    const existing = byNode.get(finding.focusNode);
    if (existing) existing.push(finding);
    else byNode.set(finding.focusNode, [finding]);
  }

  return [...byNode.entries()]
    .map(([focusNode, group]) => {
      // Within a group too: the reason to open an asset is its worst finding.
      const findings = [...group].sort(
        (a, b) => rank(a.severity) - rank(b.severity) || a.constraint.localeCompare(b.constraint),
      );
      // Which makes the group's own severity the first one — computing it a
      // second way with a fold was a second chance to disagree with the sort.
      return { focusNode, findings, severity: findings[0]!.severity };
    })
    .sort((a, b) => rank(a.severity) - rank(b.severity) || a.focusNode.localeCompare(b.focusNode));
}

/** What a suggestion tells somebody to do, in a sentence.
 *
 *  `null` when there is no mechanical repair, and that is most of them. A
 *  suggestion that restates the violation — "make it match the pattern" —
 *  trains a reader to stop looking at the column.
 */
export function describeSuggestion(suggestion: Suggestion | null): string | null {
  if (!suggestion) return null;
  const path = localName(suggestion.path);
  switch (suggestion.action) {
    case "assertMissing":
      return suggestion.hint ? `Add ${path} — ${suggestion.hint}` : `Add ${path}`;
    case "retractExcess":
      return `Remove all but ${suggestion.keep} ${path}`;
    case "retypeValue":
      return `Store ${path} as ${suggestion.to}`;
    default:
      // An action this build does not know about is shown as nothing rather
      // than as a broken sentence. The finding itself still renders.
      return null;
  }
}

/** `1:payments` → `payments`.
 *
 *  The namespace code is how the graph addresses a node and is noise to a
 *  steward reading a queue. Kept out of the display, never out of the data —
 *  the id is what a fix is applied to.
 */
export function localName(sid: string): string {
  // No conditional: `indexOf` returns -1 when there is no colon, and
  // `slice(0)` is the whole string. A guard here would restate that.
  return sid.slice(sid.indexOf(":") + 1);
}

export interface Currency {
  /** Transactions the report is behind the graph. */
  readonly behind: number;
  /** Worth warning about. */
  readonly stale: boolean;
  readonly label: string;
}

/** How current the report is.
 *
 *  **The number that makes a clean queue trustworthy.** An empty queue means
 *  "nothing is wrong" only if the pass ran after the last change; otherwise it
 *  means "nothing was wrong then", and those call for opposite reactions.
 *
 *  Anything behind at all is called out. Not a threshold: the graph's `t` only
 *  moves when something was written, so being behind by one is being behind by
 *  one real change, and a tolerance would just be a number nobody could justify.
 */
export function currency(computedAtT: number, currentT: number): Currency {
  const behind = Math.max(0, currentT - computedAtT);
  if (computedAtT === 0) {
    return { behind, stale: true, label: "never run" };
  }
  if (behind === 0) {
    return { behind: 0, stale: false, label: "current" };
  }
  return {
    behind,
    stale: true,
    label: behind === 1 ? "1 change behind" : `${behind} changes behind`,
  };
}
