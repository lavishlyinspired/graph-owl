import { useState } from "react";
import ValidationRoute from "./validation-view";
import ContradictionsRoute from "./contradictions-view";
import ResolutionRoute from "./resolution-view";
import DriftRoute from "./drift-view";
import GovernanceRoute from "./governance";
import { strings } from "../lib/strings";

const TABS = ["validation", "contradictions", "resolution", "drift", "governance"] as const;
type Tab = (typeof TABS)[number];

const TAB_LABEL: Record<Tab, string> = {
  validation: strings.governTabValidation,
  contradictions: strings.governTabContradictions,
  resolution: strings.governTabResolution,
  drift: strings.governTabDrift,
  governance: strings.governTabGovernance,
};

/** Folded in from five standalone routes (`validation-view`, `contradictions-view`,
 *  `resolution-view`, `drift-view`, `governance`) into one tabbed page, matching
 *  Vocabulary Studio's own tab pattern — one nav slot absorbing five features
 *  rather than five separate sidebar entries. Each existing page component is
 *  reused unchanged as a tab's content; none of their own internals moved. */
export default function GovernRoute() {
  const [tab, setTab] = useState<Tab>("validation");

  return (
    <div className="flex h-full flex-col">
      <div className="px-8 pt-8">
        <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.governTitle}</h1>
        <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.governDescription}</p>

        <div className="mb-0 flex gap-1 border-b border-gowl-line">
          {TABS.map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={`px-3 py-2 text-[14.5px] ${
                tab === t ? "border-b-2 border-gowl-accent text-gowl-accent" : "text-gowl-t5 hover:text-gowl-t2"
              }`}
            >
              {TAB_LABEL[t]}
            </button>
          ))}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === "validation" && <ValidationRoute />}
        {tab === "contradictions" && <ContradictionsRoute />}
        {tab === "resolution" && <ResolutionRoute />}
        {tab === "drift" && <DriftRoute />}
        {tab === "governance" && <GovernanceRoute />}
      </div>
    </div>
  );
}
