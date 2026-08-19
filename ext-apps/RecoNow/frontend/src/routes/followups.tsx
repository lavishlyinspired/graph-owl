import { useState } from "react";
import { SectionTabs } from "../components/SectionTabs";
import { sectionsFor } from "../lib/sections";
import type { Capability } from "../lib/sections";
import FollowupsMainPanel from "../panels/followupsMain";
import FollowUpDraftsPanel from "../panels/followUpDrafts";
import SuppliersPanel from "../panels/suppliers";

/** Followups — Plan 123 Slice D.
 *
 *  You chase a supplier, so the directory belongs with the chasing.
 *
 *  The panels below were separate routes until this slice. Each is unchanged;
 *  only where it is reached from moved. `sections.ts` records why each one
 *  lives here, and `sections.test.ts` fails if any capability the 30-route
 *  console had is left without a home. */
const PANELS: Partial<Record<Capability, React.ComponentType>> = {
  "follow-ups": FollowupsMainPanel,
  "follow-up-drafts": FollowUpDraftsPanel,
  "suppliers": SuppliersPanel,
};

export default function FollowupsRoute() {
  const sections = sectionsFor("followups");
  const [active, setActive] = useState<Capability>(sections[0]!.capability);
  const Panel = PANELS[active] ?? PANELS[sections[0]!.capability]!;

  return (
    <div>
      <SectionTabs route="followups" active={active} onSelect={setActive} />
      <Panel />
    </div>
  );
}
