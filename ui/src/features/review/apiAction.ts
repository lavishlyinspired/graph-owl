/** Shared by every `QueueConfig.performAction` implementation, never by
 *  `ReviewQueue.tsx` itself: catches the server's 409 (a second decision
 *  on an already-decided entry) and turns it into `{conflict: true}`
 *  rather than a throw, and flattens any other `ApiError` into a plain
 *  `Error` whose `message` carries the server's own detail — so the
 *  generic component can read a thrown error's `.message` without ever
 *  importing `ApiError` itself. */

import { ApiError } from "../../api";

export async function performAsAction(action: () => Promise<unknown>): Promise<{ conflict: boolean }> {
  try {
    await action();
    return { conflict: false };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.problem.status === 409) return { conflict: true };
      // `detail` is a required field on `Problem`, never absent — no
      // fallback to `title` needed for a value the type already guarantees.
      throw new Error(err.problem.detail);
    }
    throw err;
  }
}
