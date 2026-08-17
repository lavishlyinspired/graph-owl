/** Pure rollup logic for the Sources screen (Plan 122a A6) — there is no
 *  `Source` entity in the real API, only `ConnectorRun` history
 *  (`GET /connectors/runs`), so this reduces run history into per-service
 *  rows the same way `trace.ts` reduces a `LineageGraph` into a
 *  `TraceConfig`: pure, unit-tested, separate from rendering. */

import type { ConnectorRun } from "./api";

export type SourceHealth = "healthy" | "stale" | "degraded";

export interface SourceRollup {
  readonly serviceName: string;
  readonly connector: string;
  readonly objects: number;
  readonly lastSyncAt: string;
  readonly health: SourceHealth;
  readonly runCount: number;
}

/** The mockup's own stated definition ("STALE ... no sync in 7 days") — not
 *  a number invented here, the threshold the screen itself already names. */
const STALE_AFTER_MS = 7 * 24 * 60 * 60 * 1000;

export function rollupSources(runs: readonly ConnectorRun[], now: Date): readonly SourceRollup[] {
  const byService = new Map<string, ConnectorRun[]>();
  for (const run of runs) {
    const list = byService.get(run.serviceName) ?? [];
    list.push(run);
    byService.set(run.serviceName, list);
  }

  const rollups = Array.from(byService.entries()).map(([serviceName, serviceRuns]): SourceRollup => {
    const byRecency = [...serviceRuns].sort(
      (a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime(),
    );
    const latest = byRecency[0]!;
    const objects = Math.max(
      0,
      serviceRuns.reduce((sum, run) => sum + run.created - run.deleted, 0),
    );
    const isStale = now.getTime() - new Date(latest.startedAt).getTime() > STALE_AFTER_MS;

    return {
      serviceName,
      connector: latest.connector,
      objects,
      lastSyncAt: latest.startedAt,
      health: latest.failed > 0 ? "degraded" : isStale ? "stale" : "healthy",
      runCount: serviceRuns.length,
    };
  });

  return rollups.sort((a, b) => new Date(b.lastSyncAt).getTime() - new Date(a.lastSyncAt).getTime());
}
