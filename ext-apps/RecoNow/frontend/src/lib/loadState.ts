/** What a period-scoped screen should render right now.
 *
 *  **Found live**: `/workingpaper` rendered "Loading…" forever with no client
 *  selected, because its fetch effect returned early and never cleared the
 *  flag. Eight screens shared the shape. A screen that says "loading" while
 *  waiting for a choice nobody has been asked to make looks broken, and the
 *  user has no way to tell it apart from one that is genuinely stuck.
 *
 *  Four states, because collapsing any two of them loses a distinction a
 *  reader acts on:
 *
 *  - `no-workspace` — pick a client and period first. Not an error.
 *  - `loading` — asked, waiting.
 *  - `empty` — asked, answered, nothing there. Different from `loading`, and
 *    only one of the two means the user should do something.
 *  - `ready` — data to show. */
export type LoadState = "no-workspace" | "loading" | "empty" | "ready";

export function loadStateFor({
  clientId,
  periodId,
  loading,
  data,
}: {
  readonly clientId: string | null | undefined;
  readonly periodId: string | null | undefined;
  readonly loading: boolean;
  readonly data: unknown;
}): LoadState {
  // Checked before `data` on purpose: switching client must not leave the
  // previous client's figures on screen while the new ones load.
  if (!clientId || !periodId) return "no-workspace";
  // Data present wins over `loading` — a refresh over an existing answer must
  // not blank the screen, since the previous answer beats a spinner.
  if (data) return "ready";
  return loading ? "loading" : "empty";
}
