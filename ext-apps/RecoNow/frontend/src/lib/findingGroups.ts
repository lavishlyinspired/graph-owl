import type { RegisterRow } from "./api";

/** What kind of problem a finding is — and therefore **who has to act**.
 *
 *  The split is by remedy, not by rule name. A flat exception list makes a
 *  reviewer decide, for every row, whether it is their problem, the supplier's,
 *  or the law's — twelve times a period, from a rule label.
 *
 *  Authored in `packs/gst`, never here: a healthcare or banking pack names
 *  entirely different findings, and this file only knows the three shapes a
 *  remedy can take. */
export const CATEGORY_META = [
  {
    category: "compliance",
    label: "Compliance",
    // First, because a reviewer can least afford to miss it: the credit is
    // lost or must be reversed on a return they are about to file.
    blurb: "The law limits this credit. Adjust your own return.",
  },
  {
    category: "data",
    label: "Data",
    blurb: "Your records and the portal's disagree. Establish which is right.",
  },
  {
    category: "follow-up",
    label: "Follow-up",
    // Last, because it is recoverable and depends on somebody else.
    blurb: "Somebody else must act before this can resolve.",
  },
  {
    category: "uncategorised",
    label: "Uncategorised",
    blurb: "No category authored for this rule yet.",
  },
] as const;

export interface RuleGroup {
  readonly reason_code: string;
  readonly title: string;
  readonly rows: readonly RegisterRow[];
  readonly exposure: number;
}

export interface FindingGroup {
  readonly category: string;
  readonly label: string;
  readonly blurb: string;
  readonly rules: readonly RuleGroup[];
  readonly count: number;
  readonly exposure: number;
}

export function groupFindings(rows: readonly RegisterRow[]): readonly FindingGroup[] {
  const byCategory = new Map<string, Map<string, RegisterRow[]>>();

  for (const row of rows) {
    // A rule with no authored category still appears. Silently omitting a
    // finding is the one outcome this screen cannot have.
    const category = row.category ?? "uncategorised";
    const code = row.reason_code ?? "unknown";
    const rules = byCategory.get(category) ?? new Map<string, RegisterRow[]>();
    rules.set(code, [...(rules.get(code) ?? []), row]);
    byCategory.set(category, rules);
  }

  return CATEGORY_META.flatMap((meta) => {
    const rules = byCategory.get(meta.category);
    // An empty "Compliance" heading reads as a claim that the law was checked
    // and found nothing — a different statement from "no rule in that category
    // fired".
    if (!rules) return [];

    const grouped: RuleGroup[] = [...rules.entries()]
      .map(([reason_code, ruleRows]) => ({
        reason_code,
        title: ruleRows[0]?.title ?? reason_code,
        rows: ruleRows,
        exposure: ruleRows.reduce((sum, r) => sum + (r.exposure ?? 0), 0),
      }))
      // Costliest first inside a category: "9 compliance issues" is a number,
      // "2 blocked credit, 1 unpaid 180 days" is a list of things to do.
      .sort((a, b) => b.exposure - a.exposure);

    return [
      {
        category: meta.category,
        label: meta.label,
        blurb: meta.blurb,
        rules: grouped,
        count: grouped.reduce((n, r) => n + r.rows.length, 0),
        exposure: grouped.reduce((sum, r) => sum + r.exposure, 0),
      },
    ];
  });
}
