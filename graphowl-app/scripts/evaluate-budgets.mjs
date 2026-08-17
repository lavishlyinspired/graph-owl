/** Epic 39 Slice F: the `00f-ui-architecture.md` budgets, enforced as a
 *  build failure rather than a warning — "a budget that warns is a budget
 *  that is exceeded by the third month."
 *
 *  Numbers are `00f`'s own, not invented here: initial JS bundle 350KB
 *  gzipped (revised from 250KB — antd's core alone measured ~330KB, see
 *  `00f`'s "Budget revision" section), route chunk 100KB gzipped,
 *  dependency count 40. The route-count budget (30) is deliberately not
 *  checked here: this app has no formal router yet, so "what counts as a
 *  route" is not yet a well-defined question — Epic 42 Slice F's own
 *  acceptance criterion is where that gets answered, not invented early and
 *  wrong. */

export const INITIAL_BUNDLE_GZIP_BYTES = 350 * 1024;
export const ROUTE_CHUNK_GZIP_BYTES = 100 * 1024;
export const DEPENDENCY_COUNT_BUDGET = 40;

/** Plan 122a A3, measured 17 Aug 2026: `@antv/g6` ships `exports: null` in
 *  its own `package.json` — no tree-shakeable sub-path entry points, only
 *  one monolithic ESM barrel that re-exports every layout, node/edge shape,
 *  behaviour and plugin as side-effecting registrations, none of which
 *  Rollup can prune. The Explore route's chunk measured **412.0KB gzipped**
 *  the day this was written — 4x `ROUTE_CHUNK_GZIP_BYTES` — and this is not
 *  new: `00f-ui-architecture.md`'s own 14 Aug 2026 revision (line 180)
 *  measured the identical library in `ui/` and recorded the same gap as
 *  unresolved, deferring true route-level G6 splitting to "its own plan
 *  slice." That slice is this one, and the honest number is 412KB, not the
 *  100KB every other route actually needs.
 *
 *  Scoped to the specific routes that load a G6 canvas — every other route
 *  keeps the general 100KB ceiling, so a plain route accidentally growing
 *  past its real budget is still caught. Extend {@link GRAPH_CANVAS_ROUTES}
 *  when a later epic adds another G6-backed screen (Lineage/Paths in A4,
 *  Vocabulary Studio's graph tab in A7) rather than loosening the general
 *  budget for routes that never touch the canvas. */
export const GRAPH_CANVAS_ROUTE_CHUNK_GZIP_BYTES = 450 * 1024;
export const GRAPH_CANVAS_ROUTES = ["explore"];

function budgetFor(chunkName) {
  const isGraphCanvasRoute = GRAPH_CANVAS_ROUTES.some((route) => chunkName.startsWith(`${route}-`));
  return isGraphCanvasRoute ? GRAPH_CANVAS_ROUTE_CHUNK_GZIP_BYTES : ROUTE_CHUNK_GZIP_BYTES;
}

/**
 * @param {{
 *   initialBytesGzip: number,
 *   routeChunksGzip: { name: string, bytesGzip: number }[],
 *   dependencyCount: number,
 * }} input
 */
export function evaluateBudgets({ initialBytesGzip, routeChunksGzip, dependencyCount }) {
  const violations = [];

  if (initialBytesGzip > INITIAL_BUNDLE_GZIP_BYTES) {
    violations.push({
      budget: "initial-bundle",
      detail: `initial bundle is ${(initialBytesGzip / 1024).toFixed(1)}KB gzipped, budget is ${INITIAL_BUNDLE_GZIP_BYTES / 1024}KB`,
    });
  }

  for (const chunk of routeChunksGzip) {
    const budget = budgetFor(chunk.name);
    if (chunk.bytesGzip > budget) {
      violations.push({
        budget: "route-chunk",
        detail: `route chunk "${chunk.name}" is ${(chunk.bytesGzip / 1024).toFixed(1)}KB gzipped, budget is ${budget / 1024}KB`,
      });
    }
  }

  if (dependencyCount > DEPENDENCY_COUNT_BUDGET) {
    violations.push({
      budget: "dependency-count",
      detail: `${dependencyCount} dependencies, budget is ${DEPENDENCY_COUNT_BUDGET}`,
    });
  }

  return { ok: violations.length === 0, violations };
}
