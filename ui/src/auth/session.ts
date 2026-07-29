/** Where a session survives a page reload.
 *
 *  **The bug this exists to fix**: the refresh token lived in a module variable
 *  and nowhere else, so every reload lost it. The app fell back to the sign-in
 *  screen, and because the provider still held its own SSO cookie the user was
 *  bounced through a login they had already completed — which reads as "it did
 *  not work" even though it had.
 *
 *  **`sessionStorage`, not `localStorage`.** `localStorage` would also fix the
 *  reload, and would survive a browser restart, at the cost of leaving a
 *  long-lived credential on disk for any future XSS to collect.
 *  `sessionStorage` is scoped to the tab: a reload, a navigation and a
 *  crash-restore all keep the session, and closing the tab ends it. The
 *  **access token stays in memory only**, unchanged — this is the smallest
 *  persistence that makes a reload work, not a relaxation of that rule.
 *
 *  Storage is passed in rather than reached for, the same way `pkce.ts` does
 *  it, so every decision here is testable without a browser.
 */

const REFRESH_KEY = "graphowl.refresh.v1";

/** Park the refresh token, or clear it when the session ends.
 *
 *  `null` clears rather than storing the string `"null"` — the distinction
 *  matters because `getItem` returns `null` for both, and a stored `"null"`
 *  would be handed to the provider as a credential on the next load.
 */
export function rememberSession(storage: Storage, refreshToken: string | null): void {
  try {
    if (refreshToken) storage.setItem(REFRESH_KEY, refreshToken);
    else storage.removeItem(REFRESH_KEY);
  } catch {
    // Storage can be unavailable — private mode, a quota, an enterprise policy.
    // A session that does not survive a reload is worse than one that does, and
    // far better than a page that refuses to render. Deliberately not fatal.
  }
}

/** The parked refresh token, if there is one.
 *
 *  Unlike the PKCE verifier this is **not** consumed on read: a refresh token
 *  is used repeatedly across a session, and clearing it here would end the
 *  session on the first restore.
 */
export function storedSession(storage: Storage): string | null {
  try {
    const stored = storage.getItem(REFRESH_KEY);
    // An empty string is not a token. It reaches here from a provider that
    // returned one, and sending it back produces a confusing 401 rather than
    // the honest "no session" this should report.
    return stored === null || stored.length === 0 ? null : stored;
  } catch {
    return null;
  }
}
