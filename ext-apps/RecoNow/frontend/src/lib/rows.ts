import type { Bucket, ReconRow } from "./api";

/** Which invoices the table shows, given the two narrowings a reviewer can apply.
 *
 * A bucket answers "what state is this invoice in"; a rule label answers "which
 * invoices is this check talking about". They are independent questions, so both
 * apply at once and an invoice must satisfy each one asked.
 */
export const visibleRows = (
  rows: readonly ReconRow[],
  bucket: Bucket | null,
  ruleLabel: string | null,
): readonly ReconRow[] =>
  rows
    .filter((row) => ruleLabel === null || row.labels.includes(ruleLabel))
    .filter((row) => bucket === null || row.bucket === bucket);
