import { useEffect, useState } from "react";
import { fetchGlossaryTerms, setTermReviewers, transitionTerm, type GlossaryTerm, type TermStatus } from "../../lib/api";
import { strings } from "../../lib/strings";

const STATUS_LABEL: Record<TermStatus, string> = {
  draft: strings.glossaryStatusDraft,
  inReview: strings.glossaryStatusInReview,
  approved: strings.glossaryStatusApproved,
  deprecated: strings.glossaryStatusDeprecated,
};

/** "Candidates → promote" (Plan 122a A7 AC) maps onto the real term
 *  lifecycle (`TermStatus`: draft → inReview → approved → deprecated,
 *  `POST /glossary-terms/{id}/transitions`) rather than a separate
 *  candidate-staging concept — that one (system-suggested altLabels/
 *  concepts with a match score) is the real, not-yet-built gap tracked as
 *  the Proposals tab. */
export function GlossaryTab({ glossaryId }: { readonly glossaryId: string }) {
  const [terms, setTerms] = useState<readonly GlossaryTerm[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reviewerInputs, setReviewerInputs] = useState<Readonly<Record<string, string>>>({});

  const load = () => {
    fetchGlossaryTerms(glossaryId).then(setTerms);
  };

  useEffect(load, [glossaryId]);

  if (!terms) {
    return <div className="text-[17px] text-gowl-t5">{strings.studioLoading}</div>;
  }

  const candidates = terms.filter((t) => t.status === "draft" || t.status === "inReview");
  const approved = terms.filter((t) => t.status === "approved");

  const promote = async (term: GlossaryTerm) => {
    setBusy(true);
    setError(null);
    try {
      await transitionTerm(term.id, term.status === "draft" ? "inReview" : "approved");
      load();
    } catch {
      // The real precondition (`set_term_reviewers` must run first for an
      // approval) is what usually lands here — surfaced, not swallowed.
      setError(strings.glossaryPromoteError);
    } finally {
      setBusy(false);
    }
  };

  const assignReviewer = async (term: GlossaryTerm) => {
    const reviewer = reviewerInputs[term.id]?.trim();
    if (!reviewer) return;
    setBusy(true);
    setError(null);
    try {
      await setTermReviewers(term.id, [reviewer]);
      setReviewerInputs((prev) => ({ ...prev, [term.id]: "" }));
    } catch {
      setError(strings.glossaryReviewerError);
    } finally {
      setBusy(false);
    }
  };

  if (terms.length === 0) {
    return <p className="text-[17px] text-gowl-t5">{strings.glossaryEmpty}</p>;
  }

  return (
    <div>
      {error && <p className="mb-3 text-[16.5px] text-gowl-bad">{error}</p>}
      <div className="grid grid-cols-2 gap-4">
        <div className="rounded-lg border border-gowl-line bg-gowl-panel">
          <div className="border-b border-gowl-line bg-gowl-panel-2 px-3 py-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">
            {strings.glossaryCandidatesTitle}
          </div>
          {candidates.map((term) => (
            <div key={term.id} className="border-b border-gowl-row px-3 py-2 last:border-b-0">
              <div className="mb-1.5 flex items-center justify-between">
                <div>
                  <div className="text-[16.5px] text-gowl-t1">{term.name}</div>
                  <div className="text-[15px] text-gowl-t6">{STATUS_LABEL[term.status]}</div>
                </div>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => promote(term)}
                  className="rounded-md bg-gowl-accent px-2 py-1 text-[15px] font-semibold text-gowl-accent-on disabled:opacity-40"
                >
                  {term.status === "draft" ? strings.glossarySubmitForReview : strings.glossaryPromote}
                </button>
              </div>
              {term.status === "inReview" && (
                <div className="flex gap-1">
                  <input
                    value={reviewerInputs[term.id] ?? ""}
                    onChange={(e) => setReviewerInputs((prev) => ({ ...prev, [term.id]: e.target.value }))}
                    placeholder={strings.glossaryReviewerPlaceholder}
                    className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1 text-[15px] text-gowl-t1"
                  />
                  <button
                    type="button"
                    disabled={busy || !reviewerInputs[term.id]?.trim()}
                    onClick={() => assignReviewer(term)}
                    className="rounded-md border border-gowl-line-2 px-2 py-1 text-[15px] text-gowl-t2 disabled:opacity-40"
                  >
                    {strings.glossaryAssignReviewer}
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>

      <div className="rounded-lg border border-gowl-line bg-gowl-panel">
        <div className="border-b border-gowl-line bg-gowl-panel-2 px-3 py-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">
          {strings.glossaryApprovedTitle}
        </div>
        {approved.map((term) => (
          <div key={term.id} className="border-b border-gowl-row px-3 py-2 text-[16.5px] text-gowl-t1 last:border-b-0">
            {term.name}
          </div>
        ))}
      </div>
      </div>
    </div>
  );
}
