/** Deep-linking via query params — the same mechanism `App.tsx`'s own
 *  `readParam`/`writeParam` already use for the asset explorer's `?asset=`.
 *  Extracted here, out of `features/vocabulary/`, once a second feature
 *  (`features/review/`, Epic 42 Slice C) needed the identical two
 *  functions — Slice A's own note named this exact trigger: duplicating
 *  four lines once is not a reason to share a module, a second feature
 *  needing them is. `App.tsx` is explicitly not where new code should
 *  grow (see the file-level comment at its own top). */

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
