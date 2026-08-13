/** Putting an uploaded file into the graph, once, for every surface that
 *  offers one — Plan 108.
 *
 *  **Extracted because there are now two places to upload from.** The Packs
 *  admin panel has always had upload surfaces; the reconciliation workspace
 *  now has the same three, presented as a workflow rather than as
 *  administration. If they named the import source differently, the same file
 *  uploaded from the two places would land in two named graphs and be counted
 *  twice — a duplicate that looks exactly like a supplier filing an invoice
 *  under two GSTINs. */

import { api } from "../../api";
import type { PackImportSurface } from "./packSurfaces";

/** The first `gst:period "..."`-shaped literal in the converted Turtle, used
 *  to scope the import source. Reading it back out of the Turtle rather than
 *  changing every surface's `convert` signature keeps `PackImportSurface`
 *  generic — a surface with no period concept simply produces none, and the
 *  import falls back to the pack-wide source name. */
export function invoicePeriod(turtle: string): string | null {
  const match = turtle.match(/:period\s+"(\d{4}-\d{2})"/);
  return match?.[1] ?? null;
}

export interface ImportOutcome {
  readonly landed: number;
  readonly skipped: number;
  readonly rejected: number;
  /** How many records the pack's own converter read out of the file, before
   *  any of it reached the graph. Zero is not an error — a period nobody filed
   *  against is a legitimate and informative answer. */
  readonly count: number;
  readonly source: string;
}

/** Convert, then import. **Convert first, always**: a file the pack cannot
 *  read must never reach the graph, because a partial import is far harder to
 *  undo than a refused one. */
export async function importThroughSurface(
  packId: string,
  surface: PackImportSurface,
  text: string,
): Promise<ImportOutcome> {
  const { turtle, count } = surface.convert(text);

  // Scoped by period rather than the pack's own `${packId}-${surface.key}`
  // source name — that name is also what the pack's *bundled demo fixture*
  // imports into, so a real upload whose invoice numbers happened to coincide
  // with the fixture's would silently skip as "already imported". Scoping by
  // period both avoids that collision and gives a natural idempotence key:
  // re-uploading the same period is a no-op, uploading a different one lands
  // separately.
  const period = invoicePeriod(turtle);
  const source = period ? `${packId}-${surface.key}-${period}` : `${packId}-${surface.key}`;

  if (count === 0) return { landed: 0, skipped: 0, rejected: 0, count, source };

  const outcome = await api.importRdf(source, turtle);
  return {
    landed: outcome.landed.length,
    skipped: outcome.skipped.length,
    rejected: outcome.rejected.length,
    count,
    source,
  };
}
