import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { strings } from "../lib/strings";

/** Honestly scoped — Plan 122a A5. The mockup shows a catalog-wide
 *  contradictions queue (open count, bulk resolve, filter by source), but
 *  the real API only has `GET /assets/{id}/contradictions` (per-entity) and
 *  `POST /contradictions/reviews` (a single pair) — there is no aggregate
 *  list endpoint to build that queue against. Rather than fabricate one
 *  (a client-side loop over every known asset, or invented counts), this
 *  screen states the real capability and routes to where contradictions are
 *  actually reviewable today: the Entity screen (Plan 122a A3, shipped).
 *  See the "GOVERN backend gaps" decision, 2026-08-18. */
export default function ContradictionsRoute() {
  const navigate = useNavigate();
  const [id, setId] = useState("");

  const open = () => {
    const trimmed = id.trim();
    if (trimmed.length === 0) return;
    navigate(`/entity/${encodeURIComponent(trimmed)}`);
  };

  return (
    <div className="p-8">
      <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.contradictionsTitle}</h1>
      <p className="mb-5 text-[12.5px] text-gowl-t5">{strings.contradictionsDescription}</p>

      <div className="max-w-[560px] rounded-lg border border-gowl-line bg-gowl-panel p-5">
        <p className="mb-4 text-[12.5px] leading-relaxed text-gowl-t3">{strings.contradictionsScopeNote}</p>
        <div className="flex gap-2">
          <input
            value={id}
            onChange={(e) => setId(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") open();
            }}
            placeholder={strings.contradictionsSearchPlaceholder}
            className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[12px] text-gowl-t1"
          />
          <button
            type="button"
            onClick={open}
            disabled={id.trim().length === 0}
            className="rounded-md bg-gowl-accent px-4 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {strings.contradictionsSearchSubmit}
          </button>
        </div>
      </div>
    </div>
  );
}
