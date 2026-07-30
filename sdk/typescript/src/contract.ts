/**
 * The contract this SDK was built against — Epic 16 Slice E.
 *
 * "SDK version is pinned to a contract version" is a stated criterion, and the
 * pin has to be a *value the tests can compare*, not a sentence in a README. If
 * the service's contract version moves and this constant does not, the drift
 * test below fails and somebody has to look at what changed — which is the
 * whole point: an SDK that silently keeps working against a contract it was not
 * built for is the failure mode, not the success.
 */
export const CONTRACT_VERSION = "0.1.0";

/**
 * The paths this SDK calls.
 *
 * Listed rather than derived, so a contract that renames or drops one fails
 * here instead of at a customer's first push.
 */
export const REQUIRED_PATHS = [
  "/ingest",
  "/ingest/batch",
  "/ingest/jobs/{id}",
] as const;
