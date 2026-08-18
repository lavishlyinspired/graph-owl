/** Per-queue configuration — Epic 42 Slice D, the proof of decision 2.
 *  `ReviewQueue.tsx` never names a queue; every difference between merge
 *  candidates, extraction claims, drift, and proposals lives here, in a
 *  `QueueConfig` implementation, or it does not exist at all. The same
 *  "config carries the difference, the component stays generic" split
 *  `vocabularies.ts`/`VocabularyBrowser.tsx` already proved in Slice B.
 *
 *  **The evidence renderer is the only genuinely bespoke part** (the
 *  plan's own words for this slice): a list row is a summary line plus a
 *  detail line, generic across every queue; the detail *pane* is
 *  `fetchDetail`'s returned content, which is where a side-by-side asset
 *  diff, a highlighted source passage, a declared-vs-actual value, or a
 *  proposal's discussion thread each live in their own config file.
 *
 *  **Every action reduces to one of three shapes**, which is what makes
 *  Merge/Reject/Defer, Confirm/Reject/Edit, Apply/Ignore, and Accept/Reject
 *  representable without the component ever branching on which queue it
 *  is: `instant` (click, optionally through a static confirmation dialog —
 *  Epic 17's reversibility notice before a merge), `withText` (click,
 *  through a modal collecting one required string — a rejection reason
 *  *or* an extraction claim's corrected value; the component does not care
 *  which, only the config's own `performAction` does), and `clientOnly`
 *  (defer: no server call, because "not deciding yet" and "leave it
 *  pending" are the same state — Epic 17 has no defer status to write). */

import type { ReactNode } from "react";

export interface QueueEntry {
  readonly id: string;
  readonly status: string;
  /** Primary badge text in the list row — a score or confidence, e.g.
   *  "87% match" or "0.82 confidence". */
  readonly summary: string;
  /** Secondary line — evidence, a field name, whatever orients a reviewer
   *  before they open the entry. */
  readonly detail: string;
  /** Present once decided — "confirmed by alice", "applied automatically". */
  readonly decidedSummary?: string;
  /** Present only once rejected/ignored with a reason. */
  readonly reason?: string;
}

export interface QueueStatus {
  readonly key: string;
  readonly label: string;
}

export interface QueueAction {
  readonly key: string;
  readonly label: string;
  readonly danger?: boolean;
  readonly kind: "instant" | "withText" | "clientOnly";
  /** `instant` only — omit for no confirmation dialog at all. */
  readonly confirmTitle?: string;
  readonly confirmBody?: string;
  /** `withText` only. */
  readonly textModalTitle?: string;
  readonly textModalHint?: string;
  readonly textPlaceholder?: string;
  /** Shown after a successful `instant`/`withText` action. Defaults to
   *  `"${label} done."` when absent — every action name here happens to
   *  inflect cleanly as `-ed` (Merged, Rejected, Applied, Ignored,
   *  Accepted, Edited), but guessing that generally would be the kind of
   *  cleverness that breaks on the next queue's verb; naming it here does
   *  not. */
  readonly doneMessage?: string;
}

export interface QueueConfig {
  readonly key: string;
  readonly label: string;
  readonly subtitle: string;
  readonly statuses: readonly QueueStatus[];
  /** The status key an entry starts in and actions apply to — "pending"
   *  for every queue so far, but named rather than hardcoded so a queue
   *  whose vocabulary genuinely differs is not blocked by this shape. */
  readonly openStatus: string;
  readonly actions: readonly QueueAction[];
  emptyTitle(status: string): string;
  emptyDescription(status: string): string;
  fetchEntries(status: string): Promise<readonly QueueEntry[]>;
  /** The evidence renderer — the one bespoke part. */
  fetchDetail(entry: QueueEntry): Promise<ReactNode>;
  /** Resolves `{conflict: true}` for the two-reviewers case (a decided
   *  entry decided again) instead of throwing, so `ReviewQueue.tsx` can
   *  turn that into a plain refresh without importing anything from
   *  `../../api` to recognize it — the same "config carries the
   *  transport-specific knowledge, the component stays generic" split as
   *  everything else here. Any other failure throws a plain `Error`. */
  performAction(entry: QueueEntry, actionKey: string, text: string | undefined): Promise<{ conflict: boolean }>;
}
