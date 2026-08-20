import { useEffect, useState } from "react";
import type { AssetAnalytics } from "../lib/api";
import { connectivityRows, describeAnalytics } from "../lib/graph/analytics";
import { strings } from "../lib/strings";

/** "How connected is this neighbourhood" — the console-facing surface for
 *  `graph-owl-analytics` (Epic 38's degree centrality, connected
 *  components, orphan detection), reachable at last via
 *  `fetchGraphContextAnalytics`/`fetchAssetAnalytics`. `PageRank` is
 *  deliberately absent, matching the facade: it means something only at
 *  whole-graph scope, and computing it over an arbitrary bounded
 *  neighbourhood would produce a number shaped like PageRank without
 *  meaning what PageRank means.
 *
 *  `load` is caller-supplied rather than a hardcoded fetch, so the same
 *  panel works whether the selected node has a catalog asset id or is a
 *  bare pack-vocabulary subject (a GST invoice, say) — degree centrality
 *  means the same thing either way. */
export function ConnectivityPanel({
  cacheKey,
  load,
  names,
}: {
  /** Changes whenever `load` would return something different — the
   *  effect's own dependency, since a function reference is not a stable
   *  key. */
  readonly cacheKey: string;
  readonly load: () => Promise<AssetAnalytics>;
  readonly names?: ReadonlyMap<string, string>;
}) {
  const [analytics, setAnalytics] = useState<AssetAnalytics | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    setAnalytics(null);
    setError(false);
    load()
      .then(setAnalytics)
      .catch(() => setError(true));
    // `load` is a fresh closure every render by design (it captures the
    // current seed); `cacheKey` is what actually identifies "should this
    // re-fetch".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cacheKey]);

  if (error) {
    return <p className="text-[13px] text-gowl-bad">{strings.connectivityFailed}</p>;
  }
  if (!analytics) {
    return <p className="text-[13px] text-gowl-t5">{strings.connectivityLoading}</p>;
  }

  const rows = connectivityRows(analytics, names);
  if (rows.length === 0) {
    return <p className="text-[13px] text-gowl-t5">{strings.connectivityEmpty}</p>;
  }

  return (
    <div>
      <div className="mb-1 font-mono text-[11px] tracking-widest text-gowl-t6">{strings.connectivityTitle}</div>
      <p className="mb-2 text-[12.5px] text-gowl-t5">{strings.connectivityHint}</p>
      <p className="mb-2 text-[12.5px] text-gowl-t3">{describeAnalytics(analytics)}</p>
      <div className="overflow-hidden rounded-md border border-gowl-line">
        <div className="grid grid-cols-[1fr_60px_60px] gap-2 border-b border-gowl-line bg-gowl-panel-2 px-2 py-1 font-mono text-[11px] tracking-widest text-gowl-t6">
          <span>{strings.connectivityColNode}</span>
          <span className="text-right">{strings.connectivityColIncoming}</span>
          <span className="text-right">{strings.connectivityColOutgoing}</span>
        </div>
        {rows.map((row) => (
          <div
            key={row.id}
            className="grid grid-cols-[1fr_60px_60px] gap-2 border-b border-gowl-row px-2 py-1 text-[13px] text-gowl-t2 last:border-b-0"
          >
            <span className="truncate" title={row.orphan ? strings.connectivityOrphanTag : row.label}>
              {row.orphan ? `${row.label} ⚠` : row.label}
            </span>
            <span className="text-right font-mono">{row.inDegree}</span>
            <span className="text-right font-mono">{row.outDegree}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
