/** Indian-grouped rupees. `toLocaleString()` with no locale renders
 *  1,234,567 in the western grouping, which is wrong for a GST product. */
export function formatRupees(amount: number | null | undefined): string {
  if (amount === null || amount === undefined) return "—";
  return `₹${Math.round(amount).toLocaleString("en-IN")}`;
}
