import { useEffect, useState } from "react";
import { fetchGlossaryTerms, type GlossaryTerm } from "../../lib/api";
import { strings } from "../../lib/strings";

/** "No RDF vocabulary visible; read-only share" (Plan 122a A7 AC) — the
 *  same term data as Build, minus status/relations/usage/every editing
 *  action, for sharing outside the team that manages the vocabulary. */
export function BusinessTab({ glossaryId }: { readonly glossaryId: string }) {
  const [terms, setTerms] = useState<readonly GlossaryTerm[] | null>(null);

  useEffect(() => {
    fetchGlossaryTerms(glossaryId).then(setTerms);
  }, [glossaryId]);

  if (!terms) {
    return <div className="text-[14.5px] text-gowl-t5">{strings.studioLoading}</div>;
  }

  const approved = terms.filter((t) => t.status === "approved");

  return (
    <div>
      <p className="mb-4 text-[14px] text-gowl-t5">{strings.businessDescription}</p>
      {approved.length === 0 ? (
        <p className="text-[14.5px] text-gowl-t5">{strings.businessEmpty}</p>
      ) : (
        <div className="rounded-lg border border-gowl-line bg-gowl-panel">
          {approved.map((term) => (
            <div key={term.id} className="border-b border-gowl-row p-4 last:border-b-0">
              <div className="mb-1 text-[15.5px] font-semibold text-gowl-t1">{term.name}</div>
              <p className="text-[14px] text-gowl-t3">{term.definition || "—"}</p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
