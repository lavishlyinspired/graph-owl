/** A derivation chain, flattened for display — Epic 41 / Epic 6 Slice D.
 *
 *  The API returns a recursive explanation; a reader reads a list. This is the
 *  flattening, and it is the part that can be wrong in a way somebody would act
 *  on: a chain rendered one level deep looks complete and answers nothing,
 *  because *why the premise held* is the half a reviewer is checking.
 */

export interface Fact {
  readonly s: string;
  readonly p: string;
  readonly o: string;
  readonly t: number;
}

export type Explanation =
  | { readonly status: "asserted"; readonly fact: Fact }
  | { readonly status: "circular"; readonly fact: Fact }
  | { readonly status: "unknown" }
  | { readonly status: "derived"; readonly chains: readonly Chain[] };

export interface Chain {
  readonly rule: string;
  readonly premises: readonly Explanation[];
}

export interface Row {
  readonly depth: number;
  readonly kind: "rule" | "asserted" | "circular" | "unknown";
  /** The rule that fired, on a `rule` row. */
  readonly rule?: string;
  readonly fact?: Fact;
  /** Which route this row belongs to, when a fact follows more than one.
   *  `undefined` when there is only one, because "route 1 of 1" is noise. */
  readonly route?: number;
}

/** The chain as indented rows, depth-first.
 *
 *  **Every route, fully expanded.** A fact provable two ways has two
 *  explanations, and showing only the first hides the one a reviewer may find
 *  more convincing — or the one that reveals the rule they wanted to disable.
 *
 *  A `circular` premise is rendered as itself rather than dropped. It only
 *  arises from a cyclic ontology, and silently truncating there turns a
 *  modelling error into a chain that simply looks short.
 */
export function flatten(explanation: Explanation, depth = 0): Row[] {
  switch (explanation.status) {
    case "asserted":
      return [{ depth, kind: "asserted", fact: explanation.fact }];
    case "circular":
      return [{ depth, kind: "circular", fact: explanation.fact }];
    case "derived": {
      const many = explanation.chains.length > 1;
      return explanation.chains.flatMap((chain, index) => [
        {
          depth,
          kind: "rule" as const,
          rule: chain.rule,
          ...(many ? { route: index + 1 } : {}),
        },
        ...chain.premises.flatMap((premise) => flatten(premise, depth + 1)),
      ]);
    }
    // `unknown` lands here too, and deliberately: a fact nothing supports and
    // a status this build cannot read are the same thing to a reader — there
    // is no chain to show. A console that crashed on an unfamiliar status
    // would be worse than one that says it cannot read it.
    default:
      return [{ depth, kind: "unknown" }];
  }
}

/** How deep the chain goes.
 *
 *  Shown beside the fact, because depth is the single number that says whether
 *  an inference is a restatement or a genuine conclusion.
 */
export function depthOf(explanation: Explanation): number {
  const rows = flatten(explanation);
  return rows.reduce((deepest, row) => Math.max(deepest, row.depth), 0);
}

/** Every rule that took part, once each, in the order first encountered.
 *
 *  What a reviewer decides *about*: enabling or disabling a rule is the lever
 *  they have, and a list of the rules behind a conclusion is more actionable
 *  than the tree it came from.
 */
export function rulesUsed(explanation: Explanation): string[] {
  const seen: string[] = [];
  for (const row of flatten(explanation)) {
    if (row.rule && !seen.includes(row.rule)) seen.push(row.rule);
  }
  return seen;
}
