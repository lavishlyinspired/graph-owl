/** Pure formatting helpers, kept out of components so they are unit
 *  testable without rendering anything. */

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** "12 min ago" / "3 h ago" / "5 d ago" — the console's own activity-feed
 *  convention, matched to the delivered mockup's copy exactly ("12 min
 *  ago · 6 new classes"). `now` is a parameter, not `Date.now()` read
 *  inside, so the boundary between each unit is a plain assertion rather
 *  than a clock-dependent flake. */
export function relativeTime(isoTimestamp: string, now: Date): string {
  const then = new Date(isoTimestamp).getTime();
  const diffMs = now.getTime() - then;
  if (diffMs < MINUTE_MS) return "just now";
  if (diffMs < HOUR_MS) return `${Math.floor(diffMs / MINUTE_MS)} min ago`;
  if (diffMs < DAY_MS) return `${Math.floor(diffMs / HOUR_MS)} h ago`;
  return `${Math.floor(diffMs / DAY_MS)} d ago`;
}

/** "1,843,220" — every stat tile in the mockup uses grouped digits. */
export function formatCount(n: number): string {
  return n.toLocaleString("en-US");
}

/** "91%" — rounds rather than truncates, so 99.6% does not read as 99%
 *  when it is one asset away from 100%. */
export function formatPct(pct: number): string {
  return `${Math.round(pct)}%`;
}
