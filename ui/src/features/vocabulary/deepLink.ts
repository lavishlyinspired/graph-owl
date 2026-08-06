/** Deep-linking via query params — the same mechanism `App.tsx`'s own
 *  `readParam`/`writeParam` already use for the asset explorer's `?asset=`.
 *  Kept local to this feature rather than imported from `App.tsx`: the
 *  research behind Epic 42 found no shared URL-params module to import from,
 *  and `App.tsx` is explicitly not where new code should grow (see the
 *  file-level `eslint-disable` comment at its own top). A future feature
 *  needing the same two functions is a real reason to extract a shared
 *  module; duplicating four lines once is not. */

export function readParam(name: string): string | null {
  return new URLSearchParams(window.location.search).get(name);
}

export function writeParam(name: string, value: string | null): void {
  const params = new URLSearchParams(window.location.search);
  if (value === null) params.delete(name);
  else params.set(name, value);
  const query = params.toString();
  window.history.replaceState(null, "", query ? `?${query}` : window.location.pathname);
}
