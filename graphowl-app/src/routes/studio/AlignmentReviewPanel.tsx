import { useEffect, useState } from "react";
import {
  fetchAlignmentReviewQueue,
  fetchWhoAmI,
  isAdminOnlyError,
  upsertAlignment,
  type AlignmentReviewEntry,
} from "../../lib/api";
import { confirmAlignmentRequest, describeAlignment, formatConfidence, rejectAlignmentRequest } from "../../lib/ontology/alignmentReview";
import { strings } from "../../lib/strings";

const PANEL = "rounded-lg border border-gowl-line bg-gowl-panel p-4";

/** Ports the archived review-queue UI onto the exact backend it already
 *  called (`GET /alignments/review`, `POST /alignments`, Epic 104 Slice
 *  D) — see `plans/ontology-alignment-review.md`. `graphowl-app` has no
 *  generic `QueueConfig` framework the way the archived console did, so
 *  this is a focused, standalone panel rather than a 5th entry in an
 *  abstraction that does not exist here — same call this session already
 *  made for the ontology editor. */
export function AlignmentReviewPanel() {
  const [entries, setEntries] = useState<readonly AlignmentReviewEntry[] | null>(null);
  const [error, setError] = useState(false);
  const [forbidden, setForbidden] = useState(false);
  const [busySubject, setBusySubject] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const load = () => {
    fetchAlignmentReviewQueue()
      .then(setEntries)
      .catch(() => setError(true));
  };

  useEffect(load, []);

  const act = async (entry: AlignmentReviewEntry, kind: "confirm" | "reject") => {
    setBusySubject(entry.subject);
    setForbidden(false);
    setMessage(null);
    try {
      const request =
        kind === "confirm"
          ? confirmAlignmentRequest(entry, (await fetchWhoAmI()).name)
          : rejectAlignmentRequest(entry);
      await upsertAlignment(request);
      setMessage(kind === "confirm" ? strings.studioOntologyAlignmentsConfirmed : strings.studioOntologyAlignmentsRejected);
      load();
    } catch (caught: unknown) {
      if (isAdminOnlyError(caught)) setForbidden(true);
      else setMessage(strings.studioOntologyAlignmentsActionFailed);
    } finally {
      setBusySubject(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <p className="text-[15px] text-gowl-t6">{strings.studioOntologyAlignmentsScopeNote}</p>

      {forbidden && <p className="text-[16.5px] text-gowl-bad">{strings.studioOntologyAlignmentsAdminOnly}</p>}
      {message && <p className="text-[16.5px] text-gowl-t2">{message}</p>}
      {error && <p className="text-[16.5px] text-gowl-bad">{strings.studioOntologyAlignmentsActionFailed}</p>}

      {!error && entries && entries.length === 0 && (
        <p className="text-[16.5px] text-gowl-t5">{strings.studioOntologyAlignmentsEmpty}</p>
      )}

      {entries?.map((entry) => (
        <div key={entry.subject} className={PANEL}>
          <div className="mb-2 flex items-start justify-between gap-3">
            <div className="text-[17px] text-gowl-t1">{describeAlignment(entry)}</div>
            <span className="whitespace-nowrap font-mono text-[14.5px] text-gowl-t6">
              {formatConfidence(entry.confidence)}
            </span>
          </div>

          <div className="mb-3 grid grid-cols-2 gap-3 text-[15.5px]">
            <div>
              <div className="text-gowl-t5">{strings.studioOntologyAlignmentsLeft}</div>
              <div className="break-all font-mono text-gowl-t2">{entry.left ?? "—"}</div>
            </div>
            <div>
              <div className="text-gowl-t5">{strings.studioOntologyAlignmentsRight}</div>
              <div className="break-all font-mono text-gowl-t2">{entry.right ?? "—"}</div>
            </div>
          </div>

          <div className="mb-3 text-[15px] text-gowl-t5">
            {`${strings.studioOntologyAlignmentsSource}: ${entry.sourceKind ?? "—"}${entry.sourceDetail ? ` — ${entry.sourceDetail}` : ""}`}
          </div>

          {entry.lossyReverse && (
            <div className="mb-3 text-[15px] text-gowl-amber">{strings.studioOntologyAlignmentsLossyReverse}</div>
          )}

          <div className="flex gap-2">
            <button
              type="button"
              disabled={busySubject === entry.subject}
              onClick={() => void act(entry, "confirm")}
              className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] text-gowl-bg disabled:opacity-40"
            >
              {strings.studioOntologyAlignmentsConfirm}
            </button>
            <button
              type="button"
              disabled={busySubject === entry.subject}
              onClick={() => void act(entry, "reject")}
              className="rounded-md border border-gowl-bad px-3 py-1.5 text-[16px] text-gowl-bad disabled:opacity-40"
            >
              {strings.studioOntologyAlignmentsReject}
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
