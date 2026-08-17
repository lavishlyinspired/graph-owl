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
    if (chunk.bytesGzip > ROUTE_CHUNK_GZIP_BYTES) {
      violations.push({
        budget: "route-chunk",
        detail: `route chunk "${chunk.name}" is ${(chunk.bytesGzip / 1024).toFixed(1)}KB gzipped, budget is ${ROUTE_CHUNK_GZIP_BYTES / 1024}KB`,
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
